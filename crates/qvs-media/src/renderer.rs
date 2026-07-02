use qvs_core::QvodError;

pub struct VideoRenderer;

impl VideoRenderer {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn render_frame(
        &mut self,
        _frame: &[u8],
        _width: u32,
        _height: u32,
    ) -> Result<(), QvodError> {
        Ok(())
    }

    pub fn clear(&mut self) {}
}

impl Default for VideoRenderer {
    fn default() -> Self {
        Self::new()
    }
}

pub struct AudioRenderer;

impl AudioRenderer {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn play_audio(&mut self, _samples: &[f32]) -> Result<(), QvodError> {
        Ok(())
    }

    pub fn stop(&mut self) {}
}

impl Default for AudioRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_video_renderer_creation() {
        let _renderer = VideoRenderer::new();
    }

    #[test]
    fn test_video_render_frame() {
        let mut renderer = VideoRenderer::new();
        let result = renderer.render_frame(&[0u8; 100], 1920, 1080);
        assert!(result.is_ok());
    }

    #[test]
    fn test_audio_renderer_creation() {
        let _renderer = AudioRenderer::new();
    }

    #[test]
    fn test_audio_play() {
        let mut renderer = AudioRenderer::new();
        let result = renderer.play_audio(&[0.0f32; 100]);
        assert!(result.is_ok());
    }
}
