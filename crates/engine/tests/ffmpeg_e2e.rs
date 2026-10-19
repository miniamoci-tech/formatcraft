#![allow(clippy::unwrap_used, clippy::panic)] // test helpers
//! End-to-end conversions through a real FFmpeg on synthetic inputs.
//! Skips (with a message) when FFmpeg is not installed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use formatcraft_engine::ffmpeg::Ffmpeg;
use formatcraft_engine::job::{Crop, Rotation};
use formatcraft_engine::{MediaOptions, Task, probe, run_job};

fn make_clip(dir: &Path, name: &str, secs: u32, size: &str, audio: bool) -> PathBuf {
    let ff = Ffmpeg::get().unwrap();
    let out = dir.join(name);
    let mut c = Command::new(&ff.ffmpeg);
    c.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
    ])
    .arg(format!("testsrc=duration={secs}:size={size}:rate=25"));
    if audio {
        c.args(["-f", "lavfi", "-i"])
            .arg(format!("sine=frequency=440:duration={secs}"));
    }
    c.args(["-c:v", "mpeg4", "-q:v", "5"]);
    if audio {
        c.args(["-c:a", "aac"]);
    }
    let st = c.arg(&out).status().unwrap();
    assert!(st.success());
    out
}

fn run(task: Task, dir: &Path) -> PathBuf {
    let job = task.into_job(dir).unwrap();
    let cancel = AtomicBool::new(false);
    let last = std::sync::Mutex::new(0.0f32);
    let p = |v: f32| *last.lock().unwrap() = v;
    let out = run_job(&job, &p, &cancel).unwrap_or_else(|e| panic!("{}: {e}", job.task.kind()));
    assert_eq!(*last.lock().unwrap(), 1.0);
    out
}

fn have_ffmpeg() -> bool {
    if Ffmpeg::get().is_err() {
        eprintln!("FFmpeg not found; skipping end-to-end tests");
        return false;
    }
    true
}

#[test]
fn video_audio_pipeline() {
    if !have_ffmpeg() {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let src = make_clip(d.path(), "src.avi", 3, "320x240", true);

    for fmt in [
        "mp4", "mkv", "webm", "mov", "avi", "flv", "mpg", "wmv", "ts", "gif",
    ] {
        let opts = MediaOptions {
            start: Some(0.5),
            end: Some(2.5),
            width: Some(161),
            ..Default::default()
        };
        let out = run(
            Task::Convert {
                input: src.clone(),
                format: fmt.into(),
                options: opts,
            },
            d.path(),
        );
        let info = probe::probe(&out).unwrap();
        let v = info.streams.iter().find(|s| s.kind == "video").unwrap();
        assert!(
            v.width == Some(160) || v.width == Some(161),
            "{fmt}: {:?}",
            v.width
        );
        let dur = info.duration_secs.unwrap_or(2.0);
        assert!((1.5..=2.6).contains(&dur), "{fmt}: duration {dur}");
    }

    for fmt in [
        "mp3", "aac", "m4a", "wav", "flac", "ogg", "opus", "wma", "aiff", "ac3", "mp2",
    ] {
        let out = run(
            Task::Convert {
                input: src.clone(),
                format: fmt.into(),
                options: MediaOptions::default(),
            },
            d.path(),
        );
        let info = probe::probe(&out).unwrap();
        assert!(info.has_audio() && !info.has_video(), "{fmt}");
    }

    // Rotate + crop + speed + watermark.
    let wm = d.path().join("wm.png");
    image::RgbaImage::from_pixel(32, 32, image::Rgba([255, 0, 0, 255]))
        .save(&wm)
        .unwrap();
    let out = run(
        Task::Convert {
            input: src.clone(),
            format: "mp4".into(),
            options: MediaOptions {
                rotate: Rotation::Cw90,
                crop: Some(Crop {
                    x: 10,
                    y: 10,
                    width: 201,
                    height: 101,
                }),
                speed: Some(2.0),
                watermark: Some(formatcraft_engine::job::Watermark {
                    image: wm,
                    corner: Default::default(),
                    opacity: 0.5,
                }),
                ..Default::default()
            },
        },
        d.path(),
    );
    let info = probe::probe(&out).unwrap();
    let v = info.streams.iter().find(|s| s.kind == "video").unwrap();
    assert_eq!((v.width, v.height), (Some(100), Some(200)));
    assert!(info.duration_secs.unwrap() < 2.0);
}

#[test]
fn join_mux_split() {
    if !have_ffmpeg() {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let a = make_clip(d.path(), "a.avi", 2, "320x240", true);
    let b = make_clip(d.path(), "b.avi", 2, "640x360", false);

    let joined = run(
        Task::Join {
            inputs: vec![a.clone(), b.clone()],
            format: "mp4".into(),
            options: MediaOptions::default(),
        },
        d.path(),
    );
    let info = probe::probe(&joined).unwrap();
    assert!((3.5..=4.5).contains(&info.duration_secs.unwrap()));
    assert!(info.has_audio());

    let audio_join = run(
        Task::Join {
            inputs: vec![a.clone(), a.clone()],
            format: "mp3".into(),
            options: MediaOptions::default(),
        },
        d.path(),
    );
    assert!(probe::probe(&audio_join).unwrap().duration_secs.unwrap() > 3.5);

    let muxed = run(
        Task::Mux {
            video: b.clone(),
            audio: a.clone(),
            format: "mkv".into(),
        },
        d.path(),
    );
    let info = probe::probe(&muxed).unwrap();
    assert!(info.has_audio() && info.has_video());

    let long = make_clip(d.path(), "long.avi", 4, "160x120", true);
    let parts = run(
        Task::Split {
            input: long,
            segment_secs: 1.0,
        },
        d.path(),
    );
    let n = std::fs::read_dir(&parts).unwrap().count();
    assert!(n >= 3, "got {n} parts");
}

#[test]
fn cancel_leaves_no_output() {
    if !have_ffmpeg() {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let src = make_clip(d.path(), "src.avi", 2, "320x240", true);
    let job = Task::Convert {
        input: src,
        format: "mp4".into(),
        options: MediaOptions::default(),
    }
    .into_job(d.path())
    .unwrap();
    let cancel = AtomicBool::new(true);
    let r = run_job(&job, &|_| {}, &cancel);
    assert!(r.is_err());
    assert!(!job.output.exists());
    let leftovers: Vec<_> = std::fs::read_dir(d.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains("fctmp"))
        .collect();
    assert!(leftovers.is_empty());
}
