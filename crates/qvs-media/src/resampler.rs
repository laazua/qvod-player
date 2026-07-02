use qvs_core::QvodError;

pub struct AudioResampler;

impl AudioResampler {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn resample(
        &self,
        _input: &[f32],
        _target_sample_rate: u32,
        _target_channels: u16,
    ) -> Result<Vec<f32>, QvodError> {
        Err(QvodError::UnsupportedFormat(
            "audio resampler not available".into(),
        ))
    }
}

impl Default for AudioResampler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resampler_creation() {
        let _resampler = AudioResampler::new();
    }

    #[test]
    fn test_resample_not_available() {
        let resampler = AudioResampler::new();
        let result = resampler.resample(&[0.0f32; 100], 44100, 2);
        assert!(result.is_err());
    }
}
