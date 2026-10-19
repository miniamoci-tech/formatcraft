//! Running FFmpeg: progress, cancellation, bounded error capture.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::ffmpeg::{Ffmpeg, command};

/// Progress callback: fraction in 0.0..=1.0.
pub type Progress<'a> = &'a (dyn Fn(f32) + Sync);

const MAX_STDERR_LINES: usize = 20;
const MAX_LINE: usize = 2000;

/// Parses one `-progress` line; returns elapsed output seconds.
pub(crate) fn parse_progress_line(line: &str) -> Option<f64> {
    let (k, v) = line.trim().split_once('=')?;
    match k {
        // Both are microseconds (out_time_ms is misnamed in FFmpeg).
        "out_time_us" | "out_time_ms" => {
            let us: i64 = v.trim().parse().ok()?;
            (us >= 0).then(|| us as f64 / 1_000_000.0)
        }
        _ => None,
    }
}

/// How a run ended when it was not an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    Finished,
    /// Stopped gracefully on request (used by screen recording).
    Stopped,
}

/// Runs FFmpeg with `args`. `total_secs` drives the progress fraction.
/// When `cancel` becomes true FFmpeg is asked to quit (`q`), then killed.
pub fn run_ffmpeg(
    ff: &Ffmpeg,
    args: &[OsString],
    total_secs: Option<f64>,
    progress: Progress<'_>,
    cancel: &AtomicBool,
) -> Result<Ended> {
    let mut child = command(&ff.ffmpeg)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::ToolFailed {
            tool: "ffmpeg",
            code: None,
            message: e.to_string(),
        })?;

    let (tx, rx) = mpsc::channel::<f64>();
    let stdout = child.stdout.take();
    let out_thread = std::thread::spawn(move || {
        if let Some(out) = stdout {
            for line in BufReader::new(out).lines().map_while(std::io::Result::ok) {
                if let Some(t) = parse_progress_line(&line)
                    && tx.send(t).is_err()
                {
                    break;
                }
            }
        }
    });
    let stderr = child.stderr.take();
    let err_thread = std::thread::spawn(move || {
        let mut tail: VecDeque<String> = VecDeque::new();
        if let Some(err) = stderr {
            for line in BufReader::new(err).lines().map_while(std::io::Result::ok) {
                let line: String = line.chars().take(MAX_LINE).collect();
                if line.trim().is_empty() {
                    continue;
                }
                if tail.len() == MAX_STDERR_LINES {
                    tail.pop_front();
                }
                tail.push_back(line);
            }
        }
        tail.into_iter().collect::<Vec<_>>().join("\n")
    });

    let total = total_secs.filter(|t| t.is_finite() && *t > 0.0);
    let mut stop_requested: Option<Instant> = None;
    let status = loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(t) => {
                if let Some(total) = total {
                    progress(((t / total) as f32).clamp(0.0, 0.999));
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {}
        }
        if cancel.load(Ordering::Relaxed) {
            match stop_requested {
                None => {
                    stop_requested = Some(Instant::now());
                    if let Some(stdin) = child.stdin.as_mut() {
                        // Ignored: if the pipe is gone the kill below follows.
                        let _ = stdin.write_all(b"q\n").and_then(|_| stdin.flush());
                    }
                }
                Some(t) if t.elapsed() > Duration::from_secs(5) => {
                    let _ = child.kill();
                }
                Some(_) => {}
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                return Err(Error::IoBare(e));
            }
        }
    };
    let _ = out_thread.join();
    let stderr_text = err_thread.join().unwrap_or_default();

    if stop_requested.is_some() {
        return if status.success() {
            Ok(Ended::Stopped)
        } else {
            Err(Error::Cancelled)
        };
    }
    if status.success() {
        progress(1.0);
        Ok(Ended::Finished)
    } else {
        Err(Error::ToolFailed {
            tool: "ffmpeg",
            code: status.code(),
            message: if stderr_text.is_empty() {
                "no error output".into()
            } else {
                stderr_text
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_lines() {
        assert_eq!(parse_progress_line("out_time_us=1500000"), Some(1.5));
        assert_eq!(parse_progress_line("out_time_ms=2000000"), Some(2.0));
        assert_eq!(parse_progress_line("out_time_us=N/A"), None);
        assert_eq!(parse_progress_line("out_time_us=-5"), None);
        assert_eq!(parse_progress_line("progress=end"), None);
        assert_eq!(parse_progress_line(""), None);
    }
}
