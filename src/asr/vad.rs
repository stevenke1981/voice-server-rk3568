use crate::config::VadConfig;
use crate::error::VoiceServerError;
use sherpa_onnx::{
    SileroVadModelConfig, VadModelConfig, VoiceActivityDetector,
};

/// VAD state reported to the client.
#[derive(Debug, Clone, PartialEq)]
pub enum VadState {
    Speech,
    Silence,
}

/// Wrapper around sherpa-onnx Silero VAD.
pub struct VadEngine {
    detector: VoiceActivityDetector,
}

impl VadEngine {
    /// Create a new VAD engine.
    pub fn new(config: &VadConfig) -> Result<Self, VoiceServerError> {
        let model_path = config
            .model
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .ok_or_else(|| VoiceServerError::Vad("VAD model path not configured".into()))?;

        let vad_config = VadModelConfig {
            silero_vad: SileroVadModelConfig {
                model: Some(model_path),
                threshold: config.threshold,
                min_silence_duration: config.min_silence_duration_ms as f32 / 1000.0,
                min_speech_duration: config.min_speech_duration_ms as f32 / 1000.0,
                window_size: config.window_size,
                max_speech_duration: f32::MAX,
            },
            sample_rate: 16000,
            num_threads: 1,
            provider: Some("cpu".into()),
            debug: false,
            ..Default::default()
        };

        // Buffer size of 30 seconds should be plenty for any utterance
        let detector = VoiceActivityDetector::create(&vad_config, 30.0)
            .ok_or_else(|| VoiceServerError::Vad("Failed to create VAD: returned None".into()))?;

        Ok(Self { detector })
    }

    /// Process a chunk of PCM samples (16-bit, mono).
    /// Converts i16 samples to f32 (normalized to [-1, 1]) and feeds to VAD.
    /// Returns the current VAD state.
    pub fn process(&mut self, samples: &[i16]) -> VadState {
        let float_samples: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
        self.detector.accept_waveform(&float_samples);

        if self.detector.detected() {
            VadState::Speech
        } else {
            VadState::Silence
        }
    }

    /// Reset the VAD state.
    pub fn reset(&mut self) {
        self.detector.reset();
    }

    /// Check if the current state is speech.
    pub fn is_speech(&self) -> bool {
        self.detector.detected()
    }

    /// Check if the detector has queued speech segments.
    pub fn has_segments(&self) -> bool {
        !self.detector.is_empty()
    }

    /// Flush any buffered audio through the detector.
    pub fn flush(&self) {
        self.detector.flush();
    }
}
