use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

/// Unified error type for the voice server.
#[derive(Debug, Error)]
pub enum VoiceServerError {
    #[error("ASR error: {0}")]
    Asr(String),

    #[error("TTS error: {0}")]
    Tts(String),

    #[error("VAD error: {0}")]
    Vad(String),

    #[error("WebSocket error: {0}")]
    WebSocket(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Rate limited: {0}")]
    RateLimited(String),
}

impl VoiceServerError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Asr(_) => "ASR_ERROR",
            Self::Tts(_) => "TTS_ERROR",
            Self::Vad(_) => "VAD_ERROR",
            Self::WebSocket(_) => "WS_ERROR",
            Self::Config(_) => "CONFIG_ERROR",
            Self::Internal(_) => "INTERNAL_ERROR",
            Self::NotFound(_) => "NOT_FOUND",
            Self::RateLimited(_) => "RATE_LIMITED",
        }
    }
}

impl IntoResponse for VoiceServerError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
            Self::Config(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let body = serde_json::json!({
            "error": self.code(),
            "message": self.to_string(),
        });

        (status, axum::Json(body)).into_response()
    }
}

impl From<anyhow::Error> for VoiceServerError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

impl From<axum::Error> for VoiceServerError {
    fn from(e: axum::Error) -> Self {
        Self::WebSocket(e.to_string())
    }
}

impl From<std::io::Error> for VoiceServerError {
    fn from(e: std::io::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

impl From<serde_json::Error> for VoiceServerError {
    fn from(e: serde_json::Error) -> Self {
        Self::WebSocket(e.to_string())
    }
}

impl From<toml::de::Error> for VoiceServerError {
    fn from(e: toml::de::Error) -> Self {
        Self::Config(e.to_string())
    }
}
