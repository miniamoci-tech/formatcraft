//! Pure functions that turn a task into FFmpeg arguments. Nothing here runs a
//! process, so every branch is unit-testable.

use std::ffi::OsString;
use std::path::Path;

use crate::error::{Error, Result};
use crate::ffmpeg::Ffmpeg;
use crate::formats::{Category, Format};
use crate::job::{Corner, MediaOptions, Quality, Rotation};
use crate::probe::MediaInfo;

#[derive(Default)]
struct Args(Vec<OsString>);

impl Args {
    fn s(&mut self, a: impl Into<OsString>) -> &mut Self {
        self.0.push(a.into());
        self
    }
    fn all<I: IntoIterator<Item = S>, S: Into<OsString>>(&mut self, it: I) -> &mut Self {
        self.0.extend(it.into_iter().map(Into::into));
        self
    }
}

fn prelude() -> Args {
    let mut a = Args::default();
    a.all([
        "-hide_banner",
        "-y",
        "-loglevel",
        "error",
        "-progress",
        "pipe:1",
        "-nostats",
    ]);
    a
}

/// Formats seconds for FFmpeg with millisecond precision.
fn secs(v: f64) -> String {
    format!("{:.3}", v.max(0.0))
}

fn no_encoder(kind: &str, fmt: &Format) -> Error {
    Error::InvalidOption(format!(
        "this FFmpeg build has no {kind} encoder for {}; install a full FFmpeg build",
        fmt.label
    ))
}

/// Encoders that need even frame sizes and yuv420p for broad playback.
fn needs_yuv420(enc: &str) -> bool {
    enc.starts_with("libx26")
        || enc.starts_with("h264")
        || enc.starts_with("hevc")
        || enc.starts_with("libvpx")
        || matches!(enc, "mpeg4" | "mpeg2video" | "wmv2" | "libtheora" | "h263")
}

fn video_quality_args(a: &mut Args, enc: &str, o: &MediaOptions) {
    if let Some(k) = o.video_bitrate_k {
        a.s("-b:v").s(format!("{k}k"));
        if enc == "libvpx-vp9" || enc == "libvpx" {
            a.all(["-deadline", "good", "-cpu-used", "4"]);
        }
        return;
    }
    let q = o.quality;
    match enc {
        "libx264" => {
            let crf = match q {
                Quality::High => 18,
                Quality::Medium => 23,
                Quality::Low => 28,
            };
            a.all(["-preset", "medium", "-crf"]).s(crf.to_string());
        }
        "libx265" => {
            let crf = match q {
                Quality::High => 22,
                Quality::Medium => 28,
                Quality::Low => 32,
            };
            a.all(["-preset", "medium", "-crf"]).s(crf.to_string());
        }
        "libvpx-vp9" | "libvpx" => {
            let crf = match q {
                Quality::High => 24,
                Quality::Medium => 32,
                Quality::Low => 40,
            };
            a.s("-crf").s(crf.to_string()).all([
                "-b:v",
                "0",
                "-deadline",
                "good",
                "-cpu-used",
                "4",
                "-row-mt",
                "1",
            ]);
        }
        "gif" => {}
        "libwebp_anim" | "libwebp" => {
            let v = match q {
                Quality::High => "90",
                Quality::Medium => "75",
                Quality::Low => "50",
            };
            a.all(["-loop", "0", "-quality", v]);
        }
        "mpeg4" | "mpeg2video" | "wmv2" | "h263" | "libtheora" | "mjpeg" => {
            let v = match q {
                Quality::High => "2",
                Quality::Medium => "4",
                Quality::Low => "8",
            };
            a.s("-q:v").s(v);
        }
        _ => {
            // Hardware encoders: rate control by bitrate.
            let b = match q {
                Quality::High => "8M",
                Quality::Medium => "4M",
                Quality::Low => "1500k",
            };
            a.s("-b:v").s(b);
        }
    }
}

fn audio_quality_args(a: &mut Args, enc: &str, o: &MediaOptions) {
    let lossless = enc.starts_with("pcm_") || matches!(enc, "flac" | "wavpack" | "alac");
    if !lossless {
        let k = o.audio_bitrate_k.unwrap_or(match (enc, o.quality) {
            ("libopus", Quality::High) => 160,
            ("libopus", Quality::Medium) => 128,
            ("libopus", Quality::Low) => 96,
            ("libopencore_amrnb", _) => 12,
            (_, Quality::High) => 256,
            (_, Quality::Medium) => 192,
            (_, Quality::Low) => 128,
        });
        if enc == "libopencore_amrnb" {
            a.all(["-b:a", "12.2k"]);
        } else {
            a.s("-b:a").s(format!("{k}k"));
        }
    }
    if let Some(r) = o.sample_rate {
        a.s("-ar").s(r.to_string());
    } else if enc == "libopus" {
        a.all(["-ar", "48000"]);
    }
    if let Some(c) = o.channels {
        a.s("-ac").s(c.to_string());
    }
}

/// atempo accepts 0.5..=2.0 per instance; chain instances for wider ranges.
fn atempo_chain(speed: f64) -> Vec<String> {
    let mut out = Vec::new();
    let mut s = speed;
    let mut guard = 0;
    while (s > 2.0 || s < 0.5) && guard < 8 {
        if s > 2.0 {
            out.push("atempo=2.0".to_string());
            s /= 2.0;
        } else {
            out.push("atempo=0.5".to_string());
            s /= 0.5;
        }
        guard += 1;
    }
    if (s - 1.0).abs() > 1e-6 {
        out.push(format!("atempo={s:.6}"));
    }
    out
}

fn audio_filters(o: &MediaOptions) -> Vec<String> {
    let mut f = Vec::new();
    if let Some(s) = o.speed {
        f.extend(atempo_chain(s));
    }
    if let Some(db) = o.volume_db
        && db.abs() > 1e-6
    {
        f.push(format!("volume={db:.2}dB"));
    }
    f
}

fn scale_expr(w: Option<u32>, h: Option<u32>, even: bool) -> Option<String> {
    let dim = |v: Option<u32>| match v {
        Some(v) if even => (v & !1).max(2).to_string(),
        Some(v) => v.to_string(),
        None if even => "-2".to_string(),
        None => "-1".to_string(),
    };
    if w.is_none() && h.is_none() {
        return None;
    }
    Some(format!("scale={}:{}:flags=lanczos", dim(w), dim(h)))
}

fn video_filters(o: &MediaOptions, enc: &str, is_gif: bool) -> Vec<String> {
    let even = needs_yuv420(enc);
    let mut f = Vec::new();
    if let Some(c) = o.crop {
        let (w, h) = if even {
            (c.width & !1, c.height & !1)
        } else {
            (c.width, c.height)
        };
        f.push(format!("crop={w}:{h}:{}:{}", c.x, c.y));
    }
    match o.rotate {
        Rotation::None => {}
        Rotation::Cw90 => f.push("transpose=1".into()),
        Rotation::Ccw90 => f.push("transpose=2".into()),
        Rotation::R180 => f.push("transpose=1,transpose=1".into()),
        Rotation::FlipH => f.push("hflip".into()),
        Rotation::FlipV => f.push("vflip".into()),
    }
    let (w, h) = if is_gif && o.width.is_none() && o.height.is_none() {
        (Some(480), None)
    } else if enc == "h263" && o.width.is_none() && o.height.is_none() {
        (Some(352), Some(288))
    } else {
        (o.width, o.height)
    };
    match scale_expr(w, h, even) {
        Some(s) => f.push(s),
        None if even => f.push("scale=trunc(iw/2)*2:trunc(ih/2)*2".into()),
        None => {}
    }
    if let Some(s) = o.speed
        && (s - 1.0).abs() > 1e-6
    {
        f.push(format!("setpts=PTS/{s:.6}"));
    }
    let fps = o.fps.or(if is_gif { Some(12.0) } else { None });
    if let Some(fps) = fps {
        f.push(format!("fps={fps}"));
    }
    if even {
        f.push("format=yuv420p".into());
    }
    f
}

fn overlay_position(c: Corner) -> &'static str {
    match c {
        Corner::TopLeft => "16:16",
        Corner::TopRight => "W-w-16:16",
        Corner::BottomLeft => "16:H-h-16",
        Corner::BottomRight => "W-w-16:H-h-16",
        Corner::Center => "(W-w)/2:(H-h)/2",
    }
}

/// Builds the arguments for a single-input audio or video conversion.
pub fn convert(
    ff: &Ffmpeg,
    input: &Path,
    info: &MediaInfo,
    output: &Path,
    fmt: &Format,
    o: &MediaOptions,
) -> Result<Vec<OsString>> {
    let mut a = prelude();
    if let Some(s) = o.start.filter(|s| *s > 0.0) {
        a.s("-ss").s(secs(s));
    }
    a.s("-i").s(input);
    let wm = o
        .watermark
        .as_ref()
        .filter(|_| fmt.category == Category::Video);
    if let Some(w) = wm {
        a.s("-i").s(&w.image);
    }
    if let Some(d) = o
        .end
        .map(|e| e - o.start.unwrap_or(0.0))
        .filter(|d| *d > 0.0)
    {
        a.s("-t").s(secs(d));
    }

    match fmt.category {
        Category::Video => {
            if !info.has_video() {
                return Err(Error::InvalidOption(
                    "the input has no video track; choose an audio format".into(),
                ));
            }
            let venc = ff.pick(fmt.video).ok_or_else(|| no_encoder("video", fmt))?;
            let is_gif = matches!(venc, "gif" | "libwebp_anim" | "libwebp");
            let mut chain = video_filters(o, venc, is_gif);
            if chain.is_empty() {
                chain.push("null".into());
            }
            let mut graph = format!("[0:v:0]{}[v0]", chain.join(","));
            let mut last = "v0";
            if let Some(w) = wm {
                graph.push_str(&format!(
                    ";[1:v]format=rgba,colorchannelmixer=aa={:.3}[wm];[{last}][wm]overlay={}:format=auto[v1]",
                    w.opacity,
                    overlay_position(w.corner)
                ));
                last = "v1";
            }
            if venc == "gif" {
                graph.push_str(&format!(
                    ";[{last}]split[g0][g1];[g0]palettegen=stats_mode=diff[pal];[g1][pal]paletteuse=dither=bayer:bayer_scale=5[vg]"
                ));
                last = "vg";
            }
            a.s("-filter_complex").s(graph);
            a.s("-map").s(format!("[{last}]"));
            a.s("-c:v").s(venc);
            video_quality_args(&mut a, venc, o);

            let aenc = if o.no_audio || !info.has_audio() {
                None
            } else {
                ff.pick(fmt.audio)
            };
            match aenc {
                Some(aenc) => {
                    a.all(["-map", "0:a:0?", "-c:a", aenc]);
                    audio_quality_args(&mut a, aenc, o);
                    let af = audio_filters(o);
                    if !af.is_empty() {
                        a.s("-af").s(af.join(","));
                    }
                }
                None => {
                    a.s("-an");
                }
            }
        }
        Category::Audio => {
            if !info.has_audio() {
                return Err(Error::InvalidOption("the input has no audio track".into()));
            }
            let aenc = ff.pick(fmt.audio).ok_or_else(|| no_encoder("audio", fmt))?;
            a.all(["-map", "0:a:0", "-vn", "-sn", "-c:a", aenc]);
            audio_quality_args(&mut a, aenc, o);
            let af = audio_filters(o);
            if !af.is_empty() {
                a.s("-af").s(af.join(","));
            }
        }
        Category::Image | Category::Document => {
            return Err(Error::WrongCategory {
                format: fmt.id.into(),
                what: "audio/video conversion",
            });
        }
    }
    a.s("-map_metadata").s("0");
    a.all(fmt.mux.iter().copied());
    a.s(output);
    Ok(a.0)
}

/// Concatenates inputs, normalising size/fps/audio so mixed sources work.
pub fn join(
    ff: &Ffmpeg,
    inputs: &[(&Path, &MediaInfo)],
    output: &Path,
    fmt: &Format,
    o: &MediaOptions,
) -> Result<Vec<OsString>> {
    let n = inputs.len();
    if n < 2 {
        return Err(Error::TooFewInputs {
            what: "join",
            min: 2,
            got: n,
        });
    }
    let mut a = prelude();
    for (p, _) in inputs {
        a.s("-i").s(*p);
    }
    let video = fmt.category == Category::Video;
    let mut graph = Vec::new();
    let mut extra_input = n;
    let mut labels = String::new();

    if video {
        if inputs.iter().any(|(_, i)| !i.has_video()) {
            return Err(Error::InvalidOption(
                "every file joined into a video needs a video track".into(),
            ));
        }
        let venc = ff.pick(fmt.video).ok_or_else(|| no_encoder("video", fmt))?;
        let aenc = ff.pick(fmt.audio);
        let first = inputs.first().map(|(_, i)| *i);
        let vs = first.and_then(|i| i.streams.iter().find(|s| s.kind == "video"));
        let w = o
            .width
            .or(vs.and_then(|s| s.width).map(|v| v as u32))
            .unwrap_or(1280)
            & !1;
        let h = o
            .height
            .or(vs.and_then(|s| s.height).map(|v| v as u32))
            .unwrap_or(720)
            & !1;
        let (w, h) = (w.clamp(2, 16384), h.clamp(2, 16384));
        let fps = o
            .fps
            .or(vs.and_then(|s| s.fps))
            .filter(|f| f.is_finite() && *f > 0.0 && *f <= 240.0)
            .unwrap_or(30.0);
        for (i, (_, info)) in inputs.iter().enumerate() {
            graph.push(format!(
                "[{i}:v:0]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={fps},format=yuv420p[v{i}]"
            ));
            if aenc.is_some() {
                if info.has_audio() {
                    graph.push(format!(
                        "[{i}:a:0]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo[a{i}]"
                    ));
                } else {
                    let dur = info.duration_secs.unwrap_or(1.0).max(0.1);
                    a.all(["-f", "lavfi", "-t"])
                        .s(secs(dur))
                        .s("-i")
                        .s("anullsrc=r=48000:cl=stereo");
                    graph.push(format!(
                        "[{extra_input}:a]aformat=sample_fmts=fltp:channel_layouts=stereo[a{i}]"
                    ));
                    extra_input += 1;
                }
                labels.push_str(&format!("[v{i}][a{i}]"));
            } else {
                labels.push_str(&format!("[v{i}]"));
            }
        }
        let has_a = aenc.is_some();
        graph.push(format!(
            "{labels}concat=n={n}:v=1:a={}[vout]{}",
            u8::from(has_a),
            if has_a { "[aout]" } else { "" }
        ));
        a.s("-filter_complex").s(graph.join(";"));
        a.all(["-map", "[vout]", "-c:v", venc]);
        video_quality_args(&mut a, venc, o);
        if let Some(aenc) = aenc {
            a.all(["-map", "[aout]", "-c:a", aenc]);
            audio_quality_args(&mut a, aenc, o);
        }
    } else {
        if inputs.iter().any(|(_, i)| !i.has_audio()) {
            return Err(Error::InvalidOption(
                "every file joined into audio needs an audio track".into(),
            ));
        }
        let aenc = ff.pick(fmt.audio).ok_or_else(|| no_encoder("audio", fmt))?;
        for i in 0..n {
            graph.push(format!(
                "[{i}:a:0]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo[a{i}]"
            ));
            labels.push_str(&format!("[a{i}]"));
        }
        graph.push(format!("{labels}concat=n={n}:v=0:a=1[aout]"));
        a.s("-filter_complex").s(graph.join(";"));
        a.all(["-map", "[aout]", "-c:a", aenc]);
        audio_quality_args(&mut a, aenc, o);
    }
    a.all(fmt.mux.iter().copied());
    a.s(output);
    Ok(a.0)
}

/// Video track of `video` + first audio track of `audio`.
pub fn mux(
    ff: &Ffmpeg,
    video: &Path,
    vinfo: &MediaInfo,
    audio: &Path,
    ainfo: &MediaInfo,
    output: &Path,
    fmt: &Format,
) -> Result<Vec<OsString>> {
    if !vinfo.has_video() {
        return Err(Error::InvalidOption(
            "the video file has no video track".into(),
        ));
    }
    if !ainfo.has_audio() {
        return Err(Error::InvalidOption(
            "the audio file has no audio track".into(),
        ));
    }
    let vcodec = vinfo
        .streams
        .iter()
        .find(|s| s.kind == "video")
        .map(|s| s.codec.as_str())
        .unwrap_or_default();
    let copy_ok = match fmt.ext {
        "mkv" => true,
        "mp4" | "mov" => matches!(vcodec, "h264" | "hevc" | "mpeg4" | "av1"),
        "webm" => matches!(vcodec, "vp8" | "vp9" | "av1"),
        _ => false,
    } && fmt.id != "mp4-hevc";
    let mut a = prelude();
    a.s("-i").s(video).s("-i").s(audio);
    a.all(["-map", "0:v:0", "-map", "1:a:0", "-shortest"]);
    let o = MediaOptions::default();
    if copy_ok {
        a.all(["-c:v", "copy"]);
    } else {
        let venc = ff.pick(fmt.video).ok_or_else(|| no_encoder("video", fmt))?;
        a.s("-c:v").s(venc);
        video_quality_args(&mut a, venc, &o);
        if needs_yuv420(venc) {
            a.all(["-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p"]);
        }
    }
    let aenc = ff.pick(fmt.audio).ok_or_else(|| no_encoder("audio", fmt))?;
    a.s("-c:a").s(aenc);
    audio_quality_args(&mut a, aenc, &o);
    a.all(fmt.mux.iter().copied());
    a.s(output);
    Ok(a.0)
}

/// Splits without re-encoding into `dir/<stem>_NNN.<ext>`.
pub fn split(input: &Path, dir: &Path, stem: &str, ext: &str, segment_secs: f64) -> Vec<OsString> {
    let mut a = prelude();
    a.s("-i").s(input);
    a.all(["-map", "0", "-c", "copy", "-f", "segment", "-segment_time"])
        .s(secs(segment_secs))
        .all(["-reset_timestamps", "1"]);
    a.s(dir.join(format!("{stem}_%03d.{ext}")));
    a.0
}

/// Records the main screen. `os` is `std::env::consts::OS`.
pub fn screen_record(
    ff: &Ffmpeg,
    os: &str,
    output: &Path,
    fmt: &Format,
    fps: u32,
    max_secs: Option<f64>,
    record_audio: bool,
) -> Result<Vec<OsString>> {
    let mut a = prelude();
    match os {
        "windows" => {
            a.all(["-f", "gdigrab", "-framerate"])
                .s(fps.to_string())
                .all(["-draw_mouse", "1", "-i", "desktop"]);
        }
        "macos" => {
            let dev = if record_audio {
                "Capture screen 0:0"
            } else {
                "Capture screen 0:none"
            };
            a.all(["-f", "avfoundation", "-capture_cursor", "1", "-framerate"])
                .s(fps.to_string())
                .s("-i")
                .s(dev);
        }
        _ => {
            let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
            a.all(["-f", "x11grab", "-framerate"])
                .s(fps.to_string())
                .s("-i")
                .s(display);
        }
    }
    if let Some(s) = max_secs {
        a.s("-t").s(secs(s));
    }
    let venc = ff.pick(fmt.video).ok_or_else(|| no_encoder("video", fmt))?;
    a.s("-c:v").s(venc);
    if venc == "libx264" {
        a.all(["-preset", "veryfast", "-crf", "23"]);
    } else {
        video_quality_args(&mut a, venc, &MediaOptions::default());
    }
    if needs_yuv420(venc) {
        a.all(["-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p"]);
    }
    if os == "macos"
        && record_audio
        && let Some(aenc) = ff.pick(fmt.audio)
    {
        a.s("-c:a").s(aenc);
    }
    a.all(fmt.mux.iter().copied());
    a.s(output);
    Ok(a.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::find;
    use crate::probe::StreamInfo;

    fn av_info() -> MediaInfo {
        MediaInfo {
            duration_secs: Some(10.0),
            streams: vec![
                StreamInfo {
                    kind: "video".into(),
                    codec: "h264".into(),
                    width: Some(1920),
                    height: Some(1080),
                    fps: Some(25.0),
                    ..Default::default()
                },
                StreamInfo {
                    kind: "audio".into(),
                    codec: "aac".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    fn strs(v: &[OsString]) -> Vec<String> {
        v.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    fn ff() -> Ffmpeg {
        Ffmpeg::fake(&[
            "libx264",
            "aac",
            "libmp3lame",
            "gif",
            "libopus",
            "libvpx-vp9",
        ])
    }

    #[test]
    fn mp4_uses_x264_crf_and_faststart() {
        let fmt = find("mp4").unwrap();
        let o = MediaOptions {
            quality: Quality::High,
            width: Some(641),
            start: Some(1.5),
            end: Some(4.0),
            ..Default::default()
        };
        let a = strs(
            &convert(
                &ff(),
                Path::new("in.mkv"),
                &av_info(),
                Path::new("o.mp4"),
                fmt,
                &o,
            )
            .unwrap(),
        );
        let j = a.join(" ");
        assert!(j.contains("-ss 1.500 -i in.mkv -t 2.500"), "{j}");
        assert!(j.contains("-c:v libx264 -preset medium -crf 18"), "{j}");
        assert!(j.contains("scale=640:-2"), "odd width rounded to even: {j}");
        assert!(j.contains("-movflags +faststart"));
        assert!(j.contains("-c:a aac"));
        assert_eq!(a.last().unwrap(), "o.mp4");
    }

    #[test]
    fn extract_audio_to_mp3() {
        let fmt = find("mp3").unwrap();
        let o = MediaOptions {
            volume_db: Some(3.0),
            speed: Some(3.0),
            ..Default::default()
        };
        let j = strs(
            &convert(
                &ff(),
                Path::new("v.mp4"),
                &av_info(),
                Path::new("o.mp3"),
                fmt,
                &o,
            )
            .unwrap(),
        )
        .join(" ");
        assert!(j.contains("-vn"));
        assert!(j.contains("-c:a libmp3lame -b:a 192k"));
        assert!(
            j.contains("atempo=2.0,atempo=1.500000,volume=3.00dB"),
            "{j}"
        );
    }

    #[test]
    fn gif_uses_palette_and_defaults() {
        let fmt = find("gif").unwrap();
        let j = strs(
            &convert(
                &ff(),
                Path::new("v.mp4"),
                &av_info(),
                Path::new("o.gif"),
                fmt,
                &MediaOptions::default(),
            )
            .unwrap(),
        )
        .join(" ");
        assert!(j.contains("palettegen"));
        assert!(j.contains("scale=480:-1"));
        assert!(j.contains("fps=12"));
        assert!(j.contains("-an"));
    }

    #[test]
    fn missing_encoder_is_an_error_not_a_panic() {
        let fmt = find("wmv").unwrap();
        let r = convert(
            &ff(),
            Path::new("v.mp4"),
            &av_info(),
            Path::new("o.wmv"),
            fmt,
            &MediaOptions::default(),
        );
        assert!(matches!(r, Err(Error::InvalidOption(_))));
    }

    #[test]
    fn audio_only_input_cannot_become_video() {
        let mut info = av_info();
        info.streams.remove(0);
        let r = convert(
            &ff(),
            Path::new("a.mp3"),
            &info,
            Path::new("o.mp4"),
            find("mp4").unwrap(),
            &MediaOptions::default(),
        );
        assert!(r.is_err());
    }

    #[test]
    fn join_generates_silence_for_silent_inputs() {
        let a = av_info();
        let mut silent = av_info();
        silent.streams.truncate(1);
        let ins = [(Path::new("a.mp4"), &a), (Path::new("b.mp4"), &silent)];
        let j = strs(
            &join(
                &ff(),
                &ins,
                Path::new("o.mp4"),
                find("mp4").unwrap(),
                &MediaOptions::default(),
            )
            .unwrap(),
        )
        .join(" ");
        assert!(j.contains("anullsrc"));
        assert!(
            j.contains("[v0][a0][v1][a1]concat=n=2:v=1:a=1[vout][aout]"),
            "{j}"
        );
        assert!(j.contains("scale=1920:1080"));
    }

    #[test]
    fn mux_copies_compatible_video() {
        let v = av_info();
        let j = strs(
            &mux(
                &ff(),
                Path::new("v.mp4"),
                &v,
                Path::new("a.mp3"),
                &v,
                Path::new("o.mp4"),
                find("mp4").unwrap(),
            )
            .unwrap(),
        )
        .join(" ");
        assert!(j.contains("-c:v copy"));
    }

    #[test]
    fn atempo_chain_bounds() {
        assert_eq!(atempo_chain(1.0), Vec::<String>::new());
        assert_eq!(atempo_chain(0.25), vec!["atempo=0.5", "atempo=0.500000"]);
        assert!(atempo_chain(f64::NAN).len() <= 9);
    }
}
