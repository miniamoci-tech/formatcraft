//! Media information through `ffprobe`.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::ffmpeg::{Ffmpeg, command};

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct StreamInfo {
    pub index: u64,
    pub kind: String,
    pub codec: String,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub fps: Option<f64>,
    pub sample_rate: Option<u64>,
    pub channels: Option<u64>,
    pub bit_rate: Option<u64>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct MediaInfo {
    pub container: String,
    pub duration_secs: Option<f64>,
    pub size_bytes: Option<u64>,
    pub bit_rate: Option<u64>,
    pub streams: Vec<StreamInfo>,
}

impl MediaInfo {
    pub fn has_video(&self) -> bool {
        self.streams.iter().any(|s| s.kind == "video")
    }
    pub fn has_audio(&self) -> bool {
        self.streams.iter().any(|s| s.kind == "audio")
    }
}

pub fn probe(input: &Path) -> Result<MediaInfo> {
    if !input.is_file() {
        return Err(Error::MissingInput(input.to_path_buf()));
    }
    probe_input(&[], input.as_os_str())
}

/// Probes an input that needs demuxer options (`-f dvdvideo -title 2`) or is
/// not a plain file (`concat:a.vob|b.vob`). `input_args` go before `-i`.
pub fn probe_input(input_args: &[OsString], input: &OsStr) -> Result<MediaInfo> {
    let ff = Ffmpeg::get()?;
    let out = command(&ff.ffprobe)
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .args(input_args)
        .arg("-i")
        .arg(input)
        .output()
        .map_err(|e| Error::Probe(e.to_string()))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(Error::Probe(if msg.is_empty() {
            "ffprobe could not read the file".into()
        } else {
            msg
        }));
    }
    parse_probe_json(&String::from_utf8_lossy(&out.stdout))
}

fn num_u64(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn num_f64(v: Option<&Value>) -> Option<f64> {
    let f = match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }?;
    f.is_finite().then_some(f)
}

/// Parses rates like `30000/1001`; rejects zero denominators.
fn rate(v: Option<&Value>) -> Option<f64> {
    let s = v?.as_str()?;
    let (n, d) = s.split_once('/')?;
    let n: f64 = n.parse().ok()?;
    let d: f64 = d.parse().ok()?;
    if d == 0.0 || n == 0.0 {
        return None;
    }
    let r = n / d;
    r.is_finite().then_some(r)
}

pub fn parse_probe_json(text: &str) -> Result<MediaInfo> {
    let v: Value = serde_json::from_str(text).map_err(|e| Error::Probe(e.to_string()))?;
    let format = v.get("format");
    let mut info = MediaInfo {
        container: format
            .and_then(|f| f.get("format_name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        duration_secs: num_f64(format.and_then(|f| f.get("duration"))).filter(|d| *d >= 0.0),
        size_bytes: num_u64(format.and_then(|f| f.get("size"))),
        bit_rate: num_u64(format.and_then(|f| f.get("bit_rate"))),
        streams: Vec::new(),
    };
    if let Some(streams) = v.get("streams").and_then(Value::as_array) {
        for s in streams.iter().take(1024) {
            let str_field = |k: &str| s.get(k).and_then(Value::as_str).map(str::to_string);
            info.streams.push(StreamInfo {
                index: num_u64(s.get("index")).unwrap_or_default(),
                kind: str_field("codec_type").unwrap_or_default(),
                codec: str_field("codec_name").unwrap_or_default(),
                width: num_u64(s.get("width")),
                height: num_u64(s.get("height")),
                fps: rate(s.get("avg_frame_rate")).or_else(|| rate(s.get("r_frame_rate"))),
                sample_rate: num_u64(s.get("sample_rate")),
                channels: num_u64(s.get("channels")),
                bit_rate: num_u64(s.get("bit_rate")),
                language: s
                    .get("tags")
                    .and_then(|t| t.get("language"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }
    }
    if info.duration_secs.is_none() {
        info.duration_secs = streams_duration(&v);
    }
    Ok(info)
}

fn streams_duration(v: &Value) -> Option<f64> {
    v.get("streams")?
        .as_array()?
        .iter()
        .filter_map(|s| num_f64(s.get("duration")))
        .filter(|d| *d >= 0.0)
        .reduce(f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typical_output() {
        let json = r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264","width":1280,"height":720,"avg_frame_rate":"30000/1001"},
            {"index":1,"codec_type":"audio","codec_name":"aac","sample_rate":"48000","channels":2,"tags":{"language":"ita"}}],
            "format":{"format_name":"mov,mp4","duration":"12.5","size":"1000","bit_rate":"640"}}"#;
        let info = parse_probe_json(json).unwrap();
        assert_eq!(info.duration_secs, Some(12.5));
        assert!(info.has_video() && info.has_audio());
        let fps = info.streams[0].fps.unwrap();
        assert!((fps - 29.97).abs() < 0.01);
        assert_eq!(info.streams[1].sample_rate, Some(48000));
        assert_eq!(info.streams[1].language.as_deref(), Some("ita"));
    }

    #[test]
    fn hostile_values_do_not_panic() {
        let json = r#"{"streams":[{"avg_frame_rate":"1/0","r_frame_rate":"x","index":-3,"width":"NaN"}, 5, null],
            "format":{"duration":"-inf"}}"#;
        let info = parse_probe_json(json).unwrap();
        assert_eq!(info.duration_secs, None);
        assert_eq!(info.streams[0].fps, None);
        assert!(parse_probe_json("not json").is_err());
        assert!(parse_probe_json("[]").is_ok());
    }
}
