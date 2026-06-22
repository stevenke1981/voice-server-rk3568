//! Voice Server WebSocket 客戶端
//!
//! 連接到 voice-server 的 WebSocket 端點 (`ws://192.168.80.213:8081/ws`)，
//! 支援 ASR、TTS、VAD 等雙向通訊。
//!
//! # 使用方式
//!
//! ```bash
//! # 互動模式 (stdin 指令)
//! cargo run --bin client
//!
//! # 指定伺服器位址
//! cargo run --bin client -- --url ws://192.168.80.213:8081/ws
//!
//! # 一次性傳送 ASR 音訊檔案後等待結果
//! cargo run --bin client -- asr-file ./audio.pcm
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
//! | `help`                        | 顯示說明                      |
//! | `quit`                        | 離開                          |

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use std::marker::Unpin;
use serde::{Deserialize, Serialize};
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
#[allow(dead_code)] // fields used only for deserialization match
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

// ── CLI arguments ──────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "voice-client", version, about = "Voice Server WebSocket Client")]
struct Args {
    /// WebSocket server URL
    #[arg(short, long, default_value = "ws://192.168.80.213:8081/ws")]
    url: String,

    /// Send a PCM audio file for ASR then wait for result (non-interactive)
    #[arg(long)]
    asr_file: Option<PathBuf>,

    /// Language hint for ASR
    #[arg(long)]
    language: Option<String>,

    /// Text for TTS synthesis (non-interactive, implies --tts-and-exit)
    #[arg(long)]
    tts: Option<String>,
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
        run_asr_file_mode(asr_path, args.language, &mut write, &mut read).await;
        return;
    }

    if let Some(tts_text) = args.tts {
        run_tts_mode(tts_text, &mut write, &mut read).await;
        return;
    }

    // ── Interactive mode ───────────────────────────────

    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();

    println!("📝 Interactive mode. Type `help` for commands, `quit` to exit.");
    println!();

    loop {
        tokio::select! {
            // Receive from server
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

            // Read from stdin
            line = lines.next_line() => {
                match line {
                    Ok(Some(input)) => {
                        let input = input.trim().to_string();
                        if input.is_empty() {
                            continue;
                        }
                        if !process_command(&input, &mut write).await {
                            // quit requested
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

async fn run_asr_file_mode(
    path: PathBuf,
    language: Option<String>,
    write: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
    read: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
) {
    // Read PCM file
    let pcm_data = match fs::read(&path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("❌ Failed to read audio file: {e}");
            return;
        }
    };

    println!("📂 Loaded {} bytes from {}", pcm_data.len(), path.display());
    println!("   Sending ASR start...");

    // Send ASR start
    let start = ClientMessage::AsrStart { language };
    send_json(write, &start).await;

    // Send audio as binary frame
    let mut bin_frame = Vec::with_capacity(1 + pcm_data.len());
    bin_frame.push(ASR_AUDIO_MARKER);
    bin_frame.extend_from_slice(&pcm_data);

    if let Err(e) = write.send(Message::Binary(bin_frame)).await {
        eprintln!("❌ Failed to send audio: {e}");
        return;
    }
    println!("   Audio sent. Waiting for results...");
    println!();

    // Send ASR stop
    send_json(write, &ClientMessage::AsrStop).await;

    // Receive and print results until connection closes or timeout
    let timeout = tokio::time::Duration::from_secs(30);
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(msg)) => {
                        if let Err(e) = handle_incoming(msg).await {
                            eprintln!("⚠️  Error: {e}");
                        }
                    }
                    Some(Err(e)) => {
                        eprintln!("❌ WebSocket error: {e}");
                        break;
                    }
                    None => break,
                }
            }
            _ = &mut deadline => {
                println!("⏱️  Timeout reached (no more messages).");
                break;
            }
        }
    }
}

// ── TTS mode ───────────────────────────────────────────────

async fn run_tts_mode(
    text: String,
    write: &mut (impl SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin),
    read: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
) {
    println!("📝 Sending TTS request: \"{text}\"");

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

    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(msg)) => {
                        if let Err(e) = handle_incoming(msg).await {
                            eprintln!("⚠️  Error: {e}");
                        }
                    }
                    Some(Err(e)) => {
                        eprintln!("❌ WebSocket error: {e}");
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
}

// ── Command processing (interactive) ───────────────────────

/// Process a user command. Returns `false` if the user wants to quit.
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
                                println!(
                                    "  → Binary ASR audio frame ({} bytes) sent",
                                    pcm_data.len()
                                );
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

// ── Incoming message handling ─────────────────────────────

async fn handle_incoming(msg: Message) -> Result<(), String> {
    match msg {
        Message::Text(text) => {
            match serde_json::from_str::<ServerMessage>(&text) {
                Ok(server_msg) => {
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
                        println!(
                            "🎵 [Binary TTS audio] {} bytes PCM",
                            payload_len
                        );
                    }
                    _ => {
                        println!(
                            "📦 [Binary frame] marker=0x{marker:02x}, {} bytes",
                            payload_len
                        );
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
            let pcm_len = (data.len() as f64 * 0.75) as usize; // rough base64 decode size
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

// ── Helper functions ───────────────────────────────────────

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
    println!("║  help                 顯示說明                     ║");
    println!("║  quit                 離開                         ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();
}
