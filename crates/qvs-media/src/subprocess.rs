use std::io::Read;
use std::process::{Child, Command, Stdio};
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

/// Probe a media file using ffprobe and return metadata.
pub fn probe_file(path: &std::path::Path) -> Result<MediaInfo, QvodError> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path.as_os_str())
        .output()
        .map_err(|e| QvodError::Decode(format!("ffprobe not found: {e}")))?;

    if !output.status.success() {
        return Err(QvodError::Decode("ffprobe failed to analyze file".into()));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| QvodError::Decode(format!("ffprobe json parse: {e}")))?;

    let duration_ms = json["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .map_or(0, |secs| (secs * 1000.0) as u64);

    let bitrate = json["format"]["bit_rate"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let mut width = 0u32;
    let mut height = 0u32;
    let mut video_codec = String::new();
    let mut audio_codec = String::new();
    let mut fps = 0.0;

    if let Some(streams) = json["streams"].as_array() {
        for stream in streams {
            let codec_type = stream["codec_type"].as_str().unwrap_or("");
            let codec_name = stream["codec_name"].as_str().unwrap_or("").to_string();
            match codec_type {
                "video" => {
                    width = stream["width"].as_u64().unwrap_or(0) as u32;
                    height = stream["height"].as_u64().unwrap_or(0) as u32;
                    video_codec = codec_name;
                    if let Some(r_frame_rate) = stream["r_frame_rate"].as_str() {
                        if let Some(pos) = r_frame_rate.find('/') {
                            let num: f64 = r_frame_rate[..pos].parse().unwrap_or(0.0);
                            let den: f64 = r_frame_rate[pos + 1..].parse().unwrap_or(1.0);
                            if den > 0.0 {
                                fps = num / den;
                            }
                        }
                    }
                }
                "audio" => {
                    if audio_codec.is_empty() {
                        audio_codec = codec_name;
                    }
                }
                _ => {}
            }
        }
    }

    Ok(MediaInfo {
        width,
        height,
        duration_ms,
        video_codec,
        audio_codec,
        bitrate,
        fps,
    })
}

/// A streaming video frame reader that uses ffmpeg as a subprocess.
/// Decodes frames in a background thread and makes them available via
/// a non-blocking channel. This prevents UI thread blocking.
pub struct FfmpegFrameReader {
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

impl FfmpegFrameReader {
    /// Open a video file and start the ffmpeg subprocess in a background thread.
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
            .name("ffmpeg-decoder".into())
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

    /// Background thread: spawns ffmpeg and forwards raw frames through the channel.
    fn decoder_thread(
        path: &str,
        frame_size: usize,
        tx: mpsc::Sender<Vec<u8>>,
        running: Arc<AtomicBool>,
    ) {
        let process = match Command::new("ffmpeg")
            .args([
                "-v", "quiet", "-i", path, "-f", "rawvideo", "-pix_fmt", "rgb24", "-an", "-sn",
                "-dn", "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("ffmpeg spawn failed: {e}");
                return;
            }
        };

        let mut child = Some(process);
        let mut frame_buf = vec![0u8; frame_size];
        let mut buf_offset = 0usize;

        // Get a reference to stdout
        let stdout_ref = match child.as_mut() {
            Some(c) => match c.stdout.as_mut() {
                Some(s) => s,
                None => {
                    tracing::error!("ffmpeg: no stdout pipe");
                    return;
                }
            },
            None => return,
        };

        loop {
            if !running.load(Ordering::Relaxed) {
                break;
            }

            match stdout_ref.read(&mut frame_buf[buf_offset..]) {
                Ok(0) => break,
                Ok(n) => {
                    buf_offset += n;
                    if buf_offset >= frame_size {
                        if tx.send(frame_buf.clone()).is_err() {
                            break;
                        }
                        buf_offset = 0;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    tracing::warn!("ffmpeg read error: {e}");
                    break;
                }
            }
        }

        if let Some(mut c) = child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
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

    /// Try to read the next frame without blocking.
    /// Returns `None` if no frame is available yet, or `Some(frame_data)`.
    /// Returns `Err` if the decoder has stopped.
    pub fn try_read_frame(&mut self) -> Result<Option<Vec<u8>>, QvodError> {
        if !self.is_running() && self.frame_rx.try_recv().is_err() {
            return Err(QvodError::Decode("decoder stopped".into()));
        }

        match self.frame_rx.try_recv() {
            Ok(frame) => {
                // Approximate position
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

    /// Block and wait for the next frame.
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

    /// Close the decoder and stop the background thread.
    pub fn close(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }

    /// Check if the decoder background thread is still running.
    #[must_use]
    pub fn is_running(&mut self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

impl Drop for FfmpegFrameReader {
    fn drop(&mut self) {
        self.close();
    }
}

/// Read a single frame at the given timestamp and return raw RGB24 data.
pub fn extract_frame(path: &str, timestamp_ms: u64) -> Result<Option<Vec<u8>>, QvodError> {
    let info = probe_file(std::path::Path::new(path))?;
    if info.width == 0 || info.height == 0 {
        return Err(QvodError::Decode(
            "could not determine video dimensions".into(),
        ));
    }

    let width = info.width;
    let height = info.height;
    let frame_size = (width * height * 3) as usize;

    let seek_secs = timestamp_ms as f64 / 1000.0;
    let output = Command::new("ffmpeg")
        .args([
            "-v",
            "quiet",
            "-ss",
            &format!("{seek_secs:.3}"),
            "-i",
            path,
            "-vframes",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-an",
            "-sn",
            "-dn",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| QvodError::Decode(format!("ffmpeg seek frame failed: {e}")))?;

    if output.stdout.len() < frame_size {
        return Ok(None);
    }

    Ok(Some(output.stdout[..frame_size].to_vec()))
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
