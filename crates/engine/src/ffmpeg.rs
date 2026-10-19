//! Locating FFmpeg and asking it what it can do.
//!
//! FFmpeg is never linked: it always runs as a separate process, so its
//! licence stays separate from ours (see NOTICE).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Ffmpeg {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    encoders: HashSet<String>,
    pub version: String,
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Places to look for the binaries, most specific first.
fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(dir) = std::env::var("FORMATCRAFT_FFMPEG_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Ok(me) = std::env::current_exe()
        && let Some(dir) = me.parent()
    {
        dirs.push(dir.join("ffmpeg"));
        dirs.push(dir.to_path_buf());
        // macOS bundle: Contents/MacOS/formatcraft -> Contents/Resources/ffmpeg
        if let Some(contents) = dir.parent() {
            dirs.push(contents.join("Resources").join("ffmpeg"));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    // GUI apps on macOS do not inherit the shell PATH.
    for extra in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        dirs.push(PathBuf::from(extra));
    }
    dirs
}

fn find_tool(env_var: &str, name: &str) -> Option<PathBuf> {
    if let Ok(p) = std::env::var(env_var) {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let file = exe(name);
    candidate_dirs()
        .into_iter()
        .map(|d| d.join(&file))
        .find(|p| p.is_file())
}

pub(crate) fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

impl Ffmpeg {
    /// Finds FFmpeg and reads its encoder list. Cached for the process.
    pub fn get() -> Result<&'static Ffmpeg> {
        static CELL: OnceLock<Option<Ffmpeg>> = OnceLock::new();
        CELL.get_or_init(|| Ffmpeg::detect().ok())
            .as_ref()
            .ok_or(Error::FfmpegMissing)
    }

    pub fn detect() -> Result<Ffmpeg> {
        let ffmpeg = find_tool("FORMATCRAFT_FFMPEG", "ffmpeg").ok_or(Error::FfmpegMissing)?;
        let ffprobe = find_tool("FORMATCRAFT_FFPROBE", "ffprobe")
            .or_else(|| ffmpeg.parent().map(|d| d.join(exe("ffprobe"))))
            .ok_or(Error::FfmpegMissing)?;
        let out = command(&ffmpeg)
            .args(["-hide_banner", "-encoders"])
            .output()
            .map_err(|_| Error::FfmpegMissing)?;
        let text = String::from_utf8_lossy(&out.stdout);
        let encoders = parse_encoders(&text);
        let version = command(&ffmpeg)
            .arg("-version")
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .next()
                    .map(str::to_string)
            })
            .unwrap_or_default();
        Ok(Ffmpeg {
            ffmpeg,
            ffprobe,
            encoders,
            version,
        })
    }

    pub fn has_encoder(&self, name: &str) -> bool {
        self.encoders.contains(name)
    }

    /// First encoder from `prefs` that this FFmpeg build supports.
    pub fn pick<'a>(&self, prefs: &[&'a str]) -> Option<&'a str> {
        prefs.iter().copied().find(|e| self.has_encoder(e))
    }

    #[cfg(test)]
    pub(crate) fn fake(encoders: &[&str]) -> Ffmpeg {
        Ffmpeg {
            ffmpeg: PathBuf::from("ffmpeg"),
            ffprobe: PathBuf::from("ffprobe"),
            encoders: encoders.iter().map(|s| s.to_string()).collect(),
            version: "fake".into(),
        }
    }
}

/// Parses `ffmpeg -encoders`: lines like ` V....D libx264   description`.
fn parse_encoders(text: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    let mut in_list = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("------") {
            in_list = true;
            continue;
        }
        if !in_list {
            continue;
        }
        let mut parts = t.split_whitespace();
        if let (Some(flags), Some(name)) = (parts.next(), parts.next())
            && flags.len() == 6
        {
            set.insert(name.to_string());
        }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_encoder_list() {
        let text = "Encoders:\n V..... = Video\n ------\n V....D libx264   H.264\n A....D aac   AAC\n garbage\n";
        let set = parse_encoders(text);
        assert!(set.contains("libx264"));
        assert!(set.contains("aac"));
        assert!(!set.contains("="));
        assert!(parse_encoders("").is_empty());
    }

    #[test]
    fn pick_prefers_order() {
        let f = Ffmpeg::fake(&["mpeg4", "h264_mf"]);
        assert_eq!(f.pick(&["libx264", "h264_mf", "mpeg4"]), Some("h264_mf"));
        assert_eq!(f.pick(&["libx264"]), None);
    }
}
