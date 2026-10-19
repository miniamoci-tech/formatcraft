//! Runs one [`Job`] to completion.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use crate::error::{Error, Result};
use crate::ffmpeg::Ffmpeg;
use crate::formats::{self, Category};
use crate::job::{Job, Task};
use crate::runner::{Ended, Progress, run_ffmpeg};
use crate::{args, atomic, image_conv, pdf, probe};

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
        Task::PdfMerge { inputs } => done(progress, pdf::merge(inputs, out)),
        Task::PdfSplit { input } => done(progress, pdf::split(input, out)),
        Task::PdfCompress { input } => done(progress, pdf::compress(input, out)),
        Task::ImagesToPdf { inputs } => done(progress, pdf::images_to_pdf(inputs, out)),
    }
}

fn done(progress: Progress<'_>, r: Result<PathBuf>) -> Result<PathBuf> {
    if r.is_ok() {
        progress(1.0);
    }
    r
}
