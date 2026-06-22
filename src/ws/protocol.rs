use serde::{Deserialize, Serialize};

// ── Binary frame markers ──────────────────────────────────

/// Marker byte for ASR audio binary frames (client → server)
pub const ASR_AUDIO_MARKER: u8 = 0x00;
/// Marker byte for TTS audio binary frames (server → client)
pub const TTS_AUDIO_MARKER: u8 = 0x01;

// ── Client → Server Messages ──────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Start an ASR session
    AsrStart {
        /// Optional language hint
        language: Option<String>,
    },
    /// Audio data for ASR (alternative to binary frames)
    AsrAudio {
        /// Base64-encoded PCM data (16 kHz, 16-bit, mono)
        data: String,
        /// Sample rate (optional, default 16000)
        sample_rate: Option<u32>,
    },
    /// Stop the current ASR session
    AsrStop,
    /// Request TTS synthesis
    TtsRequest {
        /// Text to synthesize
        text: String,
        /// Optional voice selection
        voice: Option<String>,
    },
    /// Cancel the current TTS synthesis
    TtsCancel,
    /// Update runtime configuration
    Config {
        key: String,
        value: serde_json::Value,
    },
    /// Heartbeat ping
    Ping {
        timestamp: u64,
    },
}

// ── Server → Client Messages ──────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// ASR interim result
    AsrInterim {
        text: String,
        #[serde(rename = "is_final")]
        is_final: bool,
    },
    /// ASR final result
    AsrFinal {
        text: String,
        #[serde(rename = "is_final")]
        is_final: bool,
        confidence: f32,
    },
    /// ASR error
    AsrError {
        code: String,
        message: String,
    },
    /// TTS audio data (text frame — use base64 when binary not available)
    TtsAudio {
        data: String, // base64 PCM
        format: String,
        sample_rate: u32,
    },
    /// TTS synthesis complete
    TtsEnd {
        duration_ms: u32,
    },
    /// TTS error
    TtsError {
        code: String,
        message: String,
    },
    /// VAD state change
    VadState {
        state: String, // "speech" | "silence"
    },
    /// Generic error
    Error {
        code: String,
        message: String,
    },
    /// Heartbeat pong
    Pong {
        timestamp: u64,
    },
}

impl ServerMessage {
    /// Create an ASR interim result message.
    pub fn interim(text: impl Into<String>) -> Self {
        Self::AsrInterim {
            text: text.into(),
            is_final: false,
        }
    }

    /// Create an ASR final result message.
    pub fn final_result(text: impl Into<String>, confidence: f32) -> Self {
        Self::AsrFinal {
            text: text.into(),
            is_final: true,
            confidence,
        }
    }

    /// Create a VAD speech state message.
    pub fn vad_speech() -> Self {
        Self::VadState {
            state: "speech".into(),
        }
    }

    /// Create a VAD silence state message.
    pub fn vad_silence() -> Self {
        Self::VadState {
            state: "silence".into(),
        }
    }

    /// Create an error message.
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
        }
    }

    /// Create a pong message.
    pub fn pong(timestamp: u64) -> Self {
        Self::Pong { timestamp }
    }
}

// ── Helpers ───────────────────────────────────────────────

/// Parse a binary WebSocket frame.
/// Returns None if the marker byte is unknown.
pub fn parse_binary_frame(data: &[u8]) -> Option<BinaryPayload> {
    if data.is_empty() {
        return None;
    }
    let marker = data[0];
    let payload = &data[1..];
    match marker {
        ASR_AUDIO_MARKER => Some(BinaryPayload::AsrAudio(payload.to_vec())),
        TTS_AUDIO_MARKER => Some(BinaryPayload::TtsAudio(payload.to_vec())),
        _ => None,
    }
}

/// Parsed binary frame payload.
pub enum BinaryPayload {
    /// ASR audio PCM data (16 kHz, 16-bit, mono)
    AsrAudio(Vec<u8>),
    /// TTS audio PCM data
    TtsAudio(Vec<u8>),
}

/// Convert f32 samples to i16 PCM bytes for binary frame transmission.
///
/// Applies peak-based automatic gain control (AGC) to normalize volume:
/// - Finds the peak absolute sample value, then amplifies so the peak
///   reaches a target of 0.95 (near full scale, -0.45 dBFS).
/// - Pure silence (peak == 0) is left unchanged.
/// - Gain is capped at 100× to avoid extreme noise amplification.
/// - Already healthy audio (peak > 0.1) is left mostly unchanged.
pub fn f32_to_i16_pcm(samples: &[f32]) -> Vec<u8> {
    // Find peak absolute value
    let peak = samples
        .iter()
        .map(|s| s.abs())
        .fold(0.0f32, f32::max);

    // Compute gain to bring peak to 0.95 (near full scale)
    let gain = if peak > 0.000_001 {
        (0.95 / peak).min(100.0)
    } else {
        1.0
    };

    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for &sample in samples {
        let amplified = sample * gain;
        let clamped = amplified.clamp(-1.0, 1.0);
        let int_val = (clamped * 32767.0) as i16;
        bytes.extend_from_slice(&int_val.to_le_bytes());
    }
    bytes
}

/// Build a binary frame with marker byte + PCM data.
pub fn build_binary_frame(marker: u8, pcm_data: &[i16]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(1 + pcm_data.len() * 2);
    frame.push(marker);
    for &sample in pcm_data {
        frame.extend_from_slice(&sample.to_le_bytes());
    }
    frame
}
