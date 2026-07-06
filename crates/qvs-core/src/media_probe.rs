use std::path::Path;
use std::process::Command;

/// Probing result returned by both ffprobe and ffmpeg parsing.
#[derive(Debug, Default, Clone)]
pub struct MediaProbeResult {
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub video_codec: String,
    pub audio_codec: String,
    pub bitrate: u64,
    pub fps: f64,
}

/// Probe a media file — tries `ffprobe` first, falls back to `ffmpeg -i` stderr parsing.
#[must_use]
pub fn probe_media_file(path: &Path) -> Option<MediaProbeResult> {
    try_ffprobe(path).or_else(|| try_ffmpeg_probe(path))
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn try_ffprobe(path: &Path) -> Option<MediaProbeResult> {
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
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;

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

    Some(MediaProbeResult {
        duration_ms,
        width,
        height,
        video_codec,
        audio_codec,
        bitrate,
        fps,
    })
}

fn try_ffmpeg_probe(path: &Path) -> Option<MediaProbeResult> {
    let output = Command::new("ffmpeg")
        .arg("-i")
        .arg(path.as_os_str())
        .output()
        .ok()?;

    // ffmpeg outputs info to stderr when no output file is specified
    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_ffmpeg_stderr(&stderr)
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn parse_ffmpeg_stderr(stderr: &str) -> Option<MediaProbeResult> {
    let mut duration_ms = 0u64;
    let mut bitrate = 0u64;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut video_codec = String::new();
    let mut audio_codec = String::new();
    let mut fps = 0.0;

    let mut found_video = false;

    for line in stderr.lines() {
        let line = line.trim_start();
        // Parse Duration: 00:03:30.12, start: ..., bitrate: 2123 kb/s
        if let Some(dur_str) = line.strip_prefix("Duration: ") {
            let parts: Vec<&str> = dur_str.splitn(2, ',').collect();
            if let Some(time_part) = parts.first() {
                let time_str = time_part.trim();
                let hms: Vec<&str> = time_str.split(':').collect();
                if hms.len() == 3 {
                    let hours: u64 = hms[0].parse().unwrap_or(0);
                    let minutes: u64 = hms[1].parse().unwrap_or(0);
                    let secs: f64 = hms[2].parse().unwrap_or(0.0);
                    duration_ms =
                        ((hours as f64 * 3600.0 + minutes as f64 * 60.0 + secs) * 1000.0) as u64;
                }
            }
            // Parse bitrate from Duration line ("..., bitrate: 2123 kb/s")
            if let Some(br_rest) = parts.get(1) {
                if let Some(bitrate_pos) = br_rest.find("bitrate: ") {
                    let br_val = &br_rest[bitrate_pos + "bitrate: ".len()..];
                    let val: String = br_val.chars().take_while(char::is_ascii_digit).collect();
                    bitrate = val.parse::<u64>().unwrap_or(0) * 1000;
                }
            }
        }

        // Parse Stream #0:0: Video: h264, ..., 1920x1080, ..., 25 fps, ...
        if line.contains(": Video: ") && !found_video {
            found_video = true;
            // Extract codec name (after ": Video: " and before first comma)
            if let Some(video_section) = line.split(": Video: ").nth(1) {
                let codec_parts: Vec<&str> = video_section.splitn(2, ',').collect();
                if let Some(codec) = codec_parts.first() {
                    video_codec = codec.trim().to_string();
                    // Clean up parentheses like "h264 (High)"
                    if let Some(paren) = video_codec.find(" (") {
                        video_codec = video_codec[..paren].to_string();
                    }
                }
            }
            for token in line.split(',') {
                let token = token.trim();
                // Extract resolution (e.g., "1920x1080" from "1920x1080 [SAR ...]")
                if let Some(x_pos) = token.find('x') {
                    if x_pos > 0 {
                        let left: &str = &token[..x_pos];
                        let right_part: &str = &token[x_pos + 1..];
                        let right: String = right_part
                            .chars()
                            .take_while(char::is_ascii_digit)
                            .collect();
                        if left.chars().all(|c| c.is_ascii_digit()) && !right.is_empty() {
                            width = left.parse().unwrap_or(0);
                            height = right.parse().unwrap_or(0);
                        }
                    }
                }
                // Extract fps (e.g., "25 fps" or "13.45 fps")
                if token.ends_with(" fps") || token.ends_with(" fps,") {
                    let fps_str = token
                        .trim_end_matches(" fps")
                        .trim_end_matches(" fps,")
                        .trim();
                    fps = fps_str.parse().unwrap_or(0.0);
                }
            }
        }

        // Parse Stream #0:1: Audio: aac, ...
        if line.contains(": Audio: ") && audio_codec.is_empty() {
            if let Some(audio_section) = line.split(": Audio: ").nth(1) {
                let codec_parts: Vec<&str> = audio_section.splitn(2, ',').collect();
                if let Some(codec) = codec_parts.first() {
                    audio_codec = codec.trim().to_string();
                    if let Some(paren) = audio_codec.find(" (") {
                        audio_codec = audio_codec[..paren].to_string();
                    }
                }
            }
        }
    }

    if duration_ms == 0
        && bitrate == 0
        && width == 0
        && height == 0
        && video_codec.is_empty()
        && audio_codec.is_empty()
        && fps == 0.0
    {
        return None;
    }

    Some(MediaProbeResult {
        duration_ms,
        width,
        height,
        video_codec,
        audio_codec,
        bitrate,
        fps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ffmpeg_stderr_duration() {
        let stderr = "  Duration: 00:03:30.12, start: 0.000000, bitrate: 2123 kb/s\n";
        let result = parse_ffmpeg_stderr(stderr).unwrap();
        // 3*3600 + 30*60 + 30.12 = 10800 + 1800 + 30.12 = 12630.12 sec → 12630120 ms
        // But our test uses 00:03:30.12 = 210.12 sec → 210120 ms
        assert_eq!(result.duration_ms, 210120);
        assert_eq!(result.bitrate, 2123000);
    }

    #[test]
    fn test_parse_ffmpeg_stderr_video() {
        let stderr = "  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), 1920x1080 [SAR 1:1 DAR 16:9], 25 fps, 25 tbr, 90k tbn, 50 tbc (default)\n";
        let result = parse_ffmpeg_stderr(stderr).unwrap();
        assert_eq!(result.video_codec, "h264");
        assert_eq!(result.width, 1920);
        assert_eq!(result.height, 1080);
        assert!((result.fps - 25.0).abs() < 0.1);
    }

    #[test]
    fn test_parse_ffmpeg_stderr_audio() {
        let stderr = "  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 44100 Hz, stereo, fltp, 128 kb/s (default)\n";
        let result = parse_ffmpeg_stderr(stderr).unwrap();
        assert_eq!(result.audio_codec, "aac");
    }

    #[test]
    fn test_parse_ffmpeg_stderr_full() {
        let stderr = "\
  Duration: 00:00:37.41, start: 0.000000, bitrate: 2123 kb/s
  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), 1080x1920, 1995 kb/s, 13.45 fps, 10 tbr, 1000k tbn (default)
  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 44100 Hz, stereo, fltp, 128 kb/s (default)
";
        let result = parse_ffmpeg_stderr(stderr).unwrap();
        // 37.41 sec
        assert_eq!(result.duration_ms, 37410);
        assert_eq!(result.bitrate, 2123000);
        assert_eq!(result.video_codec, "h264");
        assert_eq!(result.width, 1080);
        assert_eq!(result.height, 1920);
        assert!((result.fps - 13.45).abs() < 0.01);
        assert_eq!(result.audio_codec, "aac");
    }

    #[test]
    fn test_parse_empty_stderr() {
        let result = parse_ffmpeg_stderr("");
        assert!(result.is_none());
    }

    #[test]
    fn test_no_video_stream() {
        let stderr = "\
  Duration: 00:01:00.00, start: 0.000000, bitrate: 128 kb/s
  Stream #0:0: Audio: mp3, 44100 Hz, stereo, 128 kb/s
";
        let result = parse_ffmpeg_stderr(stderr).unwrap();
        assert_eq!(result.duration_ms, 60000);
        assert_eq!(result.audio_codec, "mp3");
        assert_eq!(result.width, 0); // No video stream
    }
}
