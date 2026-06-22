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
pub struct AppState {
    pub config: Config,
    pub asr_engine: Arc<Mutex<AsrEngine>>,
    pub tts_engine: Arc<Mutex<TtsEngine>>,
    pub vad_engine: Arc<Mutex<VadEngine>>,
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
                                    &state,
                                    &tx,
                                    &mut asr_stream,
                                    &mut is_speaking,
                                    &mut audio_buffer,
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
                                        &state,
                                        &tx,
                                        &mut asr_stream,
                                        &mut is_speaking,
                                        &mut audio_buffer,
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
async fn handle_text_message(
    msg: ClientMessage,
    state: &Arc<AppState>,
    tx: &mpsc::UnboundedSender<ServerMessage>,
    asr_stream: &mut Option<sherpa_onnx::OnlineStream>,
    is_speaking: &mut bool,
    audio_buffer: &mut Vec<i16>,
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
            let _ = tx.send(ServerMessage::vad_silence());
            Ok(())
        }

        ClientMessage::AsrAudio { data, sample_rate } => {
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

            let _sr = sample_rate.unwrap_or(16000);
            process_audio_chunk(
                state, tx, asr_stream, is_speaking, audio_buffer, &samples,
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
                            let chunk_size = (sample_rate as usize) / 5; // ~200ms chunks
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
async fn process_audio_chunk(
    state: &Arc<AppState>,
    tx: &mpsc::UnboundedSender<ServerMessage>,
    asr_stream: &mut Option<sherpa_onnx::OnlineStream>,
    is_speaking: &mut bool,
    audio_buffer: &mut Vec<i16>,
    samples: &[i16],
) {
    // Run VAD
    let mut vad_engine = state.vad_engine.lock().await;
    let vad_state = vad_engine.process(samples);
    drop(vad_engine);

    // Notify client of VAD state changes
    match vad_state {
        VadState::Speech if !*is_speaking => {
            *is_speaking = true;
            let _ = tx.send(ServerMessage::vad_speech());
        }
        VadState::Silence if *is_speaking => {
            *is_speaking = false;
            let _ = tx.send(ServerMessage::vad_silence());

            if let Some(ref stream) = *asr_stream {
                if !audio_buffer.is_empty() {
                    let engine = state.asr_engine.lock().await;
                    let result = engine.recognize(stream, audio_buffer);
                    if let Some(r) = result {
                        if !r.text.is_empty() {
                            let _ = tx.send(ServerMessage::final_result(&r.text, 0.92));
                        }
                    }
                    audio_buffer.clear();
                }
            }
        }
        _ => {}
    }

    // Feed audio to ASR stream for interim results
    if let Some(ref stream) = *asr_stream {
        audio_buffer.extend_from_slice(samples);
        let engine = state.asr_engine.lock().await;
        let recent: Vec<i16> = audio_buffer.iter().copied().collect();
        let result = engine.recognize(stream, &recent);
        if let Some(r) = result {
            if !r.text.is_empty() {
                let _ = tx.send(ServerMessage::interim(&r.text));
            }
        }
    }
}

/// Handle binary ASR audio frame.
async fn handle_asr_audio(
    pcm_bytes: &[u8],
    state: &Arc<AppState>,
    tx: &mpsc::UnboundedSender<ServerMessage>,
    asr_stream: &mut Option<sherpa_onnx::OnlineStream>,
    is_speaking: &mut bool,
    audio_buffer: &mut Vec<i16>,
) {
    let samples: Vec<i16> = pcm_bytes
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    process_audio_chunk(state, tx, asr_stream, is_speaking, audio_buffer, &samples).await;
}
