//! Runs one [`Job`] to completion.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use crate::error::{Error, Result};
use crate::ffmpeg::Ffmpeg;
use crate::formats::{self, Category};
use crate::job::{Job, Task};
use crate::runner::{Ended, Progress, run_ffmpeg};
use crate::{args, atomic, disc, image_conv, pdf, probe};
use cd_da_reader::{AudioSectorReader, Toc};
use std::ffi::OsString;
use std::path::Path;

/// Share of a CD job's progress bar spent reading the disc (the rest encodes).
const RIP_SHARE: f32 = 0.8;

fn format(id: &str) -> Result<&'static formats::Format> {
    formats::find(id).ok_or_else(|| Error::UnknownFormat(id.to_string()))
}

/// Executes the job and returns the path actually written. Panics escaping
/// any converter are turned into errors so one bad file never takes the
/// queue down.
pub fn run_job(job: &Job, progress: Progress<'_>, cancel: &AtomicBool) -> Result<PathBuf> {
    match catch_unwind(AssertUnwindSafe(|| run_inner(job, progress, cancel))) {
        Ok(r) => r,
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".into());
            Err(Error::Internal(format!("converter crashed: {msg}")))
        }
    }
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

fn run_inner(job: &Job, progress: Progress<'_>, cancel: &AtomicBool) -> Result<PathBuf> {
    job.task.validate()?;
    check_cancel(cancel)?;
    let out = &job.output;
    match &job.task {
        Task::Convert {
            input,
            format: f,
            options,
        } => {
            let fmt = format(f)?;
            if disc::is_cda(input) && fmt.category == Category::Audio {
                let cda = disc::read_cda(input)?;
                let drive = disc::drive_of(input)?;
                return run_cd(&drive, cda.track, fmt, options, out, progress, cancel);
            }
            let ff = Ffmpeg::get()?;
            let info = probe::probe(input)?;
            let total = options.output_duration(info.duration_secs);
            atomic::write_with(out, |tmp| {
                let a = args::convert(ff, input, &info, tmp, fmt, options)?;
                run_ffmpeg(ff, &a, total, progress, cancel).map(|_| ())
            })
        }
        Task::ConvertImage {
            input,
            format: f,
            options,
        } => {
            let fmt = format(f)?;
            let r = image_conv::convert(input, out, fmt, options);
            progress(1.0);
            r
        }
        Task::Join {
            inputs,
            format: f,
            options,
        } => {
            let fmt = format(f)?;
            let ff = Ffmpeg::get()?;
            let infos = inputs
                .iter()
                .map(|p| probe::probe(p))
                .collect::<Result<Vec<_>>>()?;
            let total: f64 = infos.iter().filter_map(|i| i.duration_secs).sum();
            let pairs: Vec<_> = inputs
                .iter()
                .map(PathBuf::as_path)
                .zip(infos.iter())
                .collect();
            atomic::write_with(out, |tmp| {
                let a = args::join(ff, &pairs, tmp, fmt, options)?;
                run_ffmpeg(ff, &a, Some(total), progress, cancel).map(|_| ())
            })
        }
        Task::Mux {
            video,
            audio,
            format: f,
        } => {
            let fmt = format(f)?;
            let ff = Ffmpeg::get()?;
            let vi = probe::probe(video)?;
            let ai = probe::probe(audio)?;
            let total = match (vi.duration_secs, ai.duration_secs) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            atomic::write_with(out, |tmp| {
                let a = args::mux(ff, video, &vi, audio, &ai, tmp, fmt)?;
                run_ffmpeg(ff, &a, total, progress, cancel).map(|_| ())
            })
        }
        Task::Split {
            input,
            segment_secs,
        } => {
            let ff = Ffmpeg::get()?;
            let info = probe::probe(input)?;
            let ext = input
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_else(|| "mkv".into());
            let stem = input
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "part".into());
            atomic::write_dir_with(out, |dir| {
                let a = args::split(input, dir, &stem, &ext, *segment_secs);
                run_ffmpeg(ff, &a, info.duration_secs, progress, cancel).map(|_| ())
            })
        }
        Task::ScreenRecord {
            format: f,
            fps,
            max_secs,
            record_audio,
        } => {
            let fmt = format(f)?;
            if fmt.category != Category::Video {
                return Err(Error::WrongCategory {
                    format: f.clone(),
                    what: "screen recording",
                });
            }
            let ff = Ffmpeg::get()?;
            atomic::write_with(out, |tmp| {
                let a = args::screen_record(
                    ff,
                    std::env::consts::OS,
                    tmp,
                    fmt,
                    fps.unwrap_or(30),
                    *max_secs,
                    *record_audio,
                )?;
                // Stopping a recording is how it normally ends.
                match run_ffmpeg(ff, &a, *max_secs, progress, cancel)? {
                    Ended::Finished | Ended::Stopped => Ok(()),
                }
            })
        }
        Task::CdTrack {
            cda,
            drive,
            track,
            format: f,
            options,
        } => {
            let fmt = format(f)?;
            let (drive, track) = match (cda, drive, track) {
                (Some(c), _, _) => (disc::drive_of(c)?, disc::read_cda(c)?.track),
                (None, Some(d), Some(t)) => (d.clone(), *t),
                _ => return Err(Error::InvalidOption("missing drive or track".into())),
            };
            run_cd(&drive, track, fmt, options, out, progress, cancel)
        }
        Task::DvdToAudio {
            input,
            title,
            audio_track,
            format: f,
            options,
        } => {
            let fmt = format(f)?;
            let stream = audio_track.unwrap_or(0);
            run_dvd(input, *title, stream, fmt, options, out, progress, cancel)
        }
        Task::PdfMerge { inputs } => done(progress, pdf::merge(inputs, out)),
        Task::PdfSplit { input } => done(progress, pdf::split(input, out)),
        Task::PdfCompress { input } => done(progress, pdf::compress(input, out)),
        Task::ImagesToPdf { inputs } => done(progress, pdf::images_to_pdf(inputs, out)),
    }
}

fn run_cd(
    drive: &str,
    track: u8,
    fmt: &'static formats::Format,
    options: &crate::MediaOptions,
    out: &Path,
    progress: Progress<'_>,
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    let ff = Ffmpeg::get()?;
    let (reader, toc) = disc::open_drive(drive)?;
    rip_and_encode(
        ff, &reader, &toc, track, fmt, options, out, progress, cancel,
    )
}

/// Rips `track` from `src` into a scratch WAV, then encodes it into `out`.
/// Generic so tests can feed a synthetic disc.
#[allow(clippy::too_many_arguments)]
pub fn rip_and_encode<R: AudioSectorReader>(
    ff: &Ffmpeg,
    src: &R,
    toc: &Toc,
    track: u8,
    fmt: &'static formats::Format,
    options: &crate::MediaOptions,
    out: &Path,
    progress: Progress<'_>,
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    let scratch = atomic::ScratchDir::new()?;
    let wav = scratch.path().join(format!("track{track:02}.wav"));
    let rip_progress = |p: f32| progress(p * RIP_SHARE);
    let secs = disc::rip_track(src, toc, track, &wav, &rip_progress, cancel)?;
    let info = probe::probe(&wav)?;
    let total = options.output_duration(Some(secs));
    let enc_progress = |p: f32| progress(RIP_SHARE + (1.0 - RIP_SHARE) * p);
    atomic::write_with(out, |tmp| {
        let a = args::convert(ff, &wav, &info, tmp, fmt, options)?;
        run_ffmpeg(ff, &a, total, &enc_progress, cancel).map(|_| ())
    })
}

/// Turns a read failure into a message that mentions copy protection.
fn dvd_hint(e: Error) -> Error {
    match e {
        Error::Cancelled | Error::InvalidOption(_) | Error::FfmpegMissing => e,
        other => Error::Disc(format!(
            "could not read the DVD ({other}). Commercial DVDs protected by CSS cannot be \
             read: FormatCraft does not include DVD decryption"
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_dvd(
    input: &Path,
    title: Option<u32>,
    audio_stream: u32,
    fmt: &'static formats::Format,
    options: &crate::MediaOptions,
    out: &Path,
    progress: Progress<'_>,
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    let ff = Ffmpeg::get()?;
    let src = disc::locate_dvd(input)?;
    let titles = src
        .video_ts()
        .and_then(|v| disc::read_titles(v).ok())
        .unwrap_or_default();
    let sets = match src.video_ts() {
        Some(v) => disc::title_sets(v)?,
        None => Vec::new(),
    };
    let set = disc::choose_title_set(&sets, &titles, title);

    // Ways to open the DVD, best first.
    let mut attempts: Vec<(Vec<OsString>, OsString)> = Vec::new();
    if ff.has_demuxer("dvdvideo") {
        let mut a: Vec<OsString> = vec!["-f".into(), "dvdvideo".into()];
        if let Some(t) = title.or_else(|| set.and_then(|s| disc::title_in_set(&titles, s.vts))) {
            a.push("-title".into());
            a.push(t.to_string().into());
        }
        attempts.push((a, src.demuxer_input().as_os_str().to_owned()));
    }
    if let Some(set) = set {
        attempts.push((Vec::new(), disc::concat_url(set)));
    }
    if attempts.is_empty() {
        return Err(Error::Disc(
            "no DVD video files found (VIDEO_TS/VTS_xx_y.VOB); .iso images need an FFmpeg \
             build with DVD support"
                .into(),
        ));
    }
    let mut last = Error::Disc("the DVD has no audio".into());
    for (pre, url) in &attempts {
        check_cancel(cancel)?;
        let info = match probe::probe_input(pre, url) {
            Ok(i) if i.has_audio() => i,
            Ok(_) => continue,
            Err(e) => {
                last = e;
                continue;
            }
        };
        let input = args::Input {
            args: pre,
            url,
            audio_stream,
        };
        let total = options.output_duration(info.duration_secs);
        return atomic::write_with(out, |tmp| {
            let a = args::convert_input(ff, &input, &info, tmp, fmt, options)?;
            run_ffmpeg(ff, &a, total, progress, cancel).map(|_| ())
        })
        .map_err(dvd_hint);
    }
    Err(dvd_hint(last))
}

fn done(progress: Progress<'_>, r: Result<PathBuf>) -> Result<PathBuf> {
    if r.is_ok() {
        progress(1.0);
    }
    r
}
