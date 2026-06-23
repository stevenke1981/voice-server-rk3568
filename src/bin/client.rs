//! Voice Server WebSocket 客戶端
//!
//! 連接到 voice-server 的 WebSocket 端點 (`ws://192.168.80.213:8081/ws`)，
//! 支援 ASR、TTS、VAD 等雙向通訊，並可將結果存檔。
//!
//! # 使用方式
//!
//! ```bash
//! # TTS 語音合成 + 存成 WAV 檔
//! cargo run --bin client -- --tts "你好世界" -o hello.wav
//!
//! # ASR 語音辨識 + 存成文字檔 (支援 .pcm 和 .wav)
//! cargo run --bin client -- --asr-file ./audio.pcm -o result.txt
//!
//! # ASR 即時串流模式 (chunked, 模擬 streaming)
//! cargo run --bin client -- --asr-file ./audio.pcm --asr-chunk-ms 100 -o result.txt
//!
//! # ASR 單次傳送模式 (不 chunk)
//! cargo run --bin client -- --asr-file ./audio.pcm --asr-chunk-ms 0 -o result.txt
//!
//! # WAV 輸入自動轉 16kHz mono
//! cargo run --bin client -- --asr-file ./recording.wav -o result.txt
//!
//! # 互動模式 (stdin 指令)
//! cargo run --bin client
//! ```
//!
//! # 互動模式指令
//!
//! | 指令                          | 說明                          |
//! |-------------------------------|-------------------------------|
//! | `asr_start [lang]`            | 開始 ASR 辨識                 |
//! | `asr_stop`                    | 停止 ASR 辨識                 |
//! | `asr_audio <base64>`          | 傳送 base64 PCM 音訊          |
//! | `asr_audio_file <path>`       | 傳送二進位 PCM 音訊檔          |
//! | `tts <text>`                 | 請求 TTS 語音合成             |
//! | `tts_cancel`                  | 取消 TTS                      |
//! | `ping`                        | 傳送心跳                      |
//! | `config <key> <json_value>`   | 更新設定                      |
//! | `save <path>`                | 儲存上次 TTS 音訊或 ASR 結果  |
//! | `help`                        | 顯示說明                      |
//! | `quit`                        | 離開                          |

use std::fs;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use hound::WavSpec;
use serde::{Deserialize, Serialize};
use std::marker::Unpin;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

// ── Protocol types (mirrors server's ws::protocol) ─────────

/// Marker byte for ASR audio binary frames (client → server)
const ASR_AUDIO_MARKER: u8 = 0x00;

/// Client → Server JSON messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    AsrStart {
        language: Option<String>,
    },
    AsrAudio {
        data: String,
        sample_rate: Option<u32>,
    },
    AsrStop,
    TtsRequest {
        text: String,
        voice: Option<String>,
    },
    TtsCancel,
    Config {
        key: String,
        value: serde_json::Value,
    },
    Ping {
        timestamp: u64,
    },
}

/// Server → Client JSON messages
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(dead_code)]
enum ServerMessage {
    AsrInterim {
        text: String,
        #[serde(rename = "is_final")]
        is_final: bool,
    },
    AsrFinal {
        text: String,
        #[serde(rename = "is_final")]
        is_final: bool,
        confidence: f32,
    },
    AsrError {
        code: String,
        message: String,
    },
    TtsAudio {
        data: String,
        format: String,
        sample_rate: u32,
    },
    TtsEnd {
        duration_ms: u32,
    },
    TtsError {
        code: String,
        message: String,
    },
    VadState {
        state: String,
    },
    Error {
        code: String,
        message: String,
    },
    Pong {
        timestamp: u64,
    },
}

// ── Global buffer for interactive mode save ────────────────

static LAST_TTS_PCM: LazyLock<Mutex<Option<AccumulatedTts>>> =
    LazyLock::new(|| Mutex::new(None));
static LAST_ASR_TEXT: LazyLock<Mutex<Option<String>>> =
    LazyLock::new(|| Mutex::new(None));

struct AccumulatedTts {
    pcm_data: Vec<u8>,
    sample_rate: u32,
}

// ── CLI arguments ──────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "voice-client", version, about = "Voice Server WebSocket Client")]
struct Args {
    /// WebSocket server URL
    #[arg(short, long, default_value = "ws://192.168.80.213:8081/ws")]
    url: String,

    /// Send a PCM/WAV audio file for ASR then wait for result (non-interactive).
    /// Supports .pcm (raw 16-bit 16kHz mono) and .wav (auto-converted to 16kHz mono).
    #[arg(long)]
    asr_file: Option<PathBuf>,

    /// Language hint for ASR
    #[arg(long)]
    language: Option<String>,

    /// Chunk duration in ms for streaming ASR audio.
    /// Splits the audio file into chunks of this size and sends them one by one
    /// with small delays to simulate a real-time stream.
    /// Set to 0 to send the entire file in one binary frame (single-shot).
    #[arg(long, default_value = "200")]
    asr_chunk_ms: u64,

    /// Text for TTS synthesis (non-interactive)
    #[arg(long)]
    tts: Option<String>,

    /// Output file path (for --tts or --asr-file modes)
    /// If omitted, auto-generates: tts_<timestamp>.wav / asr_<timestamp>.txt
    #[arg(short, long)]
    output: Option<PathBuf>,
}

// ── Main ───────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let args = Args::parse();

    println!("🔊 Voice Server Client");
    println!("   Connecting to: {}", args.url);
    println!();

    // Connect to WebSocket server
    let (ws_stream, response) = match connect_async(&args.url).await {
        Ok(ok) => ok,
        Err(e) => {
            eprintln!("❌ Connection failed: {}", e);
            std::process::exit(1);
        }
    };

    println!("✅ Connected! HTTP status: {}", response.status());
    println!();

    let (mut write, mut read) = ws_stream.split();

    // ── Non-interactive modes ──────────────────────────

    if let Some(asr_path) = args.asr_file {
        run_asr_file_mode(asr_path, args.language, args.asr_chunk_ms, args.output, &mut write, &mut read).await;
        return;
    }

    if let Some(tts_text) = args.tts {
        run_tts_mode(tts_text, args.output, &mut write, &mut read).await;
        return;
    }

    // ── Interactive mode ───────────────────────────────

    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();

    println!("📝 Interactive mode. Type `help` for commands, `quit` to exit.");
    println!();

    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(msg)) => {
                        if let Err(e) = handle_incoming(msg).await {
                            eprintln!("⚠️  Error handling incoming message: {e}");
                        }
                    }
                    Some(Err(e)) => {
                        eprintln!("❌ WebSocket error: {e}");
                        break;
                    }
                    None => {
                        println!("🔌 Connection closed by server.");
                        break;
                    }
                }
            }
            line = lines.next_line() => {
                match line {
                    Ok(Some(input)) => {
                        let input = input.trim().to_string();
                        if input.is_empty() {
                            continue;
                        }
                        if !process_command(&input, &mut write).await {
                            break;
                        }
                    }
                    Ok(None) => {
                        println!("👋 EOF, exiting.");
                        break;
                    }
                    Err(e) => {
                        eprintln!("⚠️  Read error: {e}");
                        break;
                    }
                }
            }
        }
    }

    println!("Disconnected.");
}

// ── ASR file mode ──────────────────────────────────────────

/// Load audio from a file, supporting both .pcm (raw 16-bit mono) and .wav format.
/// For WAV files, extracts PCM data and converts to 16kHz mono if needed.
/// Returns (pcm_bytes, sample_rate, num_channels).
fn load_audio(path: &PathBuf) -> Result<(Vec<u8>, u32, u16), String> {
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "wav" => {
            let mut reader = hound::WavReader::open(path)
                .map_err(|e| format!("Cannot open WAV file: {e}"))?;
            let spec = reader.spec();
            println!("   WAV: {} Hz, {} ch, {} bits/sample",
                     spec.sample_rate, spec.channels, spec.bits_per_sample);

            // Read all samples as i16
            let samples: Vec<i16> = match spec.sample_format {
                hound::SampleFormat::Int => {
                    reader.samples::<i16>()
                        .filter_map(|s| s.ok())
                        .collect()
                }
                hound::SampleFormat::Float => {
                    // Convert f32 samples to i16
                    reader.samples::<f32>()
                        .filter_map(|s| s.ok())
                        .map(|s| (s * 32768.0).clamp(-32768.0, 32767.0) as i16)
                        .collect()
                }
            };

            // Convert to 16kHz mono if needed
            let target_sr = 16000u32;
            let mono = if spec.channels == 1 {
                samples
            } else {
                // Average stereo to mono
                samples.chunks_exact(spec.channels as usize)
                    .map(|ch| {
                        let sum: i32 = ch.iter().map(|&s| s as i32).sum();
                        (sum / spec.channels as i32) as i16
                    })
                    .collect()
            };

            let pcm: Vec<i16> = if spec.sample_rate == target_sr {
                mono
            } else {
                // Simple linear resample
                let ratio = target_sr as f64 / spec.sample_rate as f64;
                let out_len = (mono.len() as f64 * ratio) as usize;
                (0..out_len).map(|i| {
                    let src = (i as f64) / ratio;
                    let si = src as usize;
                    let f = src - si as f64;
                    if si + 1 < mono.len() {
                        let v = mono[si] as f64 * (1.0 - f) + mono[si + 1] as f64 * f;
                        v.clamp(-32768.0, 32767.0) as i16
                    } else {
                        mono.last().copied().unwrap_or(0)
                    }
                }).collect()
            };

            let mut out = Vec::with_capacity(pcm.len() * 2);
            for &s in &pcm {
                out.extend_from_slice(&s.to_le_bytes());
            }
            println!("   → Resampled: {} Hz mono, {} bytes PCM", target_sr, out.len());
            Ok((out, target_sr, 1))
        }
        _ => {
            // Assume raw PCM: 16-bit, 16kHz, mono
            let data = fs::read(path)
                .map_err(|e| format!("Cannot read file: {e}"))?;
            Ok((data, 16000, 1))
        }
    }
}

async fn run_asr_file_mode(
    path: PathBuf,
    language: Option<String>,
    chunk_ms: u64,
    output: Option<PathBuf>,
    write: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
    read: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
) {
    let (pcm_data, sample_rate, _channels) = match load_audio(&path) {
        Ok(ok) => ok,
        Err(e) => {
            eprintln!("❌ {e}");
            return;
        }
    };

    println!("📂 Loaded {} bytes from {}", pcm_data.len(), path.display());

    // Send ASR start
    let start = ClientMessage::AsrStart { language };
    send_json(write, &start).await;
    println!("   → ASR start sent");

    // Small delay to let server initialize the stream
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Compute chunk size in bytes (2 bytes per sample at 16kHz)
    let bytes_per_ms = (sample_rate as u64 * 2) / 1000;
    let chunk_size = if chunk_ms == 0 {
        pcm_data.len()
    } else {
        (chunk_ms * bytes_per_ms) as usize
    };

    let total_chunks = if chunk_size > 0 {
        (pcm_data.len() + chunk_size - 1) / chunk_size
    } else {
        1
    };

    println!("   Sending audio in {} chunk(s) ({} ms each, {} bytes)...",
             total_chunks, chunk_ms, chunk_size);
    println!();

    let send_start = std::time::Instant::now();

    // Send audio in chunks
    for (i, chunk) in pcm_data.chunks(chunk_size).enumerate() {
        let mut bin_frame = Vec::with_capacity(1 + chunk.len());
        bin_frame.push(ASR_AUDIO_MARKER);
        bin_frame.extend_from_slice(chunk);

        if let Err(e) = write.send(Message::Binary(bin_frame)).await {
            eprintln!("❌ Failed to send audio chunk {}/{}: {}", i + 1, total_chunks, e);
            return;
        }

        // Print progress for large files
        if total_chunks > 1 && (i == 0 || i == total_chunks - 1 || i % 5 == 4) {
            println!("   📤 Chunk {}/{} ({} bytes) sent at +{:?}",
                     i + 1, total_chunks, chunk.len(), send_start.elapsed());
        }

        // Small inter-chunk delay to simulate real-time streaming
        // Use half the chunk duration to avoid overwhelming the server
        if chunk_ms > 0 && i + 1 < total_chunks {
            tokio::time::sleep(std::time::Duration::from_millis(chunk_ms / 2)).await;
        }
    }

    let send_duration = send_start.elapsed();
    println!("   📤 All chunks sent in {:?}", send_duration);
    println!();

    // Send ASR stop to trigger finalization
    send_json(write, &ClientMessage::AsrStop).await;
    println!("   → ASR stop sent");

    // Collect ASR results
    let mut transcript = String::new();
    let timeout = tokio::time::Duration::from_secs(30);
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let wait_start = std::time::Instant::now();
    let mut had_interim = false;

    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let elapsed = wait_start.elapsed();
                        if let Ok(server_msg) = serde_json::from_str::<ServerMessage>(&text) {
                            match server_msg {
                                ServerMessage::AsrFinal { text: t, .. } => {
                                    println!("✅ [+{:?}] [ASR Final] \"{t}\"", elapsed);
                                    transcript = t;
                                }
                                ServerMessage::AsrInterim { text: t, .. } => {
                                    had_interim = true;
                                    if transcript.is_empty() {
                                        println!("🗣️  [+{:?}] [ASR Interim] \"{t}\"", elapsed);
                                    } else {
                                        // Update as last-known-best (show inline)
                                        let prev_len = transcript.len();
                                        transcript = t.clone();
                                        println!("🗣️  [+{:?}] [ASR Interim] \"{t}\" (prev: {}→{} chars)", elapsed, prev_len, t.len());
                                    }
                                }
                                ServerMessage::AsrError { code, message } => {
                                    eprintln!("❌ [+{:?}] [ASR Error] {code}: {message}", elapsed);
                                }
                                ServerMessage::VadState { state } => {
                                    let icon = if state == "speech" { "🗣️" } else { "🔇" };
                                    println!("{icon} [+{:?}] [VAD] {state}", elapsed);
                                }
                                _ => {
                                    print_server_message(&server_msg);
                                }
                            }
                        } else {
                            println!("📩 [+{:?}] [Raw JSON] {text}", elapsed);
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        println!("📦 [Binary] {} bytes", data.len());
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        eprintln!("❌ WebSocket error: {e}");
                        break;
                    }
                    None => break,
                }
            }
            _ = &mut deadline => {
                println!("⏱️  [+{:?}] Timeout reached. No final ASR result.", wait_start.elapsed());
                break;
            }
        }
    }

    // Save transcript
    if !transcript.is_empty() {
        let out_path = output.unwrap_or_else(|| {
            let ts = timestamp_suffix();
            PathBuf::from(format!("asr_{ts}.txt"))
        });
        match fs::write(&out_path, &transcript) {
            Ok(_) => println!("💾 ASR result saved to: {}", out_path.display()),
            Err(e) => eprintln!("⚠️  Failed to save transcript: {e}"),
        }
    } else {
        println!("ℹ️  No ASR result to save.");
        if !had_interim {
            println!("   (No interim results received either — ASR engine may not be producing output)");
        }
    }
}

// ── TTS mode ───────────────────────────────────────────────

async fn run_tts_mode(
    text: String,
    output: Option<PathBuf>,
    write: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
    read: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
) {
    println!("📝 Sending TTS request: \"{text}\"");

    let tts_text = text.clone();
    let tts_req = ClientMessage::TtsRequest {
        text,
        voice: None,
    };
    send_json(write, &tts_req).await;

    println!("   Waiting for TTS audio chunks...");
    println!();

    let timeout = tokio::time::Duration::from_secs(60);
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);

    let mut accumulated: Vec<u8> = Vec::new();
    let mut sample_rate: u32 = 44100; // default
    let mut had_error = false;

    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(server_msg) = serde_json::from_str::<ServerMessage>(&text) {
                            match server_msg {
                                ServerMessage::TtsAudio { data, format: _, sample_rate: sr } => {
                                    sample_rate = sr;
                                    match base64_decode_to_pcm(&data) {
                                        Ok(pcm) => {
                                            let len = pcm.len();
                                            accumulated.extend_from_slice(&pcm);
                                            println!("🎵 [TTS Audio] chunk: {len} bytes PCM (total: {} bytes)", accumulated.len());
                                        }
                                        Err(e) => eprintln!("⚠️  Base64 decode error: {e}"),
                                    }
                                }
                                ServerMessage::TtsEnd { duration_ms } => {
                                    println!("✅ [TTS End] duration={duration_ms}ms");
                                    break;
                                }
                                ServerMessage::TtsError { code, message } => {
                                    eprintln!("❌ [TTS Error] {code}: {message}");
                                    had_error = true;
                                    break;
                                }
                                _ => {
                                    print_server_message(&server_msg);
                                }
                            }
                        } else {
                            println!("📩 [Raw JSON] {text}");
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        if data.len() > 1 && data[0] == 0x01 {
                            accumulated.extend_from_slice(&data[1..]);
                            println!("🎵 [Binary TTS audio] chunk: {} bytes (total: {} bytes)", data.len() - 1, accumulated.len());
                        } else {
                            println!("📦 [Binary frame] {} bytes", data.len());
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        eprintln!("❌ WebSocket error: {e}");
                        had_error = true;
                        break;
                    }
                    None => break,
                }
            }
            _ = &mut deadline => {
                println!("⏱️  Timeout waiting for TTS.");
                break;
            }
        }
    }

    // Save TTS audio to WAV
    if !had_error && !accumulated.is_empty() {
        // Store in global for interactive save command
        if let Ok(mut last) = LAST_TTS_PCM.lock() {
            *last = Some(AccumulatedTts {
                pcm_data: accumulated.clone(),
                sample_rate,
            });
        }

        let out_path = output.unwrap_or_else(|| {
            let ts = timestamp_suffix();
            PathBuf::from(format!("tts_{ts}.wav"))
        });

        match write_wav_file(&out_path, &accumulated, sample_rate) {
            Ok(()) => {
                println!("💾 TTS audio saved to: {}", out_path.display());

                // Also save a .txt sidecar with the input text
                let mut txt_path = out_path.clone();
                txt_path.set_extension("txt");
                let meta = format!(
                    "Input text: {}\nSample rate: {} Hz\nPCM size: {} bytes\n",
                    tts_text,
                    sample_rate,
                    accumulated.len()
                );
                let _ = fs::write(&txt_path, &meta);
            }
            Err(e) => eprintln!("⚠️  Failed to write WAV file: {e}"),
        }
    } else if !had_error {
        println!("ℹ️  No TTS audio received.");
    }
}

// ── Command processing (interactive) ───────────────────────

async fn process_command(
    input: &str,
    write: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
) -> bool {
    let input = input.trim();
    let parts: Vec<&str> = input.splitn(3, ' ').collect();
    let cmd = parts[0].to_lowercase();

    match cmd.as_str() {
        "quit" | "exit" | "q" => {
            println!("Goodbye!");
            return false;
        }

        "help" | "h" | "?" => {
            print_help();
        }

        "save" => {
            let save_path = parts.get(1).map(|s| PathBuf::from(s));
            save_last_result(save_path);
        }

        "asr_start" => {
            let lang = parts.get(1).map(|s| s.to_string());
            let msg = ClientMessage::AsrStart { language: lang };
            send_json(write, &msg).await;
            println!("  → ASR start sent");
        }

        "asr_stop" => {
            send_json(write, &ClientMessage::AsrStop).await;
            println!("  → ASR stop sent");
        }

        "asr_audio" => {
            if let Some(data) = parts.get(1) {
                let msg = ClientMessage::AsrAudio {
                    data: data.to_string(),
                    sample_rate: Some(16000),
                };
                send_json(write, &msg).await;
                println!("  → ASR audio ({} bytes base64) sent", data.len());
            } else {
                println!("⚠️  Usage: asr_audio <base64_data>");
            }
        }

        "asr_audio_file" => {
            if let Some(path_str) = parts.get(1) {
                match fs::read(path_str) {
                    Ok(pcm_data) => {
                        let mut bin_frame = Vec::with_capacity(1 + pcm_data.len());
                        bin_frame.push(ASR_AUDIO_MARKER);
                        bin_frame.extend_from_slice(&pcm_data);

                        match write.send(Message::Binary(bin_frame)).await {
                            Ok(_) => {
                                println!("  → Binary ASR audio frame ({} bytes) sent", pcm_data.len());
                            }
                            Err(e) => {
                                eprintln!("  ❌ Failed to send binary: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  ❌ Cannot read file '{}': {e}", path_str);
                    }
                }
            } else {
                println!("⚠️  Usage: asr_audio_file <path_to_pcm_file>");
            }
        }

        "tts" => {
            let text = parts.get(1).unwrap_or(&"").to_string();
            if text.is_empty() {
                println!("⚠️  Usage: tts <text to synthesize>");
            } else {
                let msg = ClientMessage::TtsRequest {
                    text,
                    voice: parts.get(2).map(|s| s.to_string()),
                };
                send_json(write, &msg).await;
                println!("  → TTS request sent");
            }
        }

        "tts_cancel" => {
            send_json(write, &ClientMessage::TtsCancel).await;
            println!("  → TTS cancel sent");
        }

        "ping" => {
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let msg = ClientMessage::Ping { timestamp: ts };
            send_json(write, &msg).await;
            println!("  → Ping sent (ts={ts})");
        }

        "config" => {
            if let (Some(key), Some(val_str)) = (parts.get(1), parts.get(2)) {
                match serde_json::from_str::<serde_json::Value>(val_str) {
                    Ok(val) => {
                        let msg = ClientMessage::Config {
                            key: key.to_string(),
                            value: val,
                        };
                        send_json(write, &msg).await;
                        println!("  → Config '{key}' = {val_str} sent");
                    }
                    Err(e) => {
                        eprintln!("  ❌ Invalid JSON value: {e}");
                    }
                }
            } else {
                println!("⚠️  Usage: config <key> <json_value>");
            }
        }

        _ => {
            println!("⚠️  Unknown command: '{cmd}'. Type `help` for available commands.");
        }
    }

    true
}

// ── Save last result (interactive mode) ────────────────────

fn save_last_result(save_path: Option<PathBuf>) {
    // Try TTS first
    if let Ok(mut last) = LAST_TTS_PCM.lock() {
        if let Some(tts) = last.take() {
            let out_path = save_path.unwrap_or_else(|| {
                let ts = timestamp_suffix();
                PathBuf::from(format!("tts_{ts}.wav"))
            });
            match write_wav_file(&out_path, &tts.pcm_data, tts.sample_rate) {
                Ok(()) => println!("💾 TTS audio saved to: {}", out_path.display()),
                Err(e) => eprintln!("⚠️  Failed to save TTS audio: {e}"),
            }
            return;
        }
    }

    // Then try ASR
    if let Ok(mut last) = LAST_ASR_TEXT.lock() {
        if let Some(text) = last.take() {
            let out_path = save_path.unwrap_or_else(|| {
                let ts = timestamp_suffix();
                PathBuf::from(format!("asr_{ts}.txt"))
            });
            match fs::write(&out_path, &text) {
                Ok(()) => println!("💾 ASR result saved to: {}", out_path.display()),
                Err(e) => eprintln!("⚠️  Failed to save ASR result: {e}"),
            }
            return;
        }
    }

    println!("ℹ️  No saved TTS audio or ASR result available. Do a TTS or ASR first.");
}

// ── Incoming message handling ─────────────────────────────

async fn handle_incoming(msg: Message) -> Result<(), String> {
    match msg {
        Message::Text(text) => {
            match serde_json::from_str::<ServerMessage>(&text) {
                Ok(server_msg) => {
                    // Store ASR final result for later save
                    if let ServerMessage::AsrFinal { ref text, .. } = server_msg {
                        if let Ok(mut last) = LAST_ASR_TEXT.lock() {
                            *last = Some(text.clone());
                        }
                    }
                    // Store TTS audio for later save
                    if let ServerMessage::TtsAudio { ref data, sample_rate, .. } = server_msg {
                        if let Ok(pcm) = base64_decode_to_pcm(data) {
                            if let Ok(mut last) = LAST_TTS_PCM.lock() {
                                match last.as_mut() {
                                    Some(acc) => {
                                        acc.pcm_data.extend_from_slice(&pcm);
                                        acc.sample_rate = sample_rate;
                                    }
                                    None => {
                                        *last = Some(AccumulatedTts {
                                            pcm_data: pcm,
                                            sample_rate,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    print_server_message(&server_msg);
                }
                Err(e) => {
                    println!("📩 [Raw JSON] {}", text);
                    if !e.to_string().contains("unknown variant") {
                        println!("   (parse error: {e})");
                    }
                }
            }
        }
        Message::Binary(data) => {
            if data.len() >= 5 {
                let marker = data[0];
                let payload_len = data.len() - 1;
                match marker {
                    0x01 => {
                        // Store binary TTS audio for later save
                        if let Ok(mut last) = LAST_TTS_PCM.lock() {
                            match last.as_mut() {
                                Some(acc) => acc.pcm_data.extend_from_slice(&data[1..]),
                                None => {
                                    *last = Some(AccumulatedTts {
                                        pcm_data: data[1..].to_vec(),
                                        sample_rate: 44100,
                                    });
                                }
                            }
                        }
                        println!("🎵 [Binary TTS audio] {} bytes PCM", payload_len);
                    }
                    _ => {
                        println!("📦 [Binary frame] marker=0x{marker:02x}, {} bytes", payload_len);
                    }
                }
            } else {
                println!("📦 [Binary frame] {} bytes", data.len());
            }
        }
        Message::Ping(data) => {
            println!("🔁 [Ping] {:?}", data);
        }
        Message::Pong(data) => {
            println!("🔁 [Pong] {:?}", data);
        }
        Message::Close(frame) => {
            println!("🔌 [Close] {:?}", frame);
        }
        Message::Frame(_) => {}
    }

    Ok(())
}

fn print_server_message(msg: &ServerMessage) {
    match msg {
        ServerMessage::AsrInterim { text, is_final: _ } => {
            println!("🗣️  [ASR Interim] \"{text}\"");
        }
        ServerMessage::AsrFinal {
            text,
            is_final: _,
            confidence,
        } => {
            println!("✅ [ASR Final] \"{text}\" (conf={confidence:.2})");
        }
        ServerMessage::AsrError { code, message } => {
            println!("❌ [ASR Error] {code}: {message}");
        }
        ServerMessage::TtsAudio {
            data,
            format,
            sample_rate,
        } => {
            let pcm_len = (data.len() as f64 * 0.75) as usize;
            println!(
                "🎵 [TTS Audio] {} bytes PCM ({} Hz, {})",
                pcm_len, sample_rate, format
            );
        }
        ServerMessage::TtsEnd { duration_ms } => {
            println!("✅ [TTS End] duration={duration_ms}ms");
        }
        ServerMessage::TtsError { code, message } => {
            println!("❌ [TTS Error] {code}: {message}");
        }
        ServerMessage::VadState { state } => {
            let icon = match state.as_str() {
                "speech" => "🗣️",
                "silence" => "🔇",
                _ => "❓",
            };
            println!("{icon} [VAD] {state}");
        }
        ServerMessage::Error { code, message } => {
            println!("⚠️  [Error] {code}: {message}");
        }
        ServerMessage::Pong { timestamp } => {
            println!("🔁 [Pong] ts={timestamp}");
        }
    }
}

// ── File I/O helpers ───────────────────────────────────────

/// Decode a base64 string to raw PCM i16 bytes.
fn base64_decode_to_pcm(b64: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("Base64 decode error: {e}"))
}

/// Write raw PCM i16 data to a WAV file with proper header.
fn write_wav_file(path: &PathBuf, pcm_data: &[u8], sample_rate: u32) -> Result<(), String> {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|e| format!("Cannot create WAV file: {e}"))?;

    // Convert bytes to i16 samples (little-endian)
    for chunk in pcm_data.chunks_exact(2) {
        let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
        writer
            .write_sample(sample)
            .map_err(|e| format!("Write sample error: {e}"))?;
    }

    writer
        .finalize()
        .map_err(|e| format!("Finalize WAV error: {e}"))?;

    Ok(())
}

/// Generate a timestamp suffix for auto-generated filenames.
fn timestamp_suffix() -> String {
    chrono::Local::now()
        .format("%Y%m%d_%H%M%S")
        .to_string()
}

// ── WebSocket helpers ──────────────────────────────────────

async fn send_json(
    write: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
    msg: &ClientMessage,
) {
    let json = serde_json::to_string(msg).expect("serialize ClientMessage");
    if let Err(e) = write.send(Message::Text(json.into())).await {
        eprintln!("❌ Send error: {e}");
    }
}

fn print_help() {
    println!();
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║              Voice Client 指令說明                   ║");
    println!("╠══════════════════════════════════════════════════════╣");
    println!("║  asr_start [lang]      開始 ASR 辨識                ║");
    println!("║  asr_stop              停止 ASR 辨識                ║");
    println!("║  asr_audio <base64>    傳送 base64 PCM 音訊         ║");
    println!("║  asr_audio_file <path> 傳送二進位 PCM 音訊檔        ║");
    println!("║  tts <text>           請求 TTS 語音合成            ║");
    println!("║  tts_cancel           取消 TTS                     ║");
    println!("║  ping                 傳送心跳                     ║");
    println!("║  config <k> <json>    更新設定                     ║");
    println!("║  save [path]          儲存上次 TTS 音訊/ASR 結果   ║");
    println!("║  help                 顯示說明                     ║");
    println!("║  quit                 離開                         ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();
}
