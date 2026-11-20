//! Local-only fallback for codecs not supported by AVAssetReader. FFmpeg owns
//! source timing/rotation; the raw RGBA pipe has one bounded frame assembler.
use crate::{frame_count, Frame, VideoInfo, VideoRequest, MAX_FRAME_BYTES};
use serde_json::Value;
use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use vixen_media_tools::{collect, stream, Tool};

pub(super) fn video_info(path: &Path, cancel: &AtomicBool) -> Option<VideoInfo> {
    probe_info(path, cancel, true)
}

pub(super) fn source_info(path: &Path, cancel: &AtomicBool) -> Option<VideoInfo> {
    probe_info(path, cancel, false)
}

fn probe_info(path: &Path, cancel: &AtomicBool, display_rotation: bool) -> Option<VideoInfo> {
    if !path.is_file() {
        return None;
    }
    let path = path.canonicalize().ok()?;
    let mut args: Vec<OsString> = [
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height,duration:stream_tags=rotate:stream_side_data=rotation:format=duration",
        "-of",
        "json",
        "-i",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    args.push(path.into_os_string());
    let bytes = collect(
        Tool::Ffprobe,
        &args,
        cancel,
        Some(Duration::from_secs(15)),
        65_536,
    )
    .ok()?;
    parse_info_for(&bytes, display_rotation)
}

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|n| n.is_finite())
}

#[cfg(test)]
fn parse_info(bytes: &[u8]) -> Option<VideoInfo> {
    parse_info_for(bytes, true)
}

fn parse_info_for(bytes: &[u8], display_rotation: bool) -> Option<VideoInfo> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let stream = value.get("streams")?.as_array()?.first()?;
    let width = i32::try_from(stream.get("width")?.as_i64()?).ok()?;
    let height = i32::try_from(stream.get("height")?.as_i64()?).ok()?;
    if width <= 0 || height <= 0 {
        return None;
    }
    let stream_duration = stream.get("duration").and_then(number);
    let container_duration = value.get("format").and_then(|format| format.get("duration")).and_then(number);
    let duration_seconds = if display_rotation { stream_duration.or(container_duration) } else { container_duration }?;
    if duration_seconds <= 0.0 {
        return None;
    }
    let rotation = stream
        .get("side_data_list")
        .and_then(Value::as_array)
        .and_then(|data| {
            data.iter()
                .find_map(|entry| entry.get("rotation").and_then(number))
        })
        .or_else(|| stream.get("tags")?.get("rotate").and_then(number))
        .unwrap_or(0.0);
    let turns = (rotation / 90.0).round().rem_euclid(4.0) as u8;
    let (width, height) = if display_rotation && turns % 2 == 1 {
        (height, width)
    } else {
        (width, height)
    };
    Some(VideoInfo {
        width,
        height,
        duration_seconds,
    })
}

pub(super) fn video_frames_to(
    path: &Path,
    request: &VideoRequest,
    cancel: &AtomicBool,
    mut sink: impl FnMut(usize, &Arc<Frame>) -> bool,
) -> Option<usize> {
    let count = frame_count(request)?;
    if cancel.load(Ordering::Relaxed) || !path.is_file() {
        return None;
    }
    let path = path.canonicalize().ok()?;
    // These are the original effect's -ss/-t/-r/scale semantics. Crop and the
    // user-specified rotation still run in the effect consumer. Container display
    // rotation is applied by FFmpeg before the requested scale, as with AVF.
    let mut args: Vec<OsString> = [
        "-xerror",
        "-max_alloc",
        "67108864",
        "-threads",
        "2",
        "-filter_threads",
        "1",
        "-ss",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    args.push(request.start_seconds.to_string().into());
    args.push("-i".into());
    args.push(path.into_os_string());
    args.extend(
        ["-map", "0:v:0", "-an", "-sn", "-dn", "-t"]
            .into_iter()
            .map(OsString::from),
    );
    args.push(request.duration_seconds.to_string().into());
    args.push("-vf".into());
    args.push(format!("scale={}:{}", request.width, request.height).into());
    args.push("-r".into());
    args.push(request.fps.to_string().into());
    args.push("-frames:v".into());
    args.push(count.to_string().into());
    args.extend(
        [
            "-threads", "1", "-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1",
        ]
        .into_iter()
        .map(OsString::from),
    );
    let mut frames = RawFrames::new(request, count)?;
    stream(Tool::Ffmpeg, &args, cancel, None, |bytes| {
        if cancel.load(Ordering::Relaxed) {
            return Err("Video decoding was cancelled".into());
        }
        frames.feed(bytes, &mut |index, frame| {
            !cancel.load(Ordering::Relaxed) && sink(index, frame)
        })
    })
    .ok()?;
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    frames.finish()
}

struct RawFrames {
    width: i32,
    height: i32,
    bytes_per_frame: usize,
    limit: usize,
    count: usize,
    pending: Vec<u8>,
    previous: Option<Arc<Frame>>,
}
impl RawFrames {
    fn new(request: &VideoRequest, limit: usize) -> Option<Self> {
        let bytes = (request.width as usize)
            .checked_mul(request.height as usize)?
            .checked_mul(4)?;
        if bytes == 0 || bytes > MAX_FRAME_BYTES {
            return None;
        }
        Some(Self {
            width: request.width,
            height: request.height,
            bytes_per_frame: bytes,
            limit,
            count: 0,
            pending: Vec::new(),
            previous: None,
        })
    }
    fn feed(
        &mut self,
        mut bytes: &[u8],
        sink: &mut impl FnMut(usize, &Arc<Frame>) -> bool,
    ) -> Result<(), String> {
        while !bytes.is_empty() {
            if self.count >= self.limit {
                return Err("Video decoder exceeded the requested frame count".into());
            }
            let take = bytes.len().min(self.bytes_per_frame - self.pending.len());
            self.pending
                .try_reserve(take)
                .map_err(|_| "Not enough memory for a video frame".to_string())?;
            self.pending.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
            if self.pending.len() == self.bytes_per_frame {
                let frame = if let Some(previous) = self
                    .previous
                    .as_ref()
                    .filter(|frame| frame.rgba == self.pending)
                {
                    self.pending.clear();
                    previous.clone()
                } else {
                    Arc::new(Frame {
                        width: self.width,
                        height: self.height,
                        rgba: std::mem::take(&mut self.pending),
                    })
                };
                if !sink(self.count, &frame) {
                    return Err("Video frame consumer stopped".into());
                }
                self.previous = Some(frame);
                self.count += 1;
            }
        }
        Ok(())
    }
    fn finish(self) -> Option<usize> {
        self.pending.is_empty().then_some(self.count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_handles_rotation_container_duration_and_invalid_numbers() {
        let input = br#"{"streams":[{"width":160,"height":90,"duration":"1.0","tags":{"rotate":"90"}}],"format":{"duration":"2.345"}}"#;
        assert_eq!(parse_info_for(input, false), Some(VideoInfo { width: 160, height: 90, duration_seconds: 2.345 }));
        assert_eq!(parse_info(input), Some(VideoInfo { width: 90, height: 160, duration_seconds: 1.0 }));
        assert_eq!(parse_info(br#"{"streams":[{"width":160,"height":90,"duration":"3.0","side_data_list":[{"rotation":-90}]}]}"#),
            Some(VideoInfo { width: 90, height: 160, duration_seconds: 3.0 }));
        assert_eq!(parse_info(br#"{"streams":[{"width":20,"height":10,"duration":"N/A"}],"format":{"duration":"1.5"}}"#),
            Some(VideoInfo { width: 20, height: 10, duration_seconds: 1.5 }));
        for input in [
            r#"{"streams":[]}"#,
            r#"{"streams":[{"width":0,"height":1,"duration":"1"}]}"#,
            r#"{"streams":[{"width":1,"height":1,"duration":"inf"}]}"#,
            r#"{"streams":[{"width":1,"height":1,"duration":"-2"}]}"#,
        ] {
            assert!(parse_info(input.as_bytes()).is_none());
        }
    }
    #[test]
    fn raw_chunks_are_bounded_sequential_share_repeats_and_reject_truncation_or_excess() {
        let request = VideoRequest {
            start_seconds: 0.0,
            duration_seconds: 3.0,
            fps: 1.0,
            width: 2,
            height: 1,
        };
        let mut frames = RawFrames::new(&request, 3).unwrap();
        let data = [vec![1; 8], vec![1; 8], vec![3; 8]].concat();
        let mut accepted = Vec::new();
        for chunk in data.chunks(3) {
            frames
                .feed(chunk, &mut |index, frame| {
                    assert_eq!(index, accepted.len());
                    accepted.push(frame.clone());
                    true
                })
                .unwrap();
        }
        assert_eq!(frames.finish(), Some(3));
        assert!(Arc::ptr_eq(&accepted[0], &accepted[1]));
        assert_eq!(accepted[2].rgba, vec![3; 8]);
        let mut frames = RawFrames::new(&request, 1).unwrap();
        frames.feed(&[1; 7], &mut |_, _| true).unwrap();
        assert_eq!(frames.finish(), None);
        let mut frames = RawFrames::new(&request, 1).unwrap();
        assert!(frames.feed(&[1; 9], &mut |_, _| true).is_err());
        let mut frames = RawFrames::new(&request, 1).unwrap();
        assert!(frames.feed(&[1; 8], &mut |_, _| false).is_err());
    }

    #[test]
    fn actual_bundled_software_reads_metadata_rotation_and_avi_frames() {
        crate::require_media_tools_for_tests();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/assets");
        let cancel = AtomicBool::new(false);
        let info = video_info(&root.join("colors.avi"), &cancel).unwrap();
        assert_eq!((info.width, info.height), (160, 90));
        assert!((info.duration_seconds - 3.0).abs() < 0.05);
        let info = video_info(&root.join("quarter_turn.mov"), &cancel).unwrap();
        assert_eq!((info.width, info.height), (90, 160));
        let source = source_info(&root.join("quarter_turn.mov"), &cancel).unwrap();
        assert_eq!((source.width, source.height), (160, 90));
        let request = VideoRequest {
            start_seconds: 0.0,
            duration_seconds: 0.5,
            fps: 10.0,
            width: 9,
            height: 16,
        };
        let mut frames = Vec::new();
        let count = video_frames_to(
            &root.join("quarter_turn.mov"),
            &request,
            &cancel,
            |index, frame| {
                assert_eq!(index, frames.len());
                frames.push(frame.clone());
                true
            },
        )
        .unwrap();
        assert_eq!(count, 5);
        let frame = &frames[2];
        let top = ((2 * frame.width + 4) * 4) as usize;
        let bottom = ((13 * frame.width + 4) * 4) as usize;
        assert!(
            frame.rgba[top] > 210 && frame.rgba[top + 2] < 40,
            "top must remain red"
        );
        assert!(
            frame.rgba[bottom] < 40 && frame.rgba[bottom + 2] > 210,
            "bottom must remain blue"
        );
    }
}
