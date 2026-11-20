//! H.264/AAC MP4 writing through AVFoundation, so preview videos need no ffmpeg. Frames come from a caller fill function;
//! the sequence audio is read from its media file for the same span and encoded alongside, interleaved by the writer.
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetReader, AVAssetReaderStatus, AVAssetReaderTrackOutput, AVAssetWriter,
    AVAssetWriterInput, AVAssetWriterInputPixelBufferAdaptor, AVAssetWriterStatus, AVFileTypeMPEG4,
    AVMediaTypeAudio, AVMediaTypeVideo, AVURLAsset, AVVideoCodecKey, AVVideoCodecTypeH264,
    AVVideoHeightKey, AVVideoWidthKey,
};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{CMTime, CMTimeRange};
use objc2_core_video::{
    kCVPixelFormatType_32BGRA, CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddress,
    CVPixelBufferGetBytesPerRow, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
    CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};
use std::{path::Path, ptr::NonNull};

const TIMESCALE: i32 = 90_000;
// Core Audio format IDs ('lpcm', 'aac ') and AVFAudio's settings keys, whose values are their own names.
const LINEAR_PCM: u32 = 0x6C70_636D;
const MPEG4_AAC: u32 = 0x6161_6320;

pub struct Written {
    pub frames: usize,
    /// The audio file name, or why the video has none.
    pub audio: serde_json::Value,
}

fn dictionary(entries: &[(&NSString, &AnyObject)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<&NSString> = entries.iter().map(|(k, _)| *k).collect();
    let values: Vec<&AnyObject> = entries.iter().map(|(_, v)| *v).collect();
    NSDictionary::from_slices(&keys, &values)
}

fn pcm_settings(aac: bool) -> Retained<NSDictionary<NSString, AnyObject>> {
    let format = NSNumber::new_u32(if aac { MPEG4_AAC } else { LINEAR_PCM });
    let (rate, channels) = (NSNumber::new_f64(44_100.0), NSNumber::new_i32(2));
    let (bits, no) = (NSNumber::new_i32(16), NSNumber::new_bool(false));
    let bitrate = NSNumber::new_i32(192_000);
    let key = NSString::from_str;
    let (format_key, rate_key, channels_key) = (
        key("AVFormatIDKey"),
        key("AVSampleRateKey"),
        key("AVNumberOfChannelsKey"),
    );
    let mut entries: Vec<(Retained<NSString>, &AnyObject)> = vec![
        (format_key, &format),
        (rate_key, &rate),
        (channels_key, &channels),
    ];
    if aac {
        entries.push((key("AVEncoderBitRateKey"), &bitrate));
    } else {
        entries.push((key("AVLinearPCMBitDepthKey"), &bits));
        for name in [
            "AVLinearPCMIsFloatKey",
            "AVLinearPCMIsBigEndianKey",
            "AVLinearPCMIsNonInterleaved",
        ] {
            entries.push((key(name), &no));
        }
    }
    let borrowed: Vec<(&NSString, &AnyObject)> = entries.iter().map(|(k, v)| (&**k, *v)).collect();
    dictionary(&borrowed)
}

/// The sequence audio for `start..end`, decoded to PCM for the writer's AAC input.
struct Audio {
    reader: Retained<AVAssetReader>,
    output: Retained<AVAssetReaderTrackOutput>,
}

impl Audio {
    fn open(path: &Path, start_ms: f64, end_ms: f64) -> Result<Self, String> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(
            path.to_str().ok_or("Audio path is not UTF-8")?,
        ));
        let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };
        let media = unsafe { AVMediaTypeAudio }.ok_or("AVFoundation audio type unavailable")?;
        #[allow(deprecated)]
        let tracks = unsafe { asset.tracksWithMediaType(media) };
        let track = tracks
            .firstObject()
            .ok_or("AVFoundation cannot read an audio track from this file")?;
        let reader = unsafe { AVAssetReader::assetReaderWithAsset_error(&asset) }
            .map_err(|e| e.localizedDescription().to_string())?;
        let settings = pcm_settings(false);
        let output = unsafe {
            AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(
                &track,
                Some(&settings),
            )
        };
        unsafe {
            if !reader.canAddOutput(&output) {
                return Err("Audio cannot be decoded".into());
            }
            reader.addOutput(&output);
            reader.setTimeRange(CMTimeRange {
                start: CMTime::with_seconds(start_ms / 1000.0, TIMESCALE),
                duration: CMTime::with_seconds((end_ms - start_ms) / 1000.0, TIMESCALE),
            });
            if !reader.startReading() {
                return Err("Audio could not be read".into());
            }
        }
        Ok(Self { reader, output })
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        unsafe { self.reader.cancelReading() };
    }
}

fn pixel_buffer(width: usize, height: usize) -> Result<CFRetained<CVPixelBuffer>, String> {
    let mut out: *mut CVPixelBuffer = std::ptr::null_mut();
    let status = unsafe {
        CVPixelBufferCreate(
            None,
            width,
            height,
            kCVPixelFormatType_32BGRA,
            None,
            NonNull::from(&mut out),
        )
    };
    NonNull::new(out)
        .filter(|_| status == 0)
        .map(|p| unsafe { CFRetained::from_raw(p) })
        .ok_or(format!("Could not allocate a video frame ({status})"))
}

/// Writes `fill`'s frames (BGRA rows, given the row stride) at `fps` from `start_ms`, until it returns `None`.
/// The writer goes to a sibling temporary file renamed into place, so a failure never leaves a half-written video.
#[allow(clippy::too_many_arguments)]
pub fn write(
    path: &Path,
    width: u32,
    height: u32,
    fps: f64,
    start_ms: f64,
    end_ms: f64,
    audio_path: Option<&Path>,
    fill: &mut dyn FnMut(&mut [u8], usize) -> Option<()>,
) -> Result<Written, String> {
    let directory = path.parent().ok_or("Output has no folder")?;
    let name = path
        .file_name()
        .ok_or("Output has no name")?
        .to_string_lossy();
    let temporary = directory.join(format!(".{name}.{}.partial.mp4", uuid::Uuid::new_v4()));
    struct Remove<'a>(&'a Path);
    impl Drop for Remove<'_> {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0);
        }
    }
    let _cleanup = Remove(&temporary);
    let written = autoreleasepool(|_| {
        encode(
            &temporary, width, height, fps, start_ms, end_ms, audio_path, fill,
        )
    })?;
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())?;
    Ok(written)
}

#[allow(clippy::too_many_arguments)]
fn encode(
    path: &Path,
    width: u32,
    height: u32,
    fps: f64,
    start_ms: f64,
    end_ms: f64,
    audio_path: Option<&Path>,
    fill: &mut dyn FnMut(&mut [u8], usize) -> Option<()>,
) -> Result<Written, String> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(
        path.to_str().ok_or("Output path is not UTF-8")?,
    ));
    let file_type = unsafe { AVFileTypeMPEG4 }.ok_or("AVFoundation MP4 type unavailable")?;
    let writer = unsafe { AVAssetWriter::assetWriterWithURL_fileType_error(&url, file_type) }
        .map_err(|e| e.localizedDescription().to_string())?;
    let (w, h) = (NSNumber::new_u32(width), NSNumber::new_u32(height));
    let video_settings = unsafe {
        dictionary(&[
            (
                AVVideoCodecKey.ok_or("AVFoundation video keys unavailable")?,
                AVVideoCodecTypeH264.unwrap(),
            ),
            (AVVideoWidthKey.unwrap(), &w),
            (AVVideoHeightKey.unwrap(), &h),
        ])
    };
    let video_type = unsafe { AVMediaTypeVideo }.ok_or("AVFoundation video type unavailable")?;
    let video = unsafe {
        AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
            video_type,
            Some(&video_settings),
        )
    };
    unsafe { video.setExpectsMediaDataInRealTime(false) };
    let adaptor = unsafe {
        AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(&video, None)
    };
    unsafe {
        if !writer.canAddInput(&video) {
            return Err("The video writer refused H.264 at this size".into());
        }
        writer.addInput(&video);
    }

    let mut audio_note = serde_json::Value::Null;
    let mut audio = None;
    if let Some(source) = audio_path {
        match Audio::open(source, start_ms, end_ms) {
            Ok(reader) => {
                let settings = pcm_settings(true);
                let audio_type = unsafe { AVMediaTypeAudio }.unwrap();
                let input = unsafe {
                    AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
                        audio_type,
                        Some(&settings),
                    )
                };
                unsafe { input.setExpectsMediaDataInRealTime(false) };
                if unsafe { writer.canAddInput(&input) } {
                    unsafe { writer.addInput(&input) };
                    audio_note = serde_json::json!(source
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned()));
                    audio = Some((reader, input));
                } else {
                    audio_note = serde_json::json!({"omitted": "the writer refused AAC audio"});
                }
            }
            Err(reason) => audio_note = serde_json::json!({"omitted": reason}),
        }
    }

    unsafe {
        let start = CMTime::with_seconds(start_ms / 1000.0, TIMESCALE);
        if !writer.startWriting() {
            return Err(writer
                .error()
                .map(|e| e.localizedDescription().to_string())
                .unwrap_or("Video writer did not start".into()));
        }
        writer.startSessionAtSourceTime(start);
    }
    let start_ticks = (start_ms * f64::from(TIMESCALE) / 1000.0).round() as i64;
    let (mut frames, mut video_done, mut audio_done) = (0usize, false, audio.is_none());
    // Feed both inputs as each becomes ready: the writer interleaves them and stalls one that runs ahead.
    while !(video_done && audio_done) {
        let mut progressed = false;
        if !video_done && unsafe { video.isReadyForMoreMediaData() } {
            let buffer = pixel_buffer(width as usize, height as usize)?;
            unsafe { CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags(0)) };
            let row = CVPixelBufferGetBytesPerRow(&buffer);
            let base = CVPixelBufferGetBaseAddress(&buffer) as *mut u8;
            // The locked buffer owns `row * height` bytes until unlocked.
            let filled = (!base.is_null())
                .then(|| {
                    fill(
                        unsafe { std::slice::from_raw_parts_mut(base, row * height as usize) },
                        row,
                    )
                })
                .flatten();
            unsafe { CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags(0)) };
            if filled.is_some() {
                let ticks =
                    start_ticks + (frames as f64 * f64::from(TIMESCALE) / fps).round() as i64;
                if !unsafe {
                    adaptor.appendPixelBuffer_withPresentationTime(
                        &buffer,
                        CMTime::new(ticks, TIMESCALE),
                    )
                } {
                    break;
                }
                frames += 1;
            } else {
                unsafe { video.markAsFinished() };
                video_done = true;
            }
            progressed = true;
        }
        if let (false, Some((reader, input))) = (audio_done, &audio) {
            if unsafe { input.isReadyForMoreMediaData() } {
                match unsafe { reader.output.copyNextSampleBuffer() } {
                    Some(sample) => {
                        if !unsafe { input.appendSampleBuffer(&sample) } {
                            break;
                        }
                    }
                    None => {
                        if unsafe { reader.reader.status() } == AVAssetReaderStatus::Failed {
                            audio_note =
                                serde_json::json!({"omitted": "audio decoding failed partway"});
                        }
                        unsafe { input.markAsFinished() };
                        audio_done = true;
                    }
                }
                progressed = true;
            }
        }
        if !progressed {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        if unsafe { writer.status() } == AVAssetWriterStatus::Failed {
            break;
        }
    }
    // The synchronous finishWriting must not run on the main thread, where the headless bridge serves calls.
    let (done, finished) = std::sync::mpsc::channel();
    let handler = block2::RcBlock::new(move || {
        let _ = done.send(());
    });
    unsafe { writer.finishWritingWithCompletionHandler(&handler) };
    let _ = finished.recv_timeout(std::time::Duration::from_secs(120));
    if unsafe { writer.status() } != AVAssetWriterStatus::Completed {
        return Err(unsafe { writer.error() }
            .map(|e| e.localizedDescription().to_string())
            .unwrap_or("Video writing failed".into()));
    }
    Ok(Written {
        frames,
        audio: audio_note,
    })
}
