use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use qvs_core::QvodError;

#[derive(Debug, Clone)]
pub struct MediaInfo {
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
    pub video_codec: String,
    pub audio_codec: String,
    pub bitrate: u64,
    pub fps: f64,
}

impl Default for MediaInfo {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            duration_ms: 0,
            video_codec: String::new(),
            audio_codec: String::new(),
            bitrate: 0,
            fps: 0.0,
        }
    }
}

pub fn probe_file(path: &std::path::Path) -> Result<MediaInfo, QvodError> {
    use ffmpeg_next::media::Type as MediaType;

    ffmpeg_next::init().map_err(|e| QvodError::Decode(format!("ffmpeg init failed: {e}")))?;

    let ictx = ffmpeg_next::format::input(path)
        .map_err(|e| QvodError::Decode(format!("ffmpeg open failed: {e}")))?;

    let mut info = MediaInfo {
        duration_ms: (ictx.duration() / 1000) as u64,
        ..Default::default()
    };

    for stream in ictx.streams() {
        let params = stream.parameters();
        match params.medium() {
            MediaType::Video => {
                let codec_params = params.clone();
                if let Ok(ctx) = ffmpeg_next::codec::context::Context::from_parameters(codec_params)
                {
                    if let Ok(decoder) = ctx.decoder().video() {
                        info.width = decoder.width();
                        info.height = decoder.height();
                        info.video_codec = format!("{:?}", params.id()).to_lowercase();
                    }
                }
                let fps = stream.avg_frame_rate();
                info.fps = f64::from(fps.numerator()) / f64::from(fps.denominator());
            }
            MediaType::Audio => {
                if info.audio_codec.is_empty() {
                    info.audio_codec = format!("{:?}", params.id()).to_lowercase();
                }
            }
            _ => {}
        }
    }

    Ok(info)
}

#[allow(unsafe_code)]
pub struct NativeFrameReader {
    frame_rx: mpsc::Receiver<Vec<u8>>,
    width: u32,
    height: u32,
    frame_size: usize,
    fps: f64,
    duration_ms: u64,
    position_ms: u64,
    running: Arc<AtomicBool>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

#[allow(unsafe_code)]
impl NativeFrameReader {
    pub fn open(path: &str) -> Result<Self, QvodError> {
        let info = probe_file(std::path::Path::new(path))?;

        if info.width == 0 || info.height == 0 {
            return Err(QvodError::Decode(
                "could not determine video dimensions".into(),
            ));
        }

        let width = info.width;
        let height = info.height;
        let frame_size = (width * height * 3) as usize;
        let fps = if info.fps > 0.0 { info.fps } else { 24.0 };
        let duration_ms = info.duration_ms;

        let (frame_tx, frame_rx) = mpsc::channel::<Vec<u8>>();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        let path_owned = path.to_string();

        let thread_handle = std::thread::Builder::new()
            .name("ffmpeg-next-decoder".into())
            .spawn(move || {
                Self::decoder_thread(&path_owned, frame_size, frame_tx, running_clone);
            })
            .map_err(|e| QvodError::Decode(format!("decoder thread spawn: {e}")))?;

        Ok(Self {
            frame_rx,
            width,
            height,
            frame_size,
            fps,
            duration_ms,
            position_ms: 0,
            running,
            thread_handle: Some(thread_handle),
        })
    }

    fn decoder_thread(
        path: &str,
        _frame_size: usize,
        tx: mpsc::Sender<Vec<u8>>,
        running: Arc<AtomicBool>,
    ) {
        let result = Self::run_decoder(path, tx, running);
        if let Err(e) = result {
            tracing::error!("native decoder failed: {e}");
        }
    }

    fn run_decoder(
        path: &str,
        tx: mpsc::Sender<Vec<u8>>,
        running: Arc<AtomicBool>,
    ) -> Result<(), QvodError> {
        use ffmpeg_next::media::Type as MediaType;

        ffmpeg_next::init().map_err(|e| QvodError::Decode(format!("ffmpeg init: {e}")))?;

        let mut ictx = ffmpeg_next::format::input(path)
            .map_err(|e| QvodError::Decode(format!("open input: {e}")))?;

        let input_stream = ictx
            .streams()
            .best(MediaType::Video)
            .ok_or_else(|| QvodError::Decode("no video stream found".into()))?;

        let stream_index = input_stream.index();

        let decoder_params = input_stream.parameters().clone();
        let decoder_ctx = ffmpeg_next::codec::context::Context::from_parameters(decoder_params)
            .map_err(|e| QvodError::Decode(format!("create decoder context: {e}")))?;

        let mut decoder = decoder_ctx
            .decoder()
            .video()
            .map_err(|e| QvodError::Decode(format!("open decoder: {e}")))?;

        let codec_width = decoder.width();
        let codec_height = decoder.height();
        let codec_format = decoder.format();

        let format_rgb = ffmpeg_next::format::pixel::Pixel::RGB24;

        let mut scaler = ffmpeg_next::software::scaling::Context::get(
            codec_format,
            codec_width,
            codec_height,
            format_rgb,
            codec_width,
            codec_height,
            ffmpeg_next::software::scaling::Flags::BILINEAR,
        )
        .map_err(|e| QvodError::Decode(format!("create scaler: {e}")))?;

        let mut rgb_frame = ffmpeg_next::frame::Video::empty();
        unsafe {
            rgb_frame.alloc(format_rgb, codec_width, codec_height);
        }

        let mut frame = ffmpeg_next::frame::Video::empty();

        for (stream, packet) in ictx.packets() {
            if !running.load(Ordering::Relaxed) {
                return Ok(());
            }

            if stream.index() == stream_index {
                let _ = decoder.send_packet(&packet);

                while let Ok(()) = decoder.receive_frame(&mut frame) {
                    if let Err(e) = scaler.run(&frame, &mut rgb_frame) {
                        tracing::warn!("scaler run failed: {e}");
                        continue;
                    }
                    if tx.send(rgb_frame.data(0).to_vec()).is_err() {
                        return Ok(());
                    }
                }
            }
        }

        let _ = decoder.send_eof();
        while let Ok(()) = decoder.receive_frame(&mut frame) {
            if let Err(e) = scaler.run(&frame, &mut rgb_frame) {
                tracing::warn!("scaler run (flush): {e}");
                break;
            }
            if tx.send(rgb_frame.data(0).to_vec()).is_err() {
                break;
            }
        }

        Ok(())
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub fn fps(&self) -> f64 {
        self.fps
    }

    #[must_use]
    pub fn duration_ms(&self) -> u64 {
        self.duration_ms
    }

    #[must_use]
    pub fn position_ms(&self) -> u64 {
        self.position_ms
    }

    pub fn try_read_frame(&mut self) -> Result<Option<Vec<u8>>, QvodError> {
        if !self.is_running() && self.frame_rx.try_recv().is_err() {
            return Err(QvodError::Decode("decoder stopped".into()));
        }

        match self.frame_rx.try_recv() {
            Ok(frame) => {
                if self.fps > 0.0 {
                    let frame_duration_ms = (1000.0 / self.fps) as u64;
                    self.position_ms += frame_duration_ms;
                }
                Ok(Some(frame))
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(QvodError::Decode("decoder disconnected".into()))
            }
        }
    }

    pub fn read_frame(&mut self) -> Result<Option<Vec<u8>>, QvodError> {
        match self.frame_rx.recv() {
            Ok(frame) => {
                if self.fps > 0.0 {
                    let frame_duration_ms = (1000.0 / self.fps) as u64;
                    self.position_ms += frame_duration_ms;
                }
                Ok(Some(frame))
            }
            Err(mpsc::RecvError) => Ok(None),
        }
    }

    pub fn close(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }

    #[must_use]
    pub fn is_running(&mut self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

impl Drop for NativeFrameReader {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn extract_frame(path: &str, _timestamp_ms: u64) -> Result<Option<Vec<u8>>, QvodError> {
    let mut reader = NativeFrameReader::open(path)?;
    reader.read_frame()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probe_nonexistent_file() {
        let result = probe_file(std::path::Path::new("/nonexistent/video.mp4"));
        assert!(result.is_err());
    }

    #[test]
    fn test_media_info_default() {
        let info = MediaInfo::default();
        assert_eq!(info.width, 0);
        assert_eq!(info.height, 0);
        assert_eq!(info.duration_ms, 0);
    }

    #[test]
    fn test_extract_frame_nonexistent() {
        let result = extract_frame("/nonexistent/video.mp4", 0);
        assert!(result.is_err());
    }
}
