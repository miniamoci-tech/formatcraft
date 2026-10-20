//! The catalogue of output formats, grouped the way the UI shows them.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Video,
    Audio,
    Image,
    Document,
}

impl Category {
    pub const ALL: [Category; 4] = [
        Category::Video,
        Category::Audio,
        Category::Image,
        Category::Document,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Category::Video => "video",
            Category::Audio => "audio",
            Category::Image => "image",
            Category::Document => "document",
        }
    }
}

/// One output format. `video`/`audio` list encoder preferences in order;
/// the first one the local FFmpeg supports is used.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Format {
    pub id: &'static str,
    pub ext: &'static str,
    pub label: &'static str,
    pub category: Category,
    #[serde(skip)]
    pub video: &'static [&'static str],
    #[serde(skip)]
    pub audio: &'static [&'static str],
    /// Extra muxer arguments (e.g. `-f ipod` for M4R).
    #[serde(skip)]
    pub mux: &'static [&'static str],
}

const H264: &[&str] = &[
    "libx264",
    "h264_videotoolbox",
    "h264_mf",
    "h264_nvenc",
    "mpeg4",
];
const H265: &[&str] = &["libx265", "hevc_videotoolbox", "hevc_mf", "hevc_nvenc"];
const AAC: &[&str] = &["aac", "aac_at", "aac_mf"];

macro_rules! fmt {
    ($id:literal, $ext:literal, $label:literal, $cat:ident, $v:expr, $a:expr) => {
        fmt!($id, $ext, $label, $cat, $v, $a, &[])
    };
    ($id:literal, $ext:literal, $label:literal, $cat:ident, $v:expr, $a:expr, $m:expr) => {
        Format {
            id: $id,
            ext: $ext,
            label: $label,
            category: Category::$cat,
            video: $v,
            audio: $a,
            mux: $m,
        }
    };
}

pub static FORMATS: &[Format] = &[
    // Video
    fmt!(
        "mp4",
        "mp4",
        "MP4 (H.264)",
        Video,
        H264,
        AAC,
        &["-movflags", "+faststart"]
    ),
    fmt!(
        "mp4-hevc",
        "mp4",
        "MP4 (H.265/HEVC)",
        Video,
        H265,
        AAC,
        &["-movflags", "+faststart", "-tag:v", "hvc1"]
    ),
    fmt!("mkv", "mkv", "MKV", Video, H264, AAC),
    fmt!(
        "webm",
        "webm",
        "WebM (VP9)",
        Video,
        &["libvpx-vp9", "libvpx"],
        &["libopus", "libvorbis"]
    ),
    fmt!(
        "avi",
        "avi",
        "AVI",
        Video,
        &["mpeg4"],
        &["libmp3lame", "ac3"]
    ),
    fmt!("mov", "mov", "MOV", Video, H264, AAC),
    fmt!("wmv", "wmv", "WMV", Video, &["wmv2"], &["wmav2"]),
    fmt!("flv", "flv", "FLV", Video, H264, AAC),
    fmt!(
        "mpg",
        "mpg",
        "MPEG-2 (MPG)",
        Video,
        &["mpeg2video"],
        &["mp2"]
    ),
    fmt!(
        "3gp",
        "3gp",
        "3GP",
        Video,
        &["h263", "mpeg4"],
        &["aac", "libopencore_amrnb"]
    ),
    fmt!(
        "ogv",
        "ogv",
        "OGV (Theora)",
        Video,
        &["libtheora", "libvpx"],
        &["libvorbis", "libopus"]
    ),
    fmt!("ts", "ts", "MPEG-TS", Video, H264, AAC),
    fmt!("gif", "gif", "GIF (animated)", Video, &["gif"], &[]),
    fmt!(
        "webp-anim",
        "webp",
        "WebP (animated)",
        Video,
        &["libwebp_anim", "libwebp"],
        &[]
    ),
    // Audio
    fmt!("mp3", "mp3", "MP3", Audio, &[], &["libmp3lame"]),
    fmt!("aac", "aac", "AAC", Audio, &[], AAC, &["-f", "adts"]),
    fmt!("m4a", "m4a", "M4A", Audio, &[], AAC, &["-f", "ipod"]),
    fmt!(
        "m4r",
        "m4r",
        "M4R (iPhone ringtone)",
        Audio,
        &[],
        AAC,
        &["-f", "ipod", "-t", "40"]
    ),
    fmt!("wav", "wav", "WAV", Audio, &[], &["pcm_s16le"]),
    fmt!("flac", "flac", "FLAC", Audio, &[], &["flac"]),
    fmt!(
        "ogg",
        "ogg",
        "OGG (Vorbis)",
        Audio,
        &[],
        &["libvorbis", "libopus", "flac"]
    ),
    fmt!("opus", "opus", "Opus", Audio, &[], &["libopus"]),
    fmt!("wma", "wma", "WMA", Audio, &[], &["wmav2"]),
    fmt!("aiff", "aiff", "AIFF", Audio, &[], &["pcm_s16be"]),
    fmt!("ac3", "ac3", "AC3", Audio, &[], &["ac3"]),
    fmt!("mp2", "mp2", "MP2", Audio, &[], &["mp2"]),
    fmt!(
        "amr",
        "amr",
        "AMR",
        Audio,
        &[],
        &["libopencore_amrnb"],
        &["-ar", "8000", "-ac", "1"]
    ),
    fmt!("wv", "wv", "WavPack", Audio, &[], &["wavpack"]),
    // Image (converted natively, no FFmpeg)
    fmt!("jpg", "jpg", "JPG", Image, &[], &[]),
    fmt!("png", "png", "PNG", Image, &[], &[]),
    fmt!("webp", "webp", "WebP", Image, &[], &[]),
    fmt!("bmp", "bmp", "BMP", Image, &[], &[]),
    fmt!("gif-image", "gif", "GIF", Image, &[], &[]),
    fmt!("tif", "tif", "TIFF", Image, &[], &[]),
    fmt!("ico", "ico", "ICO", Image, &[], &[]),
    fmt!("tga", "tga", "TGA", Image, &[], &[]),
    fmt!("qoi", "qoi", "QOI", Image, &[], &[]),
    fmt!("pnm", "ppm", "PPM/PNM", Image, &[], &[]),
    // Document
    fmt!("pdf", "pdf", "PDF", Document, &[], &[]),
];

pub fn find(id: &str) -> Option<&'static Format> {
    let id = id.trim().to_ascii_lowercase();
    FORMATS.iter().find(|f| f.id == id)
}

pub fn in_category(cat: Category) -> impl Iterator<Item = &'static Format> {
    FORMATS.iter().filter(move |f| f.category == cat)
}

/// Extensions accepted as input for each category (used by file pickers).
pub fn input_extensions(cat: Category) -> &'static [&'static str] {
    match cat {
        Category::Video => &[
            "mp4", "mkv", "webm", "avi", "mov", "wmv", "flv", "mpg", "mpeg", "m4v", "3gp", "ogv",
            "ts", "mts", "m2ts", "vob", "gif", "rm", "rmvb", "asf", "f4v", "divx", "dv",
        ],
        Category::Audio => &[
            "mp3", "cda", "vob", "aac", "m4a", "m4r", "wav", "flac", "ogg", "oga", "opus", "wma",
            "aiff", "aif", "ac3", "mp2", "amr", "wv", "ape", "mka", "dts", "mp4", "mkv", "webm",
            "avi", "mov", "wmv", "flv", "mpg",
        ],
        Category::Image => &[
            "jpg", "jpeg", "png", "webp", "bmp", "gif", "tif", "tiff", "ico", "tga", "qoi", "ppm",
            "pgm", "pbm", "pnm", "hdr", "exr", "dds",
        ],
        Category::Document => &[
            "pdf", "jpg", "jpeg", "png", "bmp", "webp", "tif", "tiff", "gif",
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_findable() {
        let mut seen = std::collections::HashSet::new();
        for f in FORMATS {
            assert!(seen.insert(f.id), "duplicate id {}", f.id);
            assert_eq!(find(f.id).map(|x| x.id), Some(f.id));
        }
        assert!(find("MP4 ").is_some());
        assert!(find("nope").is_none());
    }

    #[test]
    fn video_and_audio_formats_have_encoders() {
        for f in FORMATS {
            match f.category {
                Category::Video => assert!(!f.video.is_empty(), "{}", f.id),
                Category::Audio => assert!(!f.audio.is_empty(), "{}", f.id),
                _ => {}
            }
        }
    }
}
