use crate::config::TtsConfig;
use crate::error::VoiceServerError;
use sherpa_onnx::{
    GenerationConfig, OfflineTts, OfflineTtsConfig,
};

/// TTS synthesis result chunk.
#[derive(Debug, Clone)]
pub struct TtsChunk {
    /// PCM audio data (f32 samples normalized to [-1, 1])
    pub audio: Vec<f32>,
    /// Sample rate of the audio in Hz
    pub sample_rate: u32,
    /// Duration in milliseconds
    pub duration_ms: u32,
}

/// Wrapper around sherpa-onnx OfflineTts.
///
/// OfflineTts is the offline (non-streaming) TTS engine that
/// synthesizes the full audio at once. We then chunk the audio
/// for streaming delivery to the WebSocket client.
pub struct TtsEngine {
    tts: OfflineTts,
}

impl TtsEngine {
    /// Create a new TTS engine.
    pub fn new(config: &TtsConfig) -> Result<Self, VoiceServerError> {
        let tts_config = build_tts_config(config);
        let tts = OfflineTts::create(&tts_config)
            .ok_or_else(|| {
                VoiceServerError::Tts("Failed to create OfflineTts: returned None".into())
            })?;

        Ok(Self { tts })
    }

    /// Synthesize text to audio.
    /// Returns the full audio as f32 samples.
    pub fn synthesize(
        &self,
        text: &str,
        sid: Option<i32>,
    ) -> Result<TtsChunk, VoiceServerError> {
        let generation_config = GenerationConfig {
            sid: sid.unwrap_or(0),
            ..Default::default()
        };

        let audio = self
            .tts
            .generate_with_config(text, &generation_config, None::<fn(&[f32], f32) -> bool>)
            .ok_or_else(|| VoiceServerError::Tts("TTS synthesis returned None".into()))?;

        let sample_rate = audio.sample_rate() as u32;
        let samples = audio.samples().to_vec();
        let duration_ms = if sample_rate > 0 {
            (samples.len() as u64 * 1000 / sample_rate as u64) as u32
        } else {
            0
        };

        Ok(TtsChunk {
            audio: samples,
            sample_rate,
            duration_ms,
        })
    }

    /// Get the output sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.tts.sample_rate() as u32
    }
}

/// Build sherpa-onnx OfflineTtsConfig from our application config.
fn build_tts_config(config: &TtsConfig) -> OfflineTtsConfig {
    let mut tts_config = OfflineTtsConfig::default();

    match config.model_type.as_str() {
        "vits" => {
            if let Some(model) = &config.model {
                tts_config.model.vits.model =
                    Some(model.to_string_lossy().to_string());
            }
            if let Some(tokens) = &config.tokens {
                tts_config.model.vits.tokens =
                    Some(tokens.to_string_lossy().to_string());
            }
            if let Some(lexicon) = &config.lexicon {
                tts_config.model.vits.lexicon =
                    Some(lexicon.to_string_lossy().to_string());
            }
            // dict_dir: directory containing dict/ subfolder with Jieba data.
            // Required for Melo TTS zh-en (Jieba word segmentation + FST text normalization).
            // Default: use the parent directory of model.onnx
            // Note: vits.data_dir is NOT set here — it is only for espeak-ng-based
            // piper models and would require phontab/phonindex/phondata files.
            let data_dir = config.data_dir.as_deref().or_else(|| {
                config.model.as_ref().and_then(|m| m.parent())
            });
            if let Some(d) = data_dir {
                let d_str = d.to_string_lossy().to_string();
                let dict_dir = format!("{}/dict", d_str.trim_end_matches(|c| c == '/' || c == '\\'));
                tts_config.model.vits.dict_dir = Some(dict_dir);
            }
        }
        "matcha" | "matcha-tts" => {
            if let Some(model) = &config.model {
                // Matcha uses acoustic_model, not model
                tts_config.model.matcha.acoustic_model =
                    Some(model.to_string_lossy().to_string());
            }
            if let Some(tokens) = &config.tokens {
                tts_config.model.matcha.tokens =
                    Some(tokens.to_string_lossy().to_string());
            }
        }
        "kokoro" => {
            if let Some(model) = &config.model {
                tts_config.model.kokoro.model =
                    Some(model.to_string_lossy().to_string());
            }
            if let Some(tokens) = &config.tokens {
                tts_config.model.kokoro.tokens =
                    Some(tokens.to_string_lossy().to_string());
            }
            if let Some(data_dir) = &config.data_dir {
                tts_config.model.kokoro.data_dir =
                    Some(data_dir.to_string_lossy().to_string());
            }
            if let Some(voice) = &config.voice {
                tts_config.model.kokoro.voices =
                    Some(voice.clone());
            }
        }
        "zipvoice" => {
            if let Some(tokens) = &config.tokens {
                tts_config.model.zipvoice.tokens =
                    Some(tokens.to_string_lossy().to_string());
            }
        }
        "pocket" | "pocket-tts" => {
            // Pocket TTS uses specific model files
            // For simplicity, if model is set, assume it's the encoder
            if let Some(model) = &config.model {
                tts_config.model.pocket.encoder =
                    Some(model.to_string_lossy().to_string());
            }
        }
        _ => {
            // Default: treat as vits
            if let Some(model) = &config.model {
                tts_config.model.vits.model =
                    Some(model.to_string_lossy().to_string());
            }
            if let Some(tokens) = &config.tokens {
                tts_config.model.vits.tokens =
                    Some(tokens.to_string_lossy().to_string());
            }
        }
    }

    tts_config.model.num_threads = config.num_threads;
    tts_config.model.provider = Some(config.provider.clone());

    tts_config
}
