//! Job descriptions: what to do, with which options, and where the result goes.
//!
//! Jobs are plain data (serde) so the UI queue, the CLI scripts and the
//! automation tools all describe work the same way.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::formats::{self, Category, Format};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    High,
    #[default]
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rotation {
    #[default]
    None,
    Cw90,
    Ccw90,
    R180,
    FlipH,
    FlipV,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
    Center,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Watermark {
    pub image: PathBuf,
    #[serde(default)]
    pub corner: Corner,
    /// 0.0 (invisible) ..= 1.0 (opaque)
    #[serde(default = "one")]
    pub opacity: f32,
}

fn one() -> f32 {
    1.0
}

/// Options for audio and video conversions. Everything is optional; the
/// defaults reproduce the source as closely as the target format allows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaOptions {
    pub quality: Quality,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub video_bitrate_k: Option<u32>,
    pub audio_bitrate_k: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    /// Trim start in seconds.
    pub start: Option<f64>,
    /// Trim end in seconds (absolute position in the source).
    pub end: Option<f64>,
    pub crop: Option<Crop>,
    pub rotate: Rotation,
    pub no_audio: bool,
    pub volume_db: Option<f64>,
    /// Playback speed, 0.25 ..= 4.0.
    pub speed: Option<f64>,
    pub watermark: Option<Watermark>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageOptions {
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Scale in percent, applied when width/height are not set.
    pub scale_percent: Option<u32>,
    pub rotate: Rotation,
    pub grayscale: bool,
    /// JPEG quality 1..=100 (default 90).
    pub jpeg_quality: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Task {
    /// Video/audio conversion (also: extract audio, video to GIF).
    Convert {
        input: PathBuf,
        format: String,
        #[serde(default)]
        options: MediaOptions,
    },
    ConvertImage {
        input: PathBuf,
        format: String,
        #[serde(default)]
        options: ImageOptions,
    },
    /// Concatenate videos (or audio files when `format` is an audio format).
    Join {
        inputs: Vec<PathBuf>,
        format: String,
        #[serde(default)]
        options: MediaOptions,
    },
    /// Put the audio of one file on the video of another.
    Mux {
        video: PathBuf,
        audio: PathBuf,
        format: String,
    },
    /// Cut a media file into pieces of `segment_secs` without re-encoding.
    Split {
        input: PathBuf,
        segment_secs: f64,
    },
    ScreenRecord {
        format: String,
        #[serde(default)]
        fps: Option<u32>,
        /// Stop automatically after this many seconds (otherwise: until stopped).
        #[serde(default)]
        max_secs: Option<f64>,
        #[serde(default)]
        record_audio: bool,
    },
    PdfMerge {
        inputs: Vec<PathBuf>,
    },
    PdfSplit {
        input: PathBuf,
    },
    PdfCompress {
        input: PathBuf,
    },
    ImagesToPdf {
        inputs: Vec<PathBuf>,
    },
    /// Rip one audio CD track, given either its `.cda` file (Windows) or a
    /// drive device path plus a track number.
    CdTrack {
        #[serde(default)]
        cda: Option<PathBuf>,
        #[serde(default)]
        drive: Option<String>,
        #[serde(default)]
        track: Option<u8>,
        format: String,
        #[serde(default)]
        options: MediaOptions,
    },
    /// Extract the audio of a DVD-Video title. `input` is the DVD drive, its
    /// `VIDEO_TS` folder (or a file in it) or an `.iso` image.
    DvdToAudio {
        input: PathBuf,
        /// DVD title number; default: the main feature.
        #[serde(default)]
        title: Option<u32>,
        /// 0-based audio stream (language); default: the first.
        #[serde(default)]
        audio_track: Option<u32>,
        format: String,
        #[serde(default)]
        options: MediaOptions,
    },
}

/// A task plus the place its result goes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub task: Task,
    /// Final output path (file, or folder for split jobs).
    pub output: PathBuf,
}

impl Task {
    /// The first input, used for naming outputs and display.
    pub fn primary_input(&self) -> Option<&Path> {
        match self {
            Task::Convert { input, .. }
            | Task::ConvertImage { input, .. }
            | Task::Split { input, .. }
            | Task::PdfSplit { input }
            | Task::PdfCompress { input } => Some(input),
            Task::Mux { video, .. } => Some(video),
            Task::DvdToAudio { input, .. } => Some(input),
            Task::CdTrack { cda, .. } => cda.as_deref(),
            Task::Join { inputs, .. }
            | Task::PdfMerge { inputs }
            | Task::ImagesToPdf { inputs } => inputs.first().map(PathBuf::as_path),
            Task::ScreenRecord { .. } => None,
        }
    }

    pub fn inputs(&self) -> Vec<&Path> {
        match self {
            Task::Mux { video, audio, .. } => vec![video, audio],
            Task::Join { inputs, .. }
            | Task::PdfMerge { inputs }
            | Task::ImagesToPdf { inputs } => inputs.iter().map(PathBuf::as_path).collect(),
            other => other.primary_input().into_iter().collect(),
        }
    }

    pub fn format_id(&self) -> &str {
        match self {
            Task::Convert { format, .. }
            | Task::ConvertImage { format, .. }
            | Task::Join { format, .. }
            | Task::Mux { format, .. }
            | Task::ScreenRecord { format, .. }
            | Task::CdTrack { format, .. }
            | Task::DvdToAudio { format, .. } => format,
            Task::Split { .. } => "split",
            Task::PdfMerge { .. }
            | Task::PdfSplit { .. }
            | Task::PdfCompress { .. }
            | Task::ImagesToPdf { .. } => "pdf",
        }
    }

    /// Short machine-readable name of the operation.
    pub fn kind(&self) -> &'static str {
        match self {
            Task::Convert { .. } => "convert",
            Task::ConvertImage { .. } => "convert_image",
            Task::Join { .. } => "join",
            Task::Mux { .. } => "mux",
            Task::Split { .. } => "split",
            Task::ScreenRecord { .. } => "screen_record",
            Task::PdfMerge { .. } => "pdf_merge",
            Task::PdfSplit { .. } => "pdf_split",
            Task::PdfCompress { .. } => "pdf_compress",
            Task::ImagesToPdf { .. } => "images_to_pdf",
            Task::CdTrack { .. } => "cd_track",
            Task::DvdToAudio { .. } => "dvd_to_audio",
        }
    }

    /// Checks the format/category pairing and input existence up front, so a
    /// queued job fails early with a clear message.
    pub fn validate(&self) -> Result<()> {
        for input in self.inputs() {
            // A DVD input may be a drive or a folder.
            let ok = if matches!(self, Task::DvdToAudio { .. }) {
                input.exists()
            } else {
                input.is_file()
            };
            if !ok {
                return Err(Error::MissingInput(input.to_path_buf()));
            }
        }
        let need = |id: &str, cats: &[Category], what: &'static str| -> Result<&'static Format> {
            let f = formats::find(id).ok_or_else(|| Error::UnknownFormat(id.to_string()))?;
            if cats.contains(&f.category) {
                Ok(f)
            } else {
                Err(Error::WrongCategory {
                    format: id.to_string(),
                    what,
                })
            }
        };
        let min = |what: &'static str, n: usize, got: usize| -> Result<()> {
            if got < n {
                Err(Error::TooFewInputs { what, min: n, got })
            } else {
                Ok(())
            }
        };
        match self {
            Task::Convert {
                format, options, ..
            } => {
                need(
                    format,
                    &[Category::Video, Category::Audio],
                    "audio/video conversion",
                )?;
                options.check()
            }
            Task::ConvertImage { format, .. } => {
                need(format, &[Category::Image], "image conversion").map(|_| ())
            }
            Task::Join {
                inputs,
                format,
                options,
            } => {
                min("join", 2, inputs.len())?;
                need(format, &[Category::Video, Category::Audio], "join")?;
                options.check()
            }
            Task::Mux { format, .. } => need(format, &[Category::Video], "mux").map(|_| ()),
            Task::Split { segment_secs, .. } => {
                if segment_secs.is_finite() && *segment_secs >= 1.0 {
                    Ok(())
                } else {
                    Err(Error::InvalidOption(
                        "segment length must be at least 1 second".into(),
                    ))
                }
            }
            Task::ScreenRecord {
                format,
                fps,
                max_secs,
                ..
            } => {
                need(format, &[Category::Video], "screen recording")?;
                if let Some(f) = fps
                    && !(1..=120).contains(f)
                {
                    return Err(Error::InvalidOption("fps must be 1..=120".into()));
                }
                if let Some(s) = max_secs
                    && !(s.is_finite() && *s > 0.0)
                {
                    return Err(Error::InvalidOption("max_secs must be positive".into()));
                }
                Ok(())
            }
            Task::PdfMerge { inputs } => min("PDF merge", 2, inputs.len()),
            Task::ImagesToPdf { inputs } => min("images to PDF", 1, inputs.len()),
            Task::PdfSplit { .. } | Task::PdfCompress { .. } => Ok(()),
            Task::CdTrack {
                cda,
                drive,
                track,
                format,
                options,
            } => {
                need(format, &[Category::Audio], "audio CD ripping")?;
                options.check()?;
                match (cda, drive, track) {
                    (Some(c), _, _) if !crate::disc::is_cda(c) => Err(Error::InvalidOption(
                        format!("{} is not a .cda file", c.display()),
                    )),
                    (Some(_), _, _) => Ok(()),
                    (None, Some(d), Some(t)) if (1..=99).contains(t) => {
                        crate::disc::check_drive_path(d)
                    }
                    (None, Some(_), _) => Err(Error::InvalidOption("track must be 1..=99".into())),
                    (None, None, _) => Err(Error::InvalidOption(
                        "give either `cda` (a TrackNN.cda file) or `drive` and `track`".into(),
                    )),
                }
            }
            Task::DvdToAudio {
                title,
                audio_track,
                format,
                options,
                ..
            } => {
                need(format, &[Category::Audio], "DVD audio extraction")?;
                options.check()?;
                if let Some(t) = title
                    && !(1..=99).contains(t)
                {
                    return Err(Error::InvalidOption("DVD title must be 1..=99".into()));
                }
                if let Some(a) = audio_track
                    && *a > 15
                {
                    return Err(Error::InvalidOption(
                        "DVD audio track must be 0..=15".into(),
                    ));
                }
                Ok(())
            }
        }
    }

    /// Extension of the produced file (None for folder outputs).
    pub fn output_ext(&self) -> Option<&'static str> {
        match self {
            Task::Split { .. } | Task::PdfSplit { .. } => None,
            Task::PdfMerge { .. } | Task::PdfCompress { .. } | Task::ImagesToPdf { .. } => {
                Some("pdf")
            }
            other => formats::find(other.format_id()).map(|f| f.ext),
        }
    }

    /// Builds a job writing into `dir`, choosing a name that does not
    /// overwrite anything already there.
    pub fn into_job(self, dir: &Path) -> Result<Job> {
        self.validate()?;
        let stem = match (&self, self.primary_input()) {
            (Task::CdTrack { track: Some(t), .. }, None) => format!("Track {t:02}"),
            (Task::DvdToAudio { input, .. }, _) => dvd_name(input),
            (_, Some(p)) => p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "output".into()),
            (_, None) => "recording".into(),
        };
        let suffix = match &self {
            Task::Join { .. } | Task::PdfMerge { .. } => "_joined",
            Task::Mux { .. } => "_mux",
            Task::Split { .. } | Task::PdfSplit { .. } => "_parts",
            Task::PdfCompress { .. } => "_compressed",
            _ => "",
        };
        let base = format!("{stem}{suffix}");
        let output = unique_path(dir, &base, self.output_ext());
        Ok(Job { task: self, output })
    }
}

/// A readable name for a DVD: the folder holding `VIDEO_TS`, else "DVD".
fn dvd_name(input: &Path) -> String {
    let mut p = input;
    for _ in 0..2 {
        let is_vts = p
            .file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("VIDEO_TS"));
        if (is_vts || p.is_file())
            && let Some(parent) = p.parent()
        {
            p = parent;
        }
    }
    if input
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("iso"))
    {
        p = input;
    }
    p.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "DVD".into())
}

impl MediaOptions {
    pub fn check(&self) -> Result<()> {
        let bad = |m: &str| Err(Error::InvalidOption(m.to_string()));
        for (name, v) in [("width", self.width), ("height", self.height)] {
            if let Some(v) = v
                && !(2..=16384).contains(&v)
            {
                return bad(&format!("{name} must be 2..=16384"));
            }
        }
        if let Some(f) = self.fps
            && !(f.is_finite() && f > 0.0 && f <= 240.0)
        {
            return bad("fps must be in (0, 240]");
        }
        for (name, v) in [("start", self.start), ("end", self.end)] {
            if let Some(v) = v
                && !(v.is_finite() && v >= 0.0)
            {
                return bad(&format!("{name} must be a non-negative number of seconds"));
            }
        }
        if let (Some(s), Some(e)) = (self.start, self.end)
            && e <= s
        {
            return bad("end must be after start");
        }
        if let Some(s) = self.speed
            && !(s.is_finite() && (0.25..=4.0).contains(&s))
        {
            return bad("speed must be 0.25..=4.0");
        }
        if let Some(v) = self.volume_db
            && !(v.is_finite() && (-60.0..=30.0).contains(&v))
        {
            return bad("volume must be -60..=30 dB");
        }
        if let Some(c) = self.channels
            && !(1..=8).contains(&c)
        {
            return bad("channels must be 1..=8");
        }
        if let Some(r) = self.sample_rate
            && !(8000..=192_000).contains(&r)
        {
            return bad("sample rate must be 8000..=192000");
        }
        if let Some(c) = self.crop
            && (c.width < 2 || c.height < 2)
        {
            return bad("crop size must be at least 2x2");
        }
        if let Some(w) = &self.watermark {
            if !w.image.is_file() {
                return Err(Error::MissingInput(w.image.clone()));
            }
            if !(w.opacity.is_finite() && (0.0..=1.0).contains(&w.opacity)) {
                return bad("watermark opacity must be 0..=1");
            }
        }
        Ok(())
    }

    /// Length of the output in seconds, given the source duration.
    pub fn output_duration(&self, source: Option<f64>) -> Option<f64> {
        let end = match (self.end, source) {
            (Some(e), Some(s)) => e.min(s),
            (Some(e), None) => e,
            (None, s) => s?,
        };
        let len = (end - self.start.unwrap_or(0.0)).max(0.0);
        let speed = self.speed.filter(|s| *s > 0.0).unwrap_or(1.0);
        let d = len / speed;
        d.is_finite().then_some(d)
    }
}

/// `dir/base.ext`, or `dir/base (2).ext`, ... — the first that does not exist.
pub fn unique_path(dir: &Path, base: &str, ext: Option<&str>) -> PathBuf {
    let name = |n: u32| {
        let b = if n <= 1 {
            base.to_string()
        } else {
            format!("{base} ({n})")
        };
        match ext {
            Some(e) => format!("{b}.{e}"),
            None => b,
        }
    };
    (1..10_000)
        .map(|n| dir.join(name(n)))
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join(name(10_000)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_path_skips_existing() {
        let d = tempfile::tempdir().unwrap();
        let first = unique_path(d.path(), "a", Some("mp4"));
        assert_eq!(first.file_name().unwrap(), "a.mp4");
        std::fs::write(&first, b"x").unwrap();
        let second = unique_path(d.path(), "a", Some("mp4"));
        assert_eq!(second.file_name().unwrap(), "a (2).mp4");
    }

    #[test]
    fn options_are_checked() {
        let mut o = MediaOptions::default();
        assert!(o.check().is_ok());
        o.start = Some(5.0);
        o.end = Some(2.0);
        assert!(o.check().is_err());
        o.end = Some(f64::NAN);
        assert!(o.check().is_err());
        o.end = None;
        o.speed = Some(10.0);
        assert!(o.check().is_err());
    }

    #[test]
    fn output_duration_accounts_for_trim_and_speed() {
        let o = MediaOptions {
            start: Some(10.0),
            end: Some(30.0),
            speed: Some(2.0),
            ..Default::default()
        };
        assert_eq!(o.output_duration(Some(100.0)), Some(10.0));
        assert_eq!(o.output_duration(Some(20.0)), Some(5.0));
        assert_eq!(MediaOptions::default().output_duration(None), None);
    }

    #[test]
    fn validate_rejects_wrong_category_and_missing_files() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("in.mp4");
        std::fs::write(&f, b"x").unwrap();
        let t = Task::Convert {
            input: f.clone(),
            format: "png".into(),
            options: Default::default(),
        };
        assert!(matches!(t.validate(), Err(Error::WrongCategory { .. })));
        let t = Task::Convert {
            input: d.path().join("missing.mp4"),
            format: "mp3".into(),
            options: Default::default(),
        };
        assert!(matches!(t.validate(), Err(Error::MissingInput(_))));
        let t = Task::Join {
            inputs: vec![f],
            format: "mp4".into(),
            options: Default::default(),
        };
        assert!(matches!(t.validate(), Err(Error::TooFewInputs { .. })));
    }

    #[test]
    fn job_json_roundtrip() {
        let json = r#"{"kind":"convert","input":"a.mkv","format":"mp4","options":{"quality":"high","width":640}}"#;
        let t: Task = serde_json::from_str(json).unwrap();
        assert_eq!(t.format_id(), "mp4");
        let back = serde_json::to_string(&t).unwrap();
        let again: Task = serde_json::from_str(&back).unwrap();
        assert_eq!(t, again);
    }
}
