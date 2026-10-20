//! Optical discs: audio CD tracks and DVD-Video.
//!
//! **Audio CD.** On Windows each audio track appears as a 44-byte `TrackNN.cda`
//! shortcut; the audio itself is only on the disc. We read the track number
//! from the shortcut, then rip the raw CD-DA sectors through `cd-da-reader`
//! (MIT) and encode the resulting WAV with FFmpeg. On macOS the system already
//! exposes tracks as AIFF files, which convert like any other audio file.
//!
//! **DVD-Video.** The audio of the main title is extracted with FFmpeg's
//! `dvdvideo` demuxer when the FFmpeg build has it, otherwise by concatenating
//! the title set's VOB files. Copy-protected (CSS) discs cannot be decrypted:
//! libdvdcss is not bundled.

use std::io::{BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::atomic::{AtomicBool, Ordering};

use cd_da_reader::{AudioSectorReader, CdReader, Toc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result, io_at};
use crate::runner::Progress;

/// Bytes of PCM in one CD-DA sector (1/75 s of 44.1 kHz 16-bit stereo).
pub const SECTOR_BYTES: u64 = 2352;
pub const SECTORS_PER_SEC: f64 = 75.0;
const CDA_MAX_BYTES: u64 = 4096;
const DVD_SECTOR: usize = 2048;
const MAX_TITLES: usize = 99;

fn disc_err(e: impl std::fmt::Display) -> Error {
    Error::Disc(e.to_string())
}

// ------------------------------------------------------------------ CDA

/// What a `.cda` shortcut says about its track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CdaFile {
    pub track: u8,
    pub start_sector: u32,
    pub length_sectors: u32,
}

impl CdaFile {
    pub fn seconds(&self) -> f64 {
        f64::from(self.length_sectors) / SECTORS_PER_SEC
    }
}

fn le_u16(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([*s.first()?, *s.get(1)?]))
}

fn le_u32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([
        *s.first()?,
        *s.get(1)?,
        *s.get(2)?,
        *s.get(3)?,
    ]))
}

fn be_u16(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([*s.first()?, *s.get(1)?]))
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([
        *s.first()?,
        *s.get(1)?,
        *s.get(2)?,
        *s.get(3)?,
    ]))
}

/// Parses the Windows CD audio shortcut format: a RIFF file of type `CDDA`
/// whose `fmt ` chunk holds the track number, start and length in sectors.
pub fn parse_cda(b: &[u8]) -> Result<CdaFile> {
    let bad = || Error::Disc("not a CD audio track (.cda) file".into());
    if b.get(0..4) != Some(b"RIFF")
        || b.get(8..12) != Some(b"CDDA")
        || b.get(12..16) != Some(b"fmt ")
    {
        return Err(bad());
    }
    let track = le_u16(b, 22).ok_or_else(bad)?;
    let track = u8::try_from(track)
        .ok()
        .filter(|t| (1..=99).contains(t))
        .ok_or_else(|| Error::Disc(format!("invalid track number {track} in .cda file")))?;
    Ok(CdaFile {
        track,
        start_sector: le_u32(b, 28).ok_or_else(bad)?,
        length_sectors: le_u32(b, 32).ok_or_else(bad)?,
    })
}

pub fn read_cda(path: &Path) -> Result<CdaFile> {
    let f = std::fs::File::open(path).map_err(io_at(path))?;
    let mut buf = Vec::new();
    f.take(CDA_MAX_BYTES)
        .read_to_end(&mut buf)
        .map_err(io_at(path))?;
    parse_cda(&buf)
}

pub fn is_cda(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cda"))
}

/// The raw device path of the drive a `.cda` file lives on (`\\.\D:`).
pub fn drive_of(path: &Path) -> Result<String> {
    for c in path.components() {
        if let Component::Prefix(p) = c {
            match p.kind() {
                Prefix::Disk(l) | Prefix::VerbatimDisk(l) => {
                    return Ok(format!(r"\\.\{}:", char::from(l).to_ascii_uppercase()));
                }
                _ => break,
            }
        }
    }
    Err(Error::Disc(
        ".cda files only exist on Windows audio CDs; open the track from the CD drive \
         (e.g. D:\\Track01.cda)"
            .into(),
    ))
}

/// Accepts only optical-drive device paths, so a tool argument can never
/// point the raw reader at a hard disk.
pub fn check_drive_path(drive: &str) -> Result<()> {
    let ok = if let Some(rest) = drive.strip_prefix(r"\\.\") {
        let b = rest.as_bytes();
        b.len() == 2 && b.first().is_some_and(u8::is_ascii_alphabetic) && b.get(1) == Some(&b':')
    } else if let Some(n) = drive.strip_prefix("/dev/sr") {
        !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())
    } else if let Some(n) = drive.strip_prefix("disk") {
        !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())
    } else {
        drive == "/dev/cdrom"
    };
    if ok {
        Ok(())
    } else {
        Err(Error::InvalidOption(format!(
            "`{drive}` is not a CD drive; use a path like \\\\.\\D: (Windows), disk4 (macOS) or /dev/sr0 (Linux)"
        )))
    }
}

// ------------------------------------------------------------- CD drive

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CdTrackInfo {
    pub number: u8,
    pub seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CdDisc {
    pub drive: String,
    pub tracks: Vec<CdTrackInfo>,
}

/// Sectors between the last audio track and the data session of a CD-Extra.
const CD_EXTRA_GAP: u32 = 11_400;

/// Audio tracks of a TOC with their lengths (next start or lead-out; an
/// audio track followed by a data track loses the CD-Extra session gap).
pub fn track_infos(toc: &Toc) -> Vec<CdTrackInfo> {
    let mut out = Vec::new();
    for (i, t) in toc.tracks.iter().enumerate() {
        if !t.is_audio {
            continue;
        }
        let end = match toc.tracks.get(i + 1) {
            Some(n) if !n.is_audio => n.start_lba.saturating_sub(CD_EXTRA_GAP),
            Some(n) => n.start_lba,
            None => toc.leadout_lba,
        };
        let sectors = end.saturating_sub(t.start_lba);
        out.push(CdTrackInfo {
            number: t.number,
            seconds: f64::from(sectors) / SECTORS_PER_SEC,
        });
    }
    out
}

/// Every drive that currently holds an audio CD, with its tracks.
pub fn list_audio_cds() -> Result<Vec<CdDisc>> {
    let drives = CdReader::list_drives().map_err(disc_err)?;
    let mut out = Vec::new();
    for d in drives.into_iter().filter(|d| d.has_audio_cd) {
        let Ok(reader) = CdReader::open(&d) else {
            continue;
        };
        if let Ok(toc) = reader.read_toc() {
            out.push(CdDisc {
                drive: d.path.clone(),
                tracks: track_infos(&toc),
            });
        }
    }
    Ok(out)
}

/// 44-byte header of a 44.1 kHz 16-bit stereo PCM WAV with `data_len` bytes.
pub fn wav_header(data_len: u32) -> [u8; 44] {
    let mut h = [0u8; 44];
    let mut put = |at: usize, bytes: &[u8]| {
        if let Some(dst) = h.get_mut(at..at + bytes.len()) {
            dst.copy_from_slice(bytes);
        }
    };
    put(0, b"RIFF");
    put(4, &data_len.saturating_add(36).to_le_bytes());
    put(8, b"WAVE");
    put(12, b"fmt ");
    put(16, &16u32.to_le_bytes());
    put(20, &1u16.to_le_bytes());
    put(22, &2u16.to_le_bytes());
    put(24, &44_100u32.to_le_bytes());
    put(28, &176_400u32.to_le_bytes());
    put(32, &4u16.to_le_bytes());
    put(34, &16u16.to_le_bytes());
    put(36, b"data");
    put(40, &data_len.to_le_bytes());
    h
}

/// Streams one track from `src` into a WAV file. Returns the track length in
/// seconds. Generic over the sector source so tests can use an in-memory disc.
pub fn rip_track<R: AudioSectorReader>(
    src: &R,
    toc: &Toc,
    track: u8,
    wav: &Path,
    progress: Progress<'_>,
    cancel: &AtomicBool,
) -> Result<f64> {
    match toc.tracks.iter().find(|t| t.number == track) {
        None => return Err(Error::Disc(format!("track {track} is not on this CD"))),
        Some(t) if !t.is_audio => {
            return Err(Error::Disc(format!(
                "track {track} is a data track, not audio"
            )));
        }
        Some(_) => {}
    }
    let mut stream = cd_da_reader::open_track_stream(src, toc, track).map_err(disc_err)?;
    let total = stream.total_sectors();
    let data_len = u64::from(total)
        .checked_mul(SECTOR_BYTES)
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| n.checked_add(36).is_some())
        .ok_or_else(|| Error::Disc("track too long for a WAV file".into()))?;
    let file = std::fs::File::create(wav).map_err(io_at(wav))?;
    let mut w = BufWriter::new(file);
    w.write_all(&wav_header(data_len)).map_err(io_at(wav))?;
    let mut written: u64 = 0;
    while let Some(chunk) = stream.next_chunk().map_err(disc_err)? {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        w.write_all(&chunk).map_err(io_at(wav))?;
        written = written.saturating_add(chunk.len() as u64);
        if data_len > 0 {
            progress((written as f64 / f64::from(data_len)) as f32);
        }
    }
    w.flush().map_err(io_at(wav))?;
    if written != u64::from(data_len) {
        return Err(Error::Disc(format!(
            "short read from the CD: got {written} of {data_len} bytes"
        )));
    }
    Ok(f64::from(total) / SECTORS_PER_SEC)
}

/// Opens the physical drive at `drive` and reads its table of contents.
pub fn open_drive(drive: &str) -> Result<(CdReader, Toc)> {
    check_drive_path(drive)?;
    let reader = CdReader::open_path(drive).map_err(|e| {
        Error::Disc(format!(
            "cannot open the CD drive {drive}: {e}. Check that an audio CD is inserted"
        ))
    })?;
    let toc = reader
        .read_toc()
        .map_err(|e| Error::Disc(format!("cannot read the CD table of contents: {e}")))?;
    Ok((reader, toc))
}

// ------------------------------------------------------------------ DVD

/// Where a DVD's files are.
#[derive(Debug, Clone, PartialEq)]
pub enum DvdSource {
    /// A disc or folder: `root` contains `video_ts`.
    Folder { root: PathBuf, video_ts: PathBuf },
    /// A disc image.
    Iso(PathBuf),
}

impl DvdSource {
    /// What FFmpeg's `dvdvideo` demuxer opens.
    pub fn demuxer_input(&self) -> &Path {
        match self {
            DvdSource::Folder { root, .. } => root,
            DvdSource::Iso(p) => p,
        }
    }

    pub fn video_ts(&self) -> Option<&Path> {
        match self {
            DvdSource::Folder { video_ts, .. } => Some(video_ts),
            DvdSource::Iso(_) => None,
        }
    }
}

fn child_named(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
        })
}

fn is_named(p: &Path, name: &str) -> bool {
    p.file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
}

/// Accepts a drive root (`D:\`), a folder containing `VIDEO_TS`, the
/// `VIDEO_TS` folder itself, any file inside it, or an `.iso` image.
pub fn locate_dvd(input: &Path) -> Result<DvdSource> {
    if input.is_file() {
        if input
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("iso"))
        {
            return Ok(DvdSource::Iso(input.to_path_buf()));
        }
        if let Some(dir) = input.parent().filter(|d| is_named(d, "VIDEO_TS")) {
            let root = dir.parent().unwrap_or(dir).to_path_buf();
            return Ok(DvdSource::Folder {
                root,
                video_ts: dir.to_path_buf(),
            });
        }
    } else if input.is_dir() {
        if is_named(input, "VIDEO_TS") {
            let root = input.parent().unwrap_or(input).to_path_buf();
            return Ok(DvdSource::Folder {
                root,
                video_ts: input.to_path_buf(),
            });
        }
        if let Some(vts) = child_named(input, "VIDEO_TS").filter(|p| p.is_dir()) {
            return Ok(DvdSource::Folder {
                root: input.to_path_buf(),
                video_ts: vts,
            });
        }
    } else {
        return Err(Error::MissingInput(input.to_path_buf()));
    }
    Err(Error::Disc(format!(
        "{} is not a DVD: choose the DVD drive, its VIDEO_TS folder or an .iso image",
        input.display()
    )))
}

/// Mounted DVD-Video discs: drive roots (Windows) or volumes (macOS/Linux)
/// that contain a `VIDEO_TS` folder.
pub fn find_dvd_volumes() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        roots.extend((b'D'..=b'Z').map(|l| PathBuf::from(format!("{}:\\", char::from(l)))));
    } else {
        let mut bases = vec![PathBuf::from("/Volumes"), PathBuf::from("/media")];
        if let Ok(user) = std::env::var("USER") {
            bases.push(PathBuf::from("/media").join(&user));
            bases.push(PathBuf::from("/run/media").join(&user));
        }
        for b in bases {
            if let Ok(rd) = std::fs::read_dir(&b) {
                roots.extend(rd.flatten().map(|e| e.path()).take(64));
            }
        }
    }
    roots
        .into_iter()
        .filter(|r| child_named(r, "VIDEO_TS").is_some_and(|p| p.is_dir()))
        .collect()
}

/// The VOB files of one title set (`VTS_NN_1.VOB`, `VTS_NN_2.VOB`, …).
#[derive(Debug, Clone, PartialEq)]
pub struct TitleSet {
    pub vts: u32,
    pub vobs: Vec<PathBuf>,
    pub bytes: u64,
}

/// `VTS_NN_M.VOB` → (NN, M).
fn vob_numbers(name: &str) -> Option<(u32, u32)> {
    let upper = name.to_ascii_uppercase();
    let rest = upper.strip_prefix("VTS_")?.strip_suffix(".VOB")?;
    let (nn, m) = rest.split_once('_')?;
    Some((nn.parse().ok()?, m.parse().ok()?))
}

/// Title sets with their content VOBs (menus, `_0.VOB`, excluded), by number.
pub fn title_sets(video_ts: &Path) -> Result<Vec<TitleSet>> {
    let mut parts: Vec<(u32, u32, PathBuf, u64)> = Vec::new();
    for e in std::fs::read_dir(video_ts)
        .map_err(io_at(video_ts))?
        .flatten()
        .take(10_000)
    {
        let p = e.path();
        let Some((vts, part)) = p
            .file_name()
            .and_then(|n| vob_numbers(&n.to_string_lossy()))
        else {
            continue;
        };
        if part == 0 {
            continue;
        }
        let len = e.metadata().map(|m| m.len()).unwrap_or(0);
        parts.push((vts, part, p, len));
    }
    parts.sort_by_key(|(v, m, _, _)| (*v, *m));
    let mut sets: Vec<TitleSet> = Vec::new();
    for (vts, _, path, len) in parts {
        match sets.last_mut() {
            Some(s) if s.vts == vts => {
                s.vobs.push(path);
                s.bytes = s.bytes.saturating_add(len);
            }
            _ => sets.push(TitleSet {
                vts,
                vobs: vec![path],
                bytes: len,
            }),
        }
    }
    Ok(sets)
}

/// A title from the disc's title search pointer table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DvdTitle {
    pub number: u32,
    pub vts: u32,
    pub chapters: u32,
}

/// Parses `VIDEO_TS.IFO`: the `TT_SRPT` table lists every title and the
/// title set holding it (DVD-Video VMGI layout).
pub fn parse_vmg_titles(ifo: &[u8]) -> Result<Vec<DvdTitle>> {
    let bad = || Error::Disc("VIDEO_TS.IFO is damaged or not a DVD-Video IFO".into());
    if ifo.get(0..12) != Some(b"DVDVIDEO-VMG") {
        return Err(bad());
    }
    let sector = be_u32(ifo, 0xC4).ok_or_else(bad)?;
    let base = usize::try_from(sector)
        .ok()
        .and_then(|s| s.checked_mul(DVD_SECTOR))
        .ok_or_else(bad)?;
    let count = usize::from(be_u16(ifo, base).ok_or_else(bad)?).min(MAX_TITLES);
    let mut titles = Vec::with_capacity(count);
    for i in 0..count {
        let at = base
            .checked_add(8)
            .and_then(|b| b.checked_add(i.checked_mul(12)?))
            .ok_or_else(bad)?;
        let chapters = be_u16(ifo, at.saturating_add(2)).ok_or_else(bad)?;
        let vts = *ifo.get(at.saturating_add(6)).ok_or_else(bad)?;
        if vts == 0 {
            return Err(bad());
        }
        titles.push(DvdTitle {
            number: u32::try_from(i).unwrap_or(u32::MAX).saturating_add(1),
            vts: u32::from(vts),
            chapters: u32::from(chapters),
        });
    }
    Ok(titles)
}

/// Reads and parses `VIDEO_TS.IFO` (capped at 1 MiB).
pub fn read_titles(video_ts: &Path) -> Result<Vec<DvdTitle>> {
    let ifo = child_named(video_ts, "VIDEO_TS.IFO")
        .ok_or_else(|| Error::Disc("VIDEO_TS.IFO not found".into()))?;
    let f = std::fs::File::open(&ifo).map_err(io_at(&ifo))?;
    let mut buf = Vec::new();
    f.take(1 << 20).read_to_end(&mut buf).map_err(io_at(&ifo))?;
    parse_vmg_titles(&buf)
}

/// The title set to use: the one holding `title` if given, else the largest
/// (the main feature on almost every disc).
pub fn choose_title_set<'a>(
    sets: &'a [TitleSet],
    titles: &[DvdTitle],
    title: Option<u32>,
) -> Option<&'a TitleSet> {
    if let Some(n) = title
        && let Some(t) = titles.iter().find(|t| t.number == n)
    {
        return sets.iter().find(|s| s.vts == t.vts);
    }
    sets.iter().max_by_key(|s| s.bytes)
}

/// The first title stored in `vts` (the title to ask `dvdvideo` for).
pub fn title_in_set(titles: &[DvdTitle], vts: u32) -> Option<u32> {
    titles.iter().find(|t| t.vts == vts).map(|t| t.number)
}

/// FFmpeg `concat:` URL joining the VOBs of a title set.
pub fn concat_url(set: &TitleSet) -> std::ffi::OsString {
    let mut s = std::ffi::OsString::from("concat:");
    for (i, v) in set.vobs.iter().enumerate() {
        if i > 0 {
            s.push("|");
        }
        s.push(v.as_os_str());
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use cd_da_reader::Track;

    pub(crate) fn cda_bytes(track: u16, start: u32, len: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&36u32.to_le_bytes());
        b.extend_from_slice(b"CDDA");
        b.extend_from_slice(b"fmt ");
        b.extend_from_slice(&24u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&track.to_le_bytes());
        b.extend_from_slice(&0x1234_5678u32.to_le_bytes());
        b.extend_from_slice(&start.to_le_bytes());
        b.extend_from_slice(&len.to_le_bytes());
        b.extend_from_slice(&[0u8; 8]);
        b
    }

    #[test]
    fn parses_cda_and_rejects_garbage() {
        let c = parse_cda(&cda_bytes(3, 15_000, 750)).unwrap();
        assert_eq!(c.track, 3);
        assert_eq!(c.length_sectors, 750);
        assert_eq!(c.seconds(), 10.0);
        assert!(parse_cda(&cda_bytes(0, 0, 0)).is_err());
        assert!(parse_cda(&cda_bytes(300, 0, 0)).is_err());
        assert!(parse_cda(b"RIFF").is_err());
        assert!(parse_cda(&[]).is_err());
        let mut truncated = cda_bytes(1, 0, 0);
        truncated.truncate(30);
        assert!(parse_cda(&truncated).is_err());
    }

    #[test]
    fn drive_paths_are_restricted_to_optical_drives() {
        for ok in [r"\\.\D:", r"\\.\e:", "/dev/sr0", "/dev/cdrom", "disk4"] {
            assert!(check_drive_path(ok).is_ok(), "{ok}");
        }
        for bad in [
            r"\\.\PhysicalDrive0",
            r"\\.\C:\x",
            "/dev/sda",
            "disk",
            "C:",
            "",
        ] {
            assert!(check_drive_path(bad).is_err(), "{bad}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn drive_of_cda_path() {
        assert_eq!(drive_of(Path::new(r"E:\Track01.cda")).unwrap(), r"\\.\E:");
        assert_eq!(
            drive_of(Path::new(r"\\?\d:\Track02.cda")).unwrap(),
            r"\\.\D:"
        );
        assert!(drive_of(Path::new(r"\\server\share\Track01.cda")).is_err());
    }

    /// In-memory disc: sector `n` holds bytes equal to `n % 251`.
    pub(crate) struct MemDisc {
        pub sectors: u32,
    }

    impl AudioSectorReader for MemDisc {
        type Error = std::io::Error;
        fn read_audio_sectors(&self, start: u32, count: u32) -> std::io::Result<Vec<u8>> {
            if start.saturating_add(count) > self.sectors {
                return Err(std::io::Error::other("out of range"));
            }
            let mut v = Vec::new();
            for s in start..start + count {
                v.extend(std::iter::repeat_n((s % 251) as u8, SECTOR_BYTES as usize));
            }
            Ok(v)
        }
    }

    pub(crate) fn mem_toc() -> Toc {
        Toc {
            first_track: 1,
            last_track: 3,
            tracks: vec![
                Track {
                    number: 1,
                    start_lba: 0,
                    start_msf: (0, 2, 0),
                    is_audio: true,
                },
                Track {
                    number: 2,
                    start_lba: 150,
                    start_msf: (0, 4, 0),
                    is_audio: true,
                },
                // CD-Extra: the data session starts after an 11 400-sector gap.
                Track {
                    number: 3,
                    start_lba: 11_700,
                    start_msf: (2, 38, 0),
                    is_audio: false,
                },
            ],
            leadout_lba: 11_800,
        }
    }

    #[test]
    fn rips_a_track_to_a_valid_wav() {
        let d = tempfile::tempdir().unwrap();
        let wav = d.path().join("t2.wav");
        let last = std::sync::Mutex::new(0.0f32);
        let secs = rip_track(
            &MemDisc { sectors: 400 },
            &mem_toc(),
            2,
            &wav,
            &|p| *last.lock().unwrap() = p,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(secs, 2.0);
        let bytes = std::fs::read(&wav).unwrap();
        assert_eq!(bytes.len() as u64, 44 + 150 * SECTOR_BYTES);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(bytes[44], 150u8, "first sector of track 2 is LBA 150");
        assert_eq!(*last.lock().unwrap(), 1.0);
        let infos = track_infos(&mem_toc());
        assert_eq!(infos.len(), 2, "data tracks are not listed");
        assert_eq!(infos[1].seconds, 2.0, "CD-Extra gap excluded");
    }

    #[test]
    fn rip_errors_instead_of_panicking() {
        let d = tempfile::tempdir().unwrap();
        let wav = d.path().join("x.wav");
        let disc = MemDisc { sectors: 400 };
        let no = AtomicBool::new(false);
        assert!(rip_track(&disc, &mem_toc(), 9, &wav, &|_| {}, &no).is_err());
        assert!(rip_track(&disc, &mem_toc(), 3, &wav, &|_| {}, &no).is_err());
        // A disc that ends early (bad TOC / scratched) gives an error.
        let short = MemDisc { sectors: 200 };
        assert!(rip_track(&short, &mem_toc(), 2, &wav, &|_| {}, &no).is_err());
        let yes = AtomicBool::new(true);
        assert!(matches!(
            rip_track(&disc, &mem_toc(), 1, &wav, &|_| {}, &yes),
            Err(Error::Cancelled)
        ));
    }

    pub(crate) fn vmg_ifo(titles: &[(u16, u8)]) -> Vec<u8> {
        let mut b = vec![0u8; DVD_SECTOR * 2];
        b[0..12].copy_from_slice(b"DVDVIDEO-VMG");
        b[0xC4..0xC8].copy_from_slice(&1u32.to_be_bytes());
        let base = DVD_SECTOR;
        b[base..base + 2].copy_from_slice(&(titles.len() as u16).to_be_bytes());
        for (i, (chapters, vts)) in titles.iter().enumerate() {
            let at = base + 8 + i * 12;
            b[at + 2..at + 4].copy_from_slice(&chapters.to_be_bytes());
            b[at + 6] = *vts;
        }
        b
    }

    #[test]
    fn parses_title_table() {
        let t = parse_vmg_titles(&vmg_ifo(&[(1, 1), (24, 2), (3, 2)])).unwrap();
        assert_eq!(t.len(), 3);
        assert_eq!(
            t[1],
            DvdTitle {
                number: 2,
                vts: 2,
                chapters: 24
            }
        );
        assert_eq!(title_in_set(&t, 2), Some(2));
        assert!(parse_vmg_titles(b"DVDVIDEO-VMG").is_err());
        let mut huge = vmg_ifo(&[]);
        huge[0xC4..0xC8].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(parse_vmg_titles(&huge).is_err());
        let mut lying = vmg_ifo(&[(1, 1)]);
        lying[DVD_SECTOR..DVD_SECTOR + 2].copy_from_slice(&5000u16.to_be_bytes());
        assert!(parse_vmg_titles(&lying).is_err(), "count beyond the buffer");
    }

    #[test]
    fn finds_dvd_folders_and_title_sets() {
        let d = tempfile::tempdir().unwrap();
        let vts = d.path().join("VIDEO_TS");
        std::fs::create_dir(&vts).unwrap();
        for (name, len) in [
            ("VIDEO_TS.VOB", 10),
            ("VTS_01_0.VOB", 10),
            ("VTS_01_1.VOB", 100),
            ("VTS_02_1.VOB", 500),
            ("vts_02_2.vob", 500),
            ("VTS_02_10.VOB", 5),
            ("notes.txt", 1),
        ] {
            std::fs::write(vts.join(name), vec![0u8; len]).unwrap();
        }
        let src = locate_dvd(d.path()).unwrap();
        assert_eq!(src.video_ts(), Some(vts.as_path()));
        assert_eq!(locate_dvd(&vts).unwrap(), src);
        assert_eq!(locate_dvd(&vts.join("VTS_01_1.VOB")).unwrap(), src);
        assert!(locate_dvd(&d.path().join("missing")).is_err());
        let sets = title_sets(&vts).unwrap();
        assert_eq!(sets.len(), 2);
        assert_eq!(sets[1].vobs.len(), 3);
        assert!(
            sets[1].vobs[2].ends_with("VTS_02_10.VOB"),
            "numeric order, not text order"
        );
        let titles = [DvdTitle {
            number: 1,
            vts: 1,
            chapters: 1,
        }];
        assert_eq!(choose_title_set(&sets, &titles, None).unwrap().vts, 2);
        assert_eq!(choose_title_set(&sets, &titles, Some(1)).unwrap().vts, 1);
        let url = concat_url(&sets[0]).to_string_lossy().into_owned();
        assert!(url.starts_with("concat:") && url.ends_with("VTS_01_1.VOB"));
    }

    #[test]
    fn wav_header_layout() {
        let h = wav_header(176_400);
        assert_eq!(&h[0..4], b"RIFF");
        assert_eq!(&h[36..40], b"data");
        assert_eq!(u32::from_le_bytes([h[40], h[41], h[42], h[43]]), 176_400);
        let max = wav_header(u32::MAX);
        assert_eq!(
            &max[4..8],
            &u32::MAX.to_le_bytes(),
            "saturates instead of overflowing"
        );
    }
}
