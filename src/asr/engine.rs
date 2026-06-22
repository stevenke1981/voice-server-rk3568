use crate::config::AsrConfig;
use crate::error::VoiceServerError;
use sherpa_onnx::{
    OnlineRecognizer, OnlineRecognizerConfig, OnlineStream, RecognizerResult,
};

/// Thread-safe wrapper around sherpa-onnx OnlineRecognizer.
///
/// OnlineRecognizer is Send + Sync (sherpa-onnx declares them),
/// but per-connection OnlineStream usage still requires care.
/// Each WebSocket connection gets its own OnlineStream created
/// from this engine.
pub struct AsrEngine {
    recognizer: OnlineRecognizer,
    sample_rate: i32,
}

impl AsrEngine {
    /// Create a new ASR engine from the given configuration.
    pub fn new(config: &AsrConfig) -> Result<Self, VoiceServerError> {
        let recognizer_config = build_recognizer_config(config);

        let recognizer = OnlineRecognizer::create(&recognizer_config)
            .ok_or_else(|| {
                VoiceServerError::Asr("Failed to create OnlineRecognizer: returned None".into())
            })?;

        Ok(Self {
            recognizer,
            sample_rate: 16000,
        })
    }

    /// Create a new recognition stream for one WebSocket connection.
    pub fn create_stream(&self) -> OnlineStream {
        self.recognizer.create_stream()
    }

    /// Accept PCM samples (16-bit, mono) and run recognition.
    /// Converts i16 samples to f32 internally for sherpa-onnx.
    /// Returns the current RecognizerResult if available.
    pub fn recognize(
        &self,
        stream: &OnlineStream,
        samples: &[i16],
    ) -> Option<RecognizerResult> {
        // Convert i16 to f32
        let float_samples: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
        stream.accept_waveform(self.sample_rate, &float_samples);

        if self.recognizer.is_ready(stream) {
            self.recognizer.decode(stream);
        }

        self.recognizer.get_result(stream)
    }

    /// Decode the current stream state without new input.
    pub fn decode(&self, stream: &OnlineStream) {
        if self.recognizer.is_ready(stream) {
            self.recognizer.decode(stream);
        }
    }

    /// Get current result from a stream without accepting new audio.
    pub fn get_result(&self, stream: &OnlineStream) -> Option<RecognizerResult> {
        self.recognizer.get_result(stream)
    }

    /// Finalize and get the final result.
    /// Marks input as finished, decodes remaining audio, and returns result.
    pub fn finalize_result(&self, stream: &OnlineStream) -> Option<RecognizerResult> {
        stream.input_finished();
        while self.recognizer.is_ready(stream) {
            self.recognizer.decode(stream);
        }
        self.recognizer.get_result(stream)
    }

    /// Check if a stream has endpoint detected.
    pub fn is_endpoint(&self, stream: &OnlineStream) -> bool {
        self.recognizer.is_endpoint(stream)
    }

    /// Reset a stream after endpoint detection.
    pub fn reset(&self, stream: &OnlineStream) {
        self.recognizer.reset(stream);
    }
}

/// Build the sherpa-onnx OnlineRecognizerConfig from our application config.
fn build_recognizer_config(config: &AsrConfig) -> OnlineRecognizerConfig {
    let mut recognizer_config = OnlineRecognizerConfig::default();

    // Feature config
    recognizer_config.feat_config.sample_rate = 16000;
    recognizer_config.feat_config.feature_dim = 80;

    // Model config
    match config.model_type.as_str() {
        "zipformer" | "zipformer2" | "transducer" => {
            if let Some(encoder) = &config.encoder {
                recognizer_config.model_config.transducer.encoder =
                    Some(encoder.to_string_lossy().to_string());
            }
            if let Some(decoder) = &config.decoder {
                recognizer_config.model_config.transducer.decoder =
                    Some(decoder.to_string_lossy().to_string());
            }
            if let Some(joiner) = &config.joiner {
                recognizer_config.model_config.transducer.joiner =
                    Some(joiner.to_string_lossy().to_string());
            }
        }
        "paraformer" => {
            if let Some(encoder) = &config.encoder {
                recognizer_config.model_config.paraformer.encoder =
                    Some(encoder.to_string_lossy().to_string());
            }
            if let Some(decoder) = &config.decoder {
                recognizer_config.model_config.paraformer.decoder =
                    Some(decoder.to_string_lossy().to_string());
            }
        }
        "sensevoice" | "sense_voice" => {
            // SenseVoice uses zipformer2_ctc internally
            if let Some(model) = &config.model {
                recognizer_config.model_config.zipformer2_ctc.model =
                    Some(model.to_string_lossy().to_string());
            }
        }
        "nemo_ctc" => {
            if let Some(model) = &config.model {
                recognizer_config.model_config.nemo_ctc.model =
                    Some(model.to_string_lossy().to_string());
            }
        }
        _ => {
            // Default: treat as zipformer/transducer
            if let Some(encoder) = &config.encoder {
                recognizer_config.model_config.transducer.encoder =
                    Some(encoder.to_string_lossy().to_string());
            }
            if let Some(decoder) = &config.decoder {
                recognizer_config.model_config.transducer.decoder =
                    Some(decoder.to_string_lossy().to_string());
            }
            if let Some(joiner) = &config.joiner {
                recognizer_config.model_config.transducer.joiner =
                    Some(joiner.to_string_lossy().to_string());
            }
        }
    }

    if let Some(tokens) = &config.tokens {
        recognizer_config.model_config.tokens =
            Some(tokens.to_string_lossy().to_string());
    }

    recognizer_config.model_config.num_threads = config.num_threads;
    recognizer_config.model_config.provider = Some(config.provider.clone());

    // Endpoint detection
    recognizer_config.enable_endpoint = true;
    recognizer_config.decoding_method = Some("greedy_search".into());

    recognizer_config
}
