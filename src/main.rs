mod asr;
mod config;
mod error;
mod tts;
mod ws;

use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::asr::engine::AsrEngine;
use crate::asr::vad::VadEngine;
use crate::config::Config;
use crate::error::VoiceServerError;
use crate::tts::engine::TtsEngine;
use crate::ws::handler::{health_handler, ws_handler, AppState};

#[tokio::main]
async fn main() -> Result<(), VoiceServerError> {
    // Initialize tracing/logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "voice_server=info,tower_http=info".into()),
        )
        .init();

    // Load configuration
    let config_path = std::env::var("VOICE_SERVER_CONFIG")
        .unwrap_or_else(|_| "config.toml".to_string());
    let config = Config::from_file(&config_path);
    info!("Config loaded: {:?} on port {}", config.server.host, config.server.port);

    // Initialize engines
    let asr_engine = AsrEngine::new(&config.asr).map_err(|e| {
        VoiceServerError::Config(format!("Failed to initialize ASR engine: {}", e))
    })?;

    let tts_engine = TtsEngine::new(&config.tts).map_err(|e| {
        VoiceServerError::Config(format!("Failed to initialize TTS engine: {}", e))
    })?;

    let vad_engine = VadEngine::new(&config.vad).map_err(|e| {
        VoiceServerError::Config(format!("Failed to initialize VAD engine: {}", e))
    })?;

    info!("All engines initialized successfully");

    // Build shared state
    let state = Arc::new(AppState {
        config: config.clone(),
        asr_engine: Arc::new(Mutex::new(asr_engine)),
        tts_engine: Arc::new(Mutex::new(tts_engine)),
        vad_engine: Arc::new(Mutex::new(vad_engine)),
        active_connections: Arc::new(AtomicUsize::new(0)),
        max_connections: config.server.max_connections,
    });

    // Build router
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(health_handler))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    // Bind and serve
    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
        .parse()
        .map_err(|e| VoiceServerError::Config(format!("Invalid address: {}", e)))?;

    info!("Voice server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await
        .map_err(|e| VoiceServerError::Internal(format!("Failed to bind: {}", e)))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| VoiceServerError::Internal(format!("Server error: {}", e)))?;

    Ok(())
}
