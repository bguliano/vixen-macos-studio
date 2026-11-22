//! H.264/AAC MP4 (or ProRes 4444 MOV) writing through AVFoundation, so preview videos need no ffmpeg. Frames come from a caller fill function;
//! the sequence audio is read from its media file for the same span and encoded alongside, interleaved by the writer.
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetReader, AVAssetReaderStatus, AVAssetReaderTrackOutput, AVAssetWriter,
    AVAssetWriterInput, AVAssetWriterInputPixelBufferAdaptor, AVAssetWriterStatus, AVFileTypeMPEG4,
    AVFileTypeQuickTimeMovie, AVMediaTypeAudio, AVMediaTypeVideo, AVURLAsset,
    AVVideoAverageBitRateKey, AVVideoCodecKey, AVVideoCodecTypeAppleProRes4444,
    AVVideoCodecTypeH264, AVVideoColorPrimariesKey, AVVideoColorPrimaries_ITU_R_709_2,
    AVVideoColorPropertiesKey, AVVideoCompressionPropertiesKey, AVVideoExpectedSourceFrameRateKey,
    AVVideoHeightKey, AVVideoMaxKeyFrameIntervalKey, AVVideoProfileLevelH264HighAutoLevel,
    AVVideoProfileLevelKey, AVVideoTransferFunctionKey, AVVideoTransferFunction_ITU_R_709_2,
    AVVideoWidthKey, AVVideoYCbCrMatrixKey, AVVideoYCbCrMatrix_ITU_R_709_2,
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

/// How the frames are compressed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Codec {
    /// H.264 High profile, 4:2:0, at this average bit rate (bits per second), in an MP4 or MOV.
    H264 { bitrate: u32 },
    /// ProRes 4444, a near-lossless master, in a MOV.
    ProRes4444,
}

impl Codec {
    pub fn describe(self) -> String {
        match self {
            Codec::H264 { bitrate } => format!("H.264 High 4:2:0 at {} kb/s", bitrate / 1000),
            Codec::ProRes4444 => "Apple ProRes 4444".into(),
        }
    }
}

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

/// The writer's video settings: the codec, the size and, for H.264, High profile at `bitrate` with BT.709 colour tags,
/// so players show the colours the frames were painted with.
unsafe fn video_settings(
    codec: Codec,
    width: u32,
    height: u32,
    fps: f64,
) -> Result<Retained<NSDictionary<NSString, AnyObject>>, String> {
    let missing = "AVFoundation video keys unavailable";
    let (w, h) = (NSNumber::new_u32(width), NSNumber::new_u32(height));
    let colors = dictionary(&[
        (AVVideoColorPrimariesKey.ok_or(missing)?, AVVideoColorPrimaries_ITU_R_709_2.ok_or(missing)?),
        (AVVideoTransferFunctionKey.ok_or(missing)?, AVVideoTransferFunction_ITU_R_709_2.ok_or(missing)?),
        (AVVideoYCbCrMatrixKey.ok_or(missing)?, AVVideoYCbCrMatrix_ITU_R_709_2.ok_or(missing)?),
    ]);
    let mut entries: Vec<(&NSString, &AnyObject)> = vec![
        (AVVideoWidthKey.ok_or(missing)?, &w),
        (AVVideoHeightKey.ok_or(missing)?, &h),
        (AVVideoColorPropertiesKey.ok_or(missing)?, &colors),
    ];
    let compression;
    let (rate, frame_rate, key_interval) = (
        NSNumber::new_u32(match codec { Codec::H264 { bitrate } => bitrate, Codec::ProRes4444 => 0 }),
        NSNumber::new_f64(fps),
        // A key frame every two seconds, so a player can seek anywhere quickly.
        NSNumber::new_u32((fps * 2.0).round().max(1.0) as u32),
    );
    match codec {
        Codec::H264 { .. } => {
            compression = dictionary(&[
                (AVVideoAverageBitRateKey.ok_or(missing)?, &rate),
                (AVVideoProfileLevelKey.ok_or(missing)?, AVVideoProfileLevelH264HighAutoLevel.ok_or(missing)?),
                (AVVideoExpectedSourceFrameRateKey.ok_or(missing)?, &frame_rate),
                (AVVideoMaxKeyFrameIntervalKey.ok_or(missing)?, &key_interval),
            ]);
            entries.push((AVVideoCodecKey.ok_or(missing)?, AVVideoCodecTypeH264.ok_or(missing)?));
            entries.push((AVVideoCompressionPropertiesKey.ok_or(missing)?, &compression));
        }
        Codec::ProRes4444 => {
            entries.push((AVVideoCodecKey.ok_or(missing)?, AVVideoCodecTypeAppleProRes4444.ok_or(missing)?));
        }
    }
    Ok(dictionary(&entries))
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

/// Writes `fill`'s frames (BGRA rows, given the row stride) at `fps` from `start_ms`, until it returns `None`. A `.mov`
/// path gets a QuickTime file, anything else MP4.
/// The writer goes to a sibling temporary file renamed into place, so a failure never leaves a half-written video.
#[allow(clippy::too_many_arguments)]
pub fn write(
    path: &Path,
    codec: Codec,
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
    let movie = name.to_lowercase().ends_with(".mov");
    let temporary = directory.join(format!(
        ".{name}.{}.partial.{}",
        uuid::Uuid::new_v4(),
        if movie { "mov" } else { "mp4" }
    ));
    struct Remove<'a>(&'a Path);
    impl Drop for Remove<'_> {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0);
        }
    }
    let _cleanup = Remove(&temporary);
    let written = autoreleasepool(|_| {
        encode(
            &temporary, movie, codec, width, height, fps, start_ms, end_ms, audio_path, fill,
        )
    })?;
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())?;
    Ok(written)
}

#[allow(clippy::too_many_arguments)]
fn encode(
    path: &Path,
    movie: bool,
    codec: Codec,
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
    let file_type = unsafe { if movie { AVFileTypeQuickTimeMovie } else { AVFileTypeMPEG4 } }
        .ok_or("AVFoundation file type unavailable")?;
    let writer = unsafe { AVAssetWriter::assetWriterWithURL_fileType_error(&url, file_type) }
        .map_err(|e| e.localizedDescription().to_string())?;
    let video_settings = unsafe { video_settings(codec, width, height, fps) }?;
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
            return Err(format!("The video writer refused {} at {width}x{height}", codec.describe()));
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
