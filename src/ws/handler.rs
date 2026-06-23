use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Instant;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::SinkExt;
use futures_util::StreamExt;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

use crate::asr::engine::AsrEngine;
use crate::asr::vad::{VadEngine, VadState};
use crate::config::Config;
use crate::error::VoiceServerError;
use crate::tts::engine::TtsEngine;
use crate::ws::protocol::{
    self, parse_binary_frame, ClientMessage, ServerMessage,
};

/// Shared application state accessible from all WebSocket handlers.
///
/// Note: VAD engine is NOT shared — each connection creates its own instance
/// because VoiceActivityDetector maintains per-instance state (audio buffers,
/// model activations). Sharing it would corrupt detection across connections.
pub struct AppState {
    pub config: Config,
    pub asr_engine: Arc<Mutex<AsrEngine>>,
    pub tts_engine: Arc<Mutex<TtsEngine>>,
    pub active_connections: Arc<AtomicUsize>,
    pub max_connections: usize,
}

/// GET /ws — WebSocket upgrade handler.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_connection(socket, state))
}

/// GET /health — Health check endpoint.
pub async fn health_handler() -> impl IntoResponse {
    axum::Json(serde_json::json!({"status": "ok"}))
}

/// Per-connection WebSocket handler.
async fn handle_connection(socket: WebSocket, state: Arc<AppState>) {
    let connection_id = uuid::Uuid::new_v4();
    let connected_at = Instant::now();

    // Check connection limit
    let current = state
        .active_connections
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if current >= state.max_connections {
        state
            .active_connections
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        warn!(
            "Connection limit reached ({}/{})",
            current, state.max_connections
        );
        return;
    }

    info!(
        "WebSocket connected: {} (active: {})",
        connection_id,
        current + 1
    );

    // Channel to send outgoing messages from background tasks
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMessage>();

    // Split socket for concurrent send/receive
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Spawn a task to forward channel messages to WebSocket
    let forward_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let text = serde_json::to_string(&msg).unwrap();
            if ws_sender.send(Message::Text(text)).await.is_err() {
                break;
            }
        }
    });

    // Per-connection session state
    let mut asr_stream: Option<sherpa_onnx::OnlineStream> = None;
    let mut is_speaking = false;
    let mut audio_buffer: Vec<i16> = Vec::new();
    // Tracks how many samples from audio_buffer have already been fed to the ASR
    // stream. On each process_audio_chunk call, only feed audio_buffer[asr_fed_len..]
    // to avoid re-feeding the same audio, which corrupts the ASR decoder state.
    let mut asr_fed_len: usize = 0;

    // Per-connection VAD engine (NOT shared — each connection needs its own
    // VoiceActivityDetector instance because VAD maintains per-stream state).
    // ~5MB per instance for Silero VAD model.
    let mut vad_engine = match VadEngine::new(&state.config.vad) {
        Ok(engine) => engine,
        Err(e) => {
            error!("Failed to create per-connection VAD engine: {}", e);
            state
                .active_connections
                .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            return;
        }
    };

    // Main message receive loop
    loop {
        tokio::select! {
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ClientMessage>(&text) {
                            Ok(msg) => {
                                if let Err(e) = handle_text_message(
                                    msg,
                                    &mut vad_engine,
                                    &state,
                                    &tx,
                                    &mut asr_stream,
                                    &mut is_speaking,
                                    &mut audio_buffer,
                                    &mut asr_fed_len,
                                    connection_id,
                                )
                                .await
                                {
                                    error!("Error handling message [{}]: {}", connection_id, e);
                                    let _ = tx.send(ServerMessage::error("HANDLER_ERROR", e.to_string()));
                                }
                            }
                            Err(e) => {
                                warn!("Invalid JSON from [{}]: {}", connection_id, e);
                                let _ = tx.send(ServerMessage::error(
                                    "PARSE_ERROR",
                                    format!("Invalid JSON: {}", e),
                                ));
                            }
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        if let Some(payload) = parse_binary_frame(&data) {
                            match payload {
                                protocol::BinaryPayload::AsrAudio(pcm_bytes) => {
                                    handle_asr_audio(
                                        &pcm_bytes,
                                        &mut vad_engine,
                                        &state,
                                        &tx,
                                        &mut asr_stream,
                                        &mut is_speaking,
                                        &mut audio_buffer,
                                        &mut asr_fed_len,
                                    )
                                    .await;
                                }
                                protocol::BinaryPayload::TtsAudio(_) => {
                                    debug!("Received unexpected TTS binary frame from client");
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Ping(_))) => {
                        // axum-tungstenite handles pings automatically,
                        // but we also respond with our protocol pong
                        let _ = tx.send(ServerMessage::pong(0));
                    }
                    Some(Ok(Message::Pong(_))) => {
                        // Ignore pong responses
                    }
                    Some(Ok(Message::Close(_))) => {
                        info!("WebSocket closed by client: {}", connection_id);
                        break;
                    }
                    Some(Err(e)) => {
                        error!("WebSocket error [{}]: {}", connection_id, e);
                        break;
                    }
                    None => break,
                }
            }
        }
    }

    // Cleanup
    drop(tx); // Close the channel → forward task will stop
    let _ = forward_task.await;

    state
        .active_connections
        .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    let elapsed = connected_at.elapsed();
    info!(
        "WebSocket disconnected: {} (duration: {:?})",
        connection_id, elapsed
    );
}

/// Handle a parsed text message from the client.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
async fn handle_text_message(
    msg: ClientMessage,
    vad_engine: &mut VadEngine,
    state: &Arc<AppState>,
    tx: &mpsc::UnboundedSender<ServerMessage>,
    asr_stream: &mut Option<sherpa_onnx::OnlineStream>,
    is_speaking: &mut bool,
    audio_buffer: &mut Vec<i16>,
    asr_fed_len: &mut usize,
    connection_id: uuid::Uuid,
) -> Result<(), VoiceServerError> {
    match msg {
        ClientMessage::AsrStart { language: _ } => {
            debug!("ASR start [{}]", connection_id);
            let engine = state.asr_engine.lock().await;
            let stream = engine.create_stream();
            *asr_stream = Some(stream);
            *is_speaking = false;
            audio_buffer.clear();
            *asr_fed_len = 0;
            // Reset per-connection VAD for a new ASR session
            vad_engine.reset();
            let _ = tx.send(ServerMessage::vad_silence());
            Ok(())
        }

        ClientMessage::AsrAudio { data, sample_rate: _ } => {
            let pcm_bytes = base64::Engine::decode(
                &base64::engine::general_purpose::STANDARD,
                &data,
            )
            .map_err(|e| {
                VoiceServerError::WebSocket(format!("Base64 decode error: {}", e))
            })?;

            let samples: Vec<i16> = pcm_bytes
                .chunks_exact(2)
                .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();

            // Engine always uses 16000 Hz per spec
            process_audio_chunk(
                vad_engine, state, tx, asr_stream, is_speaking, audio_buffer, asr_fed_len, &samples,
            )
            .await;
            Ok(())
        }

        ClientMessage::AsrStop => {
            debug!("ASR stop [{}]", connection_id);
            if let Some(stream) = asr_stream.take() {
                let engine = state.asr_engine.lock().await;
                let result = engine.finalize_result(&stream);
                if let Some(r) = result {
                    if !r.text.is_empty() {
                        let confidence = r.timestamps.as_ref()
                            .map(|_| 0.95f32)
                            .unwrap_or(0.90);
                        let _ = tx.send(ServerMessage::final_result(&r.text, confidence));
                    }
                }
            }
            *is_speaking = false;
            audio_buffer.clear();
            *asr_fed_len = 0;
            Ok(())
        }

        ClientMessage::TtsRequest { text, voice } => {
            debug!("TTS request [{}]: {} chars", connection_id, text.len());

            let state = Arc::clone(state);
            let tx = tx.clone();
            let sid = voice.and_then(|v| v.parse::<i32>().ok());

            // Spawn TTS in a blocking task to avoid blocking the event loop
            tokio::task::spawn_blocking(move || {
                let rt = tokio::runtime::Handle::current();
                let _ = rt.block_on(async move {
                    let tts_engine = state.tts_engine.lock().await;
                    match tts_engine.synthesize(&text, sid) {
                        Ok(chunk) => {
                            let pcm_bytes = protocol::f32_to_i16_pcm(&chunk.audio);
                            let sample_rate = chunk.sample_rate;
                            // ~100ms per chunk at output sample rate (chunk_size is in bytes,
                            // each PCM sample is 2 bytes, so sample_rate/5 bytes = 100ms at any rate)
                            let chunk_size = (sample_rate as usize) / 5;
                            if chunk_size == 0 {
                                return;
                            }
                            for pcm_chunk in pcm_bytes.chunks(chunk_size) {
                                let b64 = base64::Engine::encode(
                                    &base64::engine::general_purpose::STANDARD,
                                    pcm_chunk,
                                );
                                let msg = ServerMessage::TtsAudio {
                                    data: b64,
                                    format: "pcm16".into(),
                                    sample_rate,
                                };
                                if tx.send(msg).is_err() {
                                    break;
                                }
                            }
                            let _ = tx.send(ServerMessage::TtsEnd {
                                duration_ms: chunk.duration_ms,
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(ServerMessage::TtsError {
                                code: "SYNTHESIS_ERROR".into(),
                                message: e.to_string(),
                            });
                        }
                    }
                });
            });

            Ok(())
        }

        ClientMessage::TtsCancel => {
            debug!("TTS cancel [{}]", connection_id);
            // TTS runs in spawn_blocking; a proper cancellation would need
            // a CancellationToken. Placeholder for P2-5.
            info!("TTS cancel requested but not yet implemented for running tasks");
            Ok(())
        }

        ClientMessage::Config { key, value } => {
            info!("Config update [{}]: {} = {}", connection_id, key, value);
            Ok(())
        }

        ClientMessage::Ping { timestamp } => {
            let _ = tx.send(ServerMessage::pong(timestamp));
            Ok(())
        }
    }
}

/// Process an audio chunk through VAD + ASR.
///
/// Each WebSocket connection passes its own `vad_engine` — VAD instances are
/// per-connection because VoiceActivityDetector maintains internal state that
/// would be corrupted by interleaved audio from multiple connections.
async fn process_audio_chunk(
    vad_engine: &mut VadEngine,
    state: &Arc<AppState>,
    tx: &mpsc::UnboundedSender<ServerMessage>,
    asr_stream: &mut Option<sherpa_onnx::OnlineStream>,
    is_speaking: &mut bool,
    audio_buffer: &mut Vec<i16>,
    asr_fed_len: &mut usize,
    samples: &[i16],
) {
    // Run VAD (per-connection instance — safe to mutate without locks)
    let vad_state = vad_engine.process(samples);

    // Notify client of VAD state changes
    match vad_state {
        VadState::Speech if !*is_speaking => {
            *is_speaking = true;
            let _ = tx.send(ServerMessage::vad_speech());
        }
        VadState::Silence if *is_speaking => {
            *is_speaking = false;
            let _ = tx.send(ServerMessage::vad_silence());

            // When VAD transitions to silence, finalize the ASR stream.
            // Use finalize_result (marks input_finished + decodes remaining)
            // instead of re-feeding audio_buffer which would corrupt state.
            if let Some(stream) = asr_stream.take() {
                let engine = state.asr_engine.lock().await;
                let result = engine.finalize_result(&stream);
                if let Some(r) = result {
                    if !r.text.is_empty() {
                        let _ = tx.send(ServerMessage::final_result(&r.text, 0.92));
                    }
                }
                audio_buffer.clear();
                *asr_fed_len = 0;
            }
        }
        _ => {}
    }

    // Feed only NEW audio to ASR stream for interim results.
    // Audio that was already fed in previous calls must NOT be re-fed
    // because sherpa-onnx OnlineStream::accept_waveform is additive —
    // re-feeding the same samples corrupts the decoder state.
    if let Some(ref stream) = *asr_stream {
        audio_buffer.extend_from_slice(samples);

        // Determine the new samples not yet fed to ASR
        if *asr_fed_len < audio_buffer.len() {
            let new_samples = &audio_buffer[*asr_fed_len..];
            let engine = state.asr_engine.lock().await;
            let result = engine.recognize(stream, new_samples);
            *asr_fed_len = audio_buffer.len();

            if let Some(r) = result {
                if !r.text.is_empty() {
                    let _ = tx.send(ServerMessage::interim(&r.text));
                }
            }
        }
    }
}

/// Handle binary ASR audio frame.
async fn handle_asr_audio(
    pcm_bytes: &[u8],
    vad_engine: &mut VadEngine,
    state: &Arc<AppState>,
    tx: &mpsc::UnboundedSender<ServerMessage>,
    asr_stream: &mut Option<sherpa_onnx::OnlineStream>,
    is_speaking: &mut bool,
    audio_buffer: &mut Vec<i16>,
    asr_fed_len: &mut usize,
) {
    let samples: Vec<i16> = pcm_bytes
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    process_audio_chunk(vad_engine, state, tx, asr_stream, is_speaking, audio_buffer, asr_fed_len, &samples).await;
}
