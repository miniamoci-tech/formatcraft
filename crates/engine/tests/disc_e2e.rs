#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)] // test helpers
//! Audio CD and DVD extraction end to end, on synthetic discs: an in-memory
//! CD (sine tones) and a VIDEO_TS folder whose VOBs are made by FFmpeg.
//! Skips when FFmpeg is not installed. No physical drive is needed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use cd_da_reader::{AudioSectorReader, Toc, Track};
use formatcraft_engine::disc::SECTOR_BYTES;
use formatcraft_engine::exec::rip_and_encode;
use formatcraft_engine::ffmpeg::Ffmpeg;
use formatcraft_engine::formats::find;
use formatcraft_engine::{Error, MediaOptions, Task, probe, run_job};

fn ffmpeg() -> Option<&'static Ffmpeg> {
    let f = Ffmpeg::get().ok();
    if f.is_none() {
        eprintln!("FFmpeg not found; skipping disc tests");
    }
    f
}

/// A CD whose every sector holds a 440 Hz (left) / 660 Hz (right) tone.
struct ToneDisc {
    sectors: u32,
}

impl AudioSectorReader for ToneDisc {
    type Error = std::io::Error;
    fn read_audio_sectors(&self, start: u32, count: u32) -> std::io::Result<Vec<u8>> {
        if start.saturating_add(count) > self.sectors {
            return Err(std::io::Error::other("beyond lead-out"));
        }
        let frames_per_sector = SECTOR_BYTES / 4;
        let mut v = Vec::with_capacity((count as u64 * SECTOR_BYTES) as usize);
        let first = u64::from(start) * frames_per_sector;
        for n in first..first + u64::from(count) * frames_per_sector {
            let t = n as f64 / 44_100.0;
            let l = ((t * 440.0 * std::f64::consts::TAU).sin() * 8000.0) as i16;
            let r = ((t * 660.0 * std::f64::consts::TAU).sin() * 8000.0) as i16;
            v.extend_from_slice(&l.to_le_bytes());
            v.extend_from_slice(&r.to_le_bytes());
        }
        Ok(v)
    }
}

fn tone_toc() -> Toc {
    Toc {
        first_track: 1,
        last_track: 2,
        tracks: vec![
            Track {
                number: 1,
                start_lba: 0,
                start_msf: (0, 2, 0),
                is_audio: true,
            },
            Track {
                number: 2,
                start_lba: 225,
                start_msf: (0, 5, 0),
                is_audio: true,
            },
        ],
        leadout_lba: 375,
    }
}

#[test]
fn audio_cd_track_to_mp3_flac_wav() {
    let Some(ff) = ffmpeg() else { return };
    let d = tempfile::tempdir().unwrap();
    let disc = ToneDisc { sectors: 375 };
    for (fmt, track, secs) in [
        ("mp3", 1u8, 3.0),
        ("flac", 2, 2.0),
        ("wav", 2, 2.0),
        ("m4a", 1, 3.0),
    ] {
        let out = d
            .path()
            .join(format!("Track {track:02}.{}", find(fmt).unwrap().ext));
        let last = std::sync::Mutex::new(0.0f32);
        let got = rip_and_encode(
            ff,
            &disc,
            &tone_toc(),
            track,
            find(fmt).unwrap(),
            &MediaOptions::default(),
            &out,
            &|p| *last.lock().unwrap() = p,
            &AtomicBool::new(false),
        )
        .unwrap_or_else(|e| panic!("{fmt}: {e}"));
        let info = probe::probe(&got).unwrap();
        let a = info.streams.iter().find(|s| s.kind == "audio").unwrap();
        assert_eq!(a.channels, Some(2), "{fmt}");
        let dur = info.duration_secs.unwrap();
        assert!((dur - secs).abs() < 0.15, "{fmt}: {dur}");
        assert_eq!(*last.lock().unwrap(), 1.0, "{fmt}");
    }
}

#[test]
fn audio_cd_cancel_and_bad_track_leave_nothing() {
    let Some(ff) = ffmpeg() else { return };
    let d = tempfile::tempdir().unwrap();
    let out = d.path().join("x.mp3");
    let disc = ToneDisc { sectors: 375 };
    let mp3 = find("mp3").unwrap();
    let o = MediaOptions::default();
    let r = rip_and_encode(
        ff,
        &disc,
        &tone_toc(),
        1,
        mp3,
        &o,
        &out,
        &|_| {},
        &AtomicBool::new(true),
    );
    assert!(matches!(r, Err(Error::Cancelled)));
    let r = rip_and_encode(
        ff,
        &disc,
        &tone_toc(),
        7,
        mp3,
        &o,
        &out,
        &|_| {},
        &AtomicBool::new(false),
    );
    assert!(r.is_err());
    // Scratched disc: reads fail half way.
    let bad = ToneDisc { sectors: 300 };
    let r = rip_and_encode(
        ff,
        &bad,
        &tone_toc(),
        2,
        mp3,
        &o,
        &out,
        &|_| {},
        &AtomicBool::new(false),
    );
    assert!(r.is_err());
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 0);
}

#[test]
fn cda_task_on_non_windows_path_is_a_clear_error() {
    let d = tempfile::tempdir().unwrap();
    let cda = d.path().join("Track01.cda");
    let mut b = Vec::new();
    for part in [
        &b"RIFF"[..],
        &36u32.to_le_bytes(),
        b"CDDA",
        b"fmt ",
        &24u32.to_le_bytes(),
        &1u16.to_le_bytes(),
        &1u16.to_le_bytes(),
        &[0u8; 20],
    ] {
        b.extend_from_slice(part);
    }
    std::fs::write(&cda, b).unwrap();
    let job = Task::CdTrack {
        cda: Some(cda),
        drive: None,
        track: None,
        format: "mp3".into(),
        options: Default::default(),
    }
    .into_job(d.path())
    .unwrap();
    // The temp folder is on a hard disk, not an audio CD: opening the "drive"
    // must fail with an error, never a panic.
    let r = run_job(&job, &|_| {}, &AtomicBool::new(false));
    assert!(r.is_err());
    assert!(!job.output.exists());
}

fn make_vob(ff: &Ffmpeg, path: &Path, secs: u32, freq: u32) {
    let st = Command::new(&ff.ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
        ])
        .arg(format!("testsrc=duration={secs}:size=720x576:rate=25"))
        .args(["-f", "lavfi", "-i"])
        .arg(format!(
            "sine=frequency={freq}:duration={secs}:sample_rate=48000"
        ))
        .args([
            "-c:v",
            "mpeg2video",
            "-b:v",
            "1M",
            "-c:a",
            "ac3",
            "-b:a",
            "192k",
            "-f",
            "dvd",
        ])
        .arg(path)
        .status()
        .unwrap();
    assert!(st.success());
}

/// VIDEO_TS.IFO whose title table maps title 1 → VTS 2 and title 2 → VTS 1.
fn vmg_ifo() -> Vec<u8> {
    let mut b = vec![0u8; 4096];
    b[0..12].copy_from_slice(b"DVDVIDEO-VMG");
    b[0xC4..0xC8].copy_from_slice(&1u32.to_be_bytes());
    b[2048..2050].copy_from_slice(&2u16.to_be_bytes());
    b[2048 + 8 + 6] = 2;
    b[2048 + 20 + 6] = 1;
    b
}

fn make_dvd(ff: &Ffmpeg, root: &Path) -> PathBuf {
    let vts = root.join("VIDEO_TS");
    std::fs::create_dir_all(&vts).unwrap();
    make_vob(ff, &vts.join("VTS_01_1.VOB"), 3, 440);
    make_vob(ff, &vts.join("VTS_01_2.VOB"), 3, 440);
    make_vob(ff, &vts.join("VTS_02_1.VOB"), 1, 880);
    std::fs::write(vts.join("VIDEO_TS.IFO"), vmg_ifo()).unwrap();
    vts
}

#[test]
fn dvd_main_title_and_chosen_title_to_audio() {
    let Some(ff) = ffmpeg() else { return };
    let d = tempfile::tempdir().unwrap();
    let disc_root = d.path().join("MY_DVD");
    let vts = make_dvd(ff, &disc_root);
    let out_dir = d.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();

    // Default: the largest title set (two VOBs, ~6 s), from the disc root.
    let job = Task::DvdToAudio {
        input: disc_root.clone(),
        title: None,
        audio_track: None,
        format: "mp3".into(),
        options: Default::default(),
    }
    .into_job(&out_dir)
    .unwrap();
    assert!(
        job.output.ends_with("MY_DVD.mp3"),
        "{}",
        job.output.display()
    );
    let out = run_job(&job, &|_| {}, &AtomicBool::new(false)).unwrap_or_else(|e| panic!("{e}"));
    let dur = probe::probe(&out).unwrap().duration_secs.unwrap();
    assert!((5.5..=6.6).contains(&dur), "main title: {dur}");

    // Title 1 lives in VTS 2 (~1 s); pick it from a file inside VIDEO_TS.
    let job = Task::DvdToAudio {
        input: vts.join("VTS_01_1.VOB"),
        title: Some(1),
        audio_track: None,
        format: "flac".into(),
        options: Default::default(),
    }
    .into_job(&out_dir)
    .unwrap();
    let out = run_job(&job, &|_| {}, &AtomicBool::new(false)).unwrap_or_else(|e| panic!("{e}"));
    let dur = probe::probe(&out).unwrap().duration_secs.unwrap();
    assert!((0.7..=1.4).contains(&dur), "title 1: {dur}");

    // A missing audio track is reported, not crashed on.
    let job = Task::DvdToAudio {
        input: vts.clone(),
        title: None,
        audio_track: Some(3),
        format: "mp3".into(),
        options: Default::default(),
    }
    .into_job(&out_dir)
    .unwrap();
    let e = run_job(&job, &|_| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(e.to_string().contains("audio track"), "{e}");

    // Not a DVD.
    let job = Task::DvdToAudio {
        input: out_dir.clone(),
        title: None,
        audio_track: None,
        format: "mp3".into(),
        options: Default::default(),
    }
    .into_job(&out_dir);
    let e = job
        .and_then(|j| run_job(&j, &|_| {}, &AtomicBool::new(false)))
        .unwrap_err();
    assert!(e.to_string().contains("not a DVD"), "{e}");
}
