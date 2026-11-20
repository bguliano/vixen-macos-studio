//! Decodes video with AVFoundation first, and bundled FFmpeg for unsupported
//! formats. Both paths stream bounded frames to the Video effect's disk cache.

use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetReader, AVAssetReaderStatus, AVAssetReaderTrackOutput, AVAssetTrack, AVMediaTypeVideo,
    AVURLAsset,
};
use objc2_core_foundation::CFString;
use objc2_core_media::{CMTime, CMTimeRange};
use objc2_core_video::{
    kCVPixelBufferHeightKey, kCVPixelBufferPixelFormatTypeKey, kCVPixelBufferWidthKey,
    kCVPixelFormatType_32BGRA, CVImageBuffer, CVPixelBufferGetBaseAddress,
    CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};
pub mod cache;
mod software;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Video dimensions and duration. The rendering metadata API reports display
/// size; the persisted source-metadata API reports coded size before rotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoInfo {
    pub width: i32,
    pub height: i32,
    pub duration_seconds: f64,
}

/// What `-ss start -t duration -r fps -vf scale=width:height` asks of ffmpeg.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoRequest {
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub fps: f64,
    pub width: i32,
    pub height: i32,
}

/// A decoded picture, top row first, as straight RGBA.
#[derive(Debug, PartialEq)]
pub struct Frame {
    pub width: i32,
    pub height: i32,
    pub rgba: Vec<u8>,
}

/// Quarter turns clockwise that a track's tag asks for on display.
fn quarter_turns(track: &AVAssetTrack) -> i32 {
    let t = unsafe { track.preferredTransform() };
    let degrees = t.b.atan2(t.a).to_degrees();
    ((degrees / 90.0).round() as i32).rem_euclid(4)
}

fn open(path: &Path) -> Option<(Retained<AVURLAsset>, Retained<AVAssetTrack>)> {
    if !path.is_file() {
        return None;
    }
    let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str()?));
    let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };
    let media_type = unsafe { AVMediaTypeVideo }?;
    #[allow(deprecated)]
    let tracks = unsafe { asset.tracksWithMediaType(media_type) };
    let track = tracks.firstObject()?;
    Some((asset, track))
}

/// The size and length of a video, or `None` when it can't be read.
pub fn video_info(path: &Path) -> Option<VideoInfo> {
    static NEVER_CANCEL: AtomicBool = AtomicBool::new(false);
    video_info_cancelable(path, &NEVER_CANCEL)
}

pub fn video_info_cancelable(path: &Path, cancel: &AtomicBool) -> Option<VideoInfo> {
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    let info = native_video_info(path).filter(|info| {
        info.width > 0
            && info.height > 0
            && info.duration_seconds.is_finite()
            && info.duration_seconds > 0.0
    });
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    info.or_else(|| software::video_info(path, cancel))
}

/// Tests that decode the sample assets call this first. Without the locally built
/// FFmpeg, metadata and AVI decoding fail quietly and the Video effect keeps its
/// defaults, which reads like a regression.
#[doc(hidden)]
pub fn require_media_tools_for_tests() {
    if let Err(error) = vixen_media_tools::Tool::Ffprobe.executable() {
        panic!("{error}. Build it once with scripts/build_media_tools.sh --fetch (the app build does this too).");
    }
}

/// Metadata persisted by Video.GetVideoInformation: coded dimensions (before
/// display rotation) and container duration at ffmpeg's displayed hundredths.
/// Rendering continues to use `video_info_cancelable`'s display dimensions.
pub fn video_source_info_cancelable(path: &Path, cancel: &AtomicBool) -> Option<VideoInfo> {
    if cancel.load(Ordering::Relaxed) { return None; }
    // Query coded size explicitly: naturalSize can include pixel aspect/clean
    // aperture adjustments. The original also obtains this from FFmpeg.
    software::source_info(path, cancel).map(|mut info| {
        // libavformat/dump.c adds 5000 microseconds before printing hundredths.
        info.duration_seconds = source_duration_seconds(info.duration_seconds);
        info
    })
}

fn source_duration_seconds(seconds: f64) -> f64 {
    (((seconds * 1_000_000.0).round() + 5000.0) / 10_000.0).floor() / 100.0
}

fn native_video_info(path: &Path) -> Option<VideoInfo> {
    autoreleasepool(|_| {
        let (asset, track) = open(path)?;
        #[allow(deprecated)]
        let (size, duration) = unsafe { (track.naturalSize(), asset.duration()) };
        let (w, h) = (size.width.round() as i32, size.height.round() as i32);
        let (width, height) = if quarter_turns(&track) % 2 == 1 {
            (h, w)
        } else {
            (w, h)
        };
        Some(VideoInfo {
            width,
            height,
            duration_seconds: unsafe { duration.seconds() },
        })
    })
}

fn settings(width: i32, height: i32) -> Retained<NSDictionary<NSString, AnyObject>> {
    // CFString and NSString are toll-free bridged.
    fn key(s: &'static CFString) -> &'static NSString {
        unsafe { &*(s as *const CFString as *const NSString) }
    }
    let format = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
    let (w, h) = (NSNumber::new_i32(width), NSNumber::new_i32(height));
    let keys = unsafe {
        [
            key(kCVPixelBufferPixelFormatTypeKey),
            key(kCVPixelBufferWidthKey),
            key(kCVPixelBufferHeightKey),
        ]
    };
    let values: [&AnyObject; 3] = [&format, &w, &h];
    NSDictionary::from_slices(&keys, &values)
}

fn copy_rgba(image: &CVImageBuffer) -> Option<(i32, i32, Vec<u8>)> {
    unsafe {
        if CVPixelBufferLockBaseAddress(image, CVPixelBufferLockFlags::ReadOnly) != 0 {
            return None;
        }
    }
    let result = (|| {
        let (width, height) = (CVPixelBufferGetWidth(image), CVPixelBufferGetHeight(image));
        let row = CVPixelBufferGetBytesPerRow(image);
        let row_bytes = width.checked_mul(4)?;
        let bytes = row_bytes.checked_mul(height)?;
        if bytes > MAX_FRAME_BYTES
            || row < row_bytes
            || row.checked_mul(height)? > isize::MAX as usize
        {
            return None;
        }
        let base = CVPixelBufferGetBaseAddress(image) as *const u8;
        if base.is_null() {
            return None;
        }
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(bytes).ok()?;
        rgba.resize(bytes, 0);
        for y in 0..height {
            // The locked pixel buffer guarantees its rows; checked dimensions
            // prevent overflow before constructing each borrowed row slice.
            let source = unsafe { std::slice::from_raw_parts(base.add(y * row), row_bytes) };
            for (out, bgra) in rgba[y * row_bytes..(y + 1) * row_bytes]
                .chunks_exact_mut(4)
                .zip(source.chunks_exact(4))
            {
                out.copy_from_slice(&[bgra[2], bgra[1], bgra[0], 255]);
            }
        }
        Some((
            i32::try_from(width).ok()?,
            i32::try_from(height).ok()?,
            rgba,
        ))
    })();
    unsafe { CVPixelBufferUnlockBaseAddress(image, CVPixelBufferLockFlags::ReadOnly) };
    result
}

/// Turns a picture clockwise to how it should display.
fn turned(width: i32, height: i32, rgba: Vec<u8>, turns: i32) -> Option<Frame> {
    if turns == 0 {
        return Some(Frame {
            width,
            height,
            rgba,
        });
    }
    let (w, h) = (width as usize, height as usize);
    let (out_w, out_h) = if turns % 2 == 1 { (h, w) } else { (w, h) };
    let mut out = Vec::new();
    out.try_reserve_exact(rgba.len()).ok()?;
    out.resize(rgba.len(), 0);
    for y in 0..h {
        for x in 0..w {
            let (ox, oy) = match turns {
                1 => (h - 1 - y, x),
                2 => (w - 1 - x, h - 1 - y),
                _ => (y, w - 1 - x),
            };
            let (from, to) = ((y * w + x) * 4, (oy * out_w + ox) * 4);
            out[to..to + 4].copy_from_slice(&rgba[from..from + 4]);
        }
    }
    Some(Frame {
        width: out_w as i32,
        height: out_h as i32,
        rgba: out,
    })
}

const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
const MAX_VECTOR_BYTES: u64 = 512 * 1024 * 1024;

/// Limits one decoded picture and the output index count, without limiting the
/// whole clip's decoded bytes. Production writes frames to its disk cache as they arrive.
pub(crate) fn frame_count(request: &VideoRequest) -> Option<usize> {
    if !request.start_seconds.is_finite()
        || !request.duration_seconds.is_finite()
        || !request.fps.is_finite()
        || request.start_seconds < 0.0
        || request.duration_seconds <= 0.0
        || request.fps <= 0.0
        || !(request.start_seconds + request.duration_seconds).is_finite()
        || !(request.duration_seconds + 1.0 / request.fps).is_finite()
        || request.width <= 0
        || request.height <= 0
    {
        return None;
    }
    let count = (request.duration_seconds * request.fps)
        .round_ties_even()
        .max(1.0);
    if !count.is_finite() || count > 250_000.0 {
        return None;
    }
    let bytes = (request.width as u64)
        .checked_mul(request.height as u64)?
        .checked_mul(4)?;
    (bytes <= MAX_FRAME_BYTES as u64).then_some(count as usize)
}

/// Decodes the requested stretch of a video at the requested rate and size. Each output frame shows the source frame nearest its
/// time, as ffmpeg's `-r` does, so a low rate source repeats frames. Returns what it got if the video ends first, and `None` if it
/// can't be read at all.
pub fn video_frames(path: &Path, request: &VideoRequest) -> Option<Vec<Arc<Frame>>> {
    static NEVER_CANCEL: AtomicBool = AtomicBool::new(false);
    video_frames_cancelable(path, request, &NEVER_CANCEL)
}

/// Convenience collector for tests and callers that need all frames at once.
/// Production cache writers use `video_frames_to` to avoid retaining a whole clip.
pub fn video_frames_cancelable(
    path: &Path,
    request: &VideoRequest,
    cancel: &AtomicBool,
) -> Option<Vec<Arc<Frame>>> {
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    let count = frame_count(request)?;
    let bytes = (request.width as u64)
        .checked_mul(request.height as u64)?
        .checked_mul(4)?
        .checked_mul(count as u64)?;
    if bytes > MAX_VECTOR_BYTES {
        return None;
    }
    let mut frames = Vec::new();
    frames.try_reserve_exact(count).ok()?;
    video_frames_to_resettable(path, request, cancel, |event| {
        match event {
            FrameEvent::Reset => frames.clear(),
            FrameEvent::Frame(_, frame) => frames.push(Arc::clone(frame)),
        }
        true
    })?;
    Some(frames)
}

/// Emits sequential sampled frames with only the current decoded source frame
/// retained internally. Repeated output samples share the same Arc. Returning
/// false from the sink or setting cancel aborts and returns None; accepted frames
/// may already have been emitted, so disk writers should publish only on success.
/// Like the vector API, a source ending early emits its last available frame once.
/// Unsupported formats retry with bundled FFmpeg before any output is accepted;
/// a later native failure returns None because this callback cannot retract data.
pub fn video_frames_to(
    path: &Path,
    request: &VideoRequest,
    cancel: &AtomicBool,
    mut sink: impl FnMut(usize, &Arc<Frame>) -> bool,
) -> Option<usize> {
    video_frames_to_resettable(path, request, cancel, |event| match event {
        FrameEvent::Frame(index, frame) => sink(index, frame),
        // This interface cannot retract accepted output. Cache/Vec callers use
        // the resettable variant to discard partial output before retrying.
        FrameEvent::Reset => false,
    })
}

pub(crate) enum FrameEvent<'a> {
    Reset,
    Frame(usize, &'a Arc<Frame>),
}

/// A failed decoder may have emitted earlier frames. Reset must discard all of
/// them before accepting a second decoder's output beginning again at index0.
pub(crate) fn video_frames_to_resettable(
    path: &Path,
    request: &VideoRequest,
    cancel: &AtomicBool,
    sink: impl FnMut(FrameEvent<'_>) -> bool,
) -> Option<usize> {
    frame_count(request)?;
    decode_with_fallback(
        cancel,
        sink,
        |emit| native_frames_to(path, request, cancel, emit),
        |emit| software::video_frames_to(path, request, cancel, emit),
    )
}

fn decode_with_fallback(
    cancel: &AtomicBool,
    mut sink: impl FnMut(FrameEvent<'_>) -> bool,
    native: impl FnOnce(&mut dyn FnMut(usize, &Arc<Frame>) -> bool) -> Option<usize>,
    fallback: impl FnOnce(&mut dyn FnMut(usize, &Arc<Frame>) -> bool) -> Option<usize>,
) -> Option<usize> {
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    let mut emitted = 0;
    let mut aborted = false;
    let result = native(&mut |index, frame| {
        emitted += 1;
        let accepted = sink(FrameEvent::Frame(index, frame));
        aborted |= !accepted;
        accepted
    });
    if cancel.load(Ordering::Relaxed) || aborted {
        return None;
    }
    if result.is_some() {
        return result;
    }
    if emitted != 0 && !sink(FrameEvent::Reset) {
        return None;
    }
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    fallback(&mut |index, frame| sink(FrameEvent::Frame(index, frame)))
}

fn native_frames_to(
    path: &Path,
    request: &VideoRequest,
    cancel: &AtomicBool,
    mut sink: impl FnMut(usize, &Arc<Frame>) -> bool,
) -> Option<usize> {
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    let count = frame_count(request)?;
    let (asset, track) = autoreleasepool(|_| open(path))?;
    let turns = quarter_turns(&track);
    // The reader works in the track's stored orientation.
    let (stored_w, stored_h) = if turns % 2 == 1 {
        (request.height, request.width)
    } else {
        (request.width, request.height)
    };
    let reader = unsafe { AVAssetReader::assetReaderWithAsset_error(&asset) }.ok()?;
    let output_settings = settings(stored_w, stored_h);
    let output = unsafe {
        AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(
            &track,
            Some(&output_settings),
        )
    };
    unsafe {
        output.setAlwaysCopiesSampleData(false);
        if !reader.canAddOutput(&output) {
            return None;
        }
        reader.addOutput(&output);
        let step = 1.0 / request.fps;
        const TIMESCALE: i32 = 600_000;
        reader.setTimeRange(CMTimeRange {
            start: CMTime::with_seconds(request.start_seconds, TIMESCALE),
            duration: CMTime::with_seconds(request.duration_seconds + step, TIMESCALE),
        });
        if !reader.startReading() {
            return None;
        }
    }

    struct StopReader<'a>(&'a AVAssetReader);
    impl Drop for StopReader<'_> {
        fn drop(&mut self) {
            unsafe { self.0.cancelReading() };
        }
    }
    let _stop_reader = StopReader(&reader);
    let step = 1.0 / request.fps;
    let mut emitted = 0;
    let mut current: Option<Arc<Frame>> = None;
    while emitted < count {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let more = autoreleasepool(|_| -> Option<bool> {
            let Some(sample) = (unsafe { output.copyNextSampleBuffer() }) else {
                // A decoder failure can occur after emitting valid earlier frames.
                // Only completed EOF may publish a shorter cache; errors abort it.
                return (unsafe { reader.status() } == AVAssetReaderStatus::Completed)
                    .then_some(false);
            };
            let Some(image) = (unsafe { sample.image_buffer() }) else {
                return Some(true);
            };
            let time =
                unsafe { sample.presentation_time_stamp().seconds() } - request.start_seconds;
            // Same nearest-frame boundary as the original vector decoder.
            while let Some(shown) = &current {
                if emitted < count && (emitted as f64 + 0.5) * step < time {
                    if cancel.load(Ordering::Relaxed) || !sink(emitted, shown) {
                        return None;
                    }
                    emitted += 1;
                } else {
                    break;
                }
            }
            let (width, height, rgba) = copy_rgba(&image)?;
            current = Some(Arc::new(turned(width, height, rgba, turns)?));
            Some(true)
        })?;
        if !more {
            break;
        }
    }
    if cancel.load(Ordering::Relaxed)
        || matches!(
            unsafe { reader.status() },
            AVAssetReaderStatus::Failed | AVAssetReaderStatus::Cancelled
        )
    {
        return None;
    }
    if emitted < count {
        if let Some(last) = current {
            if !sink(emitted, &last) || cancel.load(Ordering::Relaxed) {
                return None;
            }
            emitted += 1;
        }
    }
    Some(emitted)
}

#[cfg(test)]
mod decoder_tests;
