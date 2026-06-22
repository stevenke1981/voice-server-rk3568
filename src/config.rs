use serde::Deserialize;
use std::path::PathBuf;

/// Server configuration loaded from config.toml
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub asr: AsrConfig,
    #[serde(default)]
    pub tts: TtsConfig,
    #[serde(default)]
    pub vad: VadConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AsrConfig {
    #[serde(default = "default_asr_model_type")]
    pub model_type: String,
    pub encoder: Option<PathBuf>,
    pub decoder: Option<PathBuf>,
    pub joiner: Option<PathBuf>,
    pub tokens: Option<PathBuf>,
    pub model: Option<PathBuf>,
    #[serde(default = "default_num_threads")]
    pub num_threads: i32,
    #[serde(default = "default_provider")]
    pub provider: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TtsConfig {
    #[serde(default = "default_tts_model_type")]
    pub model_type: String,
    pub model: Option<PathBuf>,
    pub tokens: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    pub voice: Option<String>,
    #[serde(default = "default_num_threads")]
    pub num_threads: i32,
    #[serde(default = "default_provider")]
    pub provider: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VadConfig {
    pub model: Option<PathBuf>,
    #[serde(default = "default_vad_threshold")]
    pub threshold: f32,
    #[serde(default = "default_min_speech_duration_ms")]
    pub min_speech_duration_ms: u32,
    #[serde(default = "default_min_silence_duration_ms")]
    pub min_silence_duration_ms: u32,
    #[serde(default = "default_window_size")]
    pub window_size: i32,
}

// ── Defaults ──────────────────────────────────────────────

fn default_host() -> String {
    "0.0.0.0".to_string()
}
fn default_port() -> u16 {
    8080
}
fn default_max_connections() -> usize {
    8
}
fn default_idle_timeout_secs() -> u64 {
    300
}
fn default_asr_model_type() -> String {
    "zipformer".to_string()
}
fn default_tts_model_type() -> String {
    "piper".to_string()
}
fn default_num_threads() -> i32 {
    2
}
fn default_provider() -> String {
    "cpu".to_string()
}
fn default_vad_threshold() -> f32 {
    0.5
}
fn default_min_speech_duration_ms() -> u32 {
    100
}
fn default_min_silence_duration_ms() -> u32 {
    500
}
fn default_window_size() -> i32 {
    512
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            max_connections: default_max_connections(),
            idle_timeout_secs: default_idle_timeout_secs(),
        }
    }
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self {
            model_type: default_asr_model_type(),
            encoder: None,
            decoder: None,
            joiner: None,
            tokens: None,
            model: None,
            num_threads: default_num_threads(),
            provider: default_provider(),
        }
    }
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            model_type: default_tts_model_type(),
            model: None,
            tokens: None,
            data_dir: None,
            voice: None,
            num_threads: default_num_threads(),
            provider: default_provider(),
        }
    }
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            model: None,
            threshold: default_vad_threshold(),
            min_speech_duration_ms: default_min_speech_duration_ms(),
            min_silence_duration_ms: default_min_silence_duration_ms(),
            window_size: default_window_size(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            asr: AsrConfig::default(),
            tts: TtsConfig::default(),
            vad: VadConfig::default(),
        }
    }
}

impl Config {
    /// Load config from a TOML file path.
    /// Falls back to defaults if file doesn't exist or parsing fails.
    pub fn from_file(path: &str) -> Self {
        let content = std::fs::read_to_string(path);
        match content {
            Ok(text) => match toml::from_str(&text) {
                Ok(cfg) => cfg,
                Err(e) => {
                    tracing::warn!("Failed to parse config file '{}': {}. Using defaults.", path, e);
                    Config::default()
                }
            },
            Err(e) => {
                tracing::info!("No config file '{}' ({}). Using defaults.", path, e);
                Config::default()
            }
        }
    }
}
