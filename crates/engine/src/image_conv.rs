//! Native image conversion through the `image` crate. Inputs the crate cannot
//! decode (HEIC, AVIF, PSD, RAW, ...) are first decoded by FFmpeg when present.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader, Limits};

use crate::error::{Error, Result, io_at};
use crate::ffmpeg::{Ffmpeg, command};
use crate::formats::{Category, Format};
use crate::job::{ImageOptions, Rotation};

/// Upper bound for decoded images (bytes); protects against decompression bombs.
const MAX_ALLOC: u64 = 1024 * 1024 * 1024;
const MAX_DIM: u32 = 32_768;

fn limits() -> Limits {
    let mut l = Limits::default();
    l.max_alloc = Some(MAX_ALLOC);
    l.max_image_width = Some(MAX_DIM);
    l.max_image_height = Some(MAX_DIM);
    l
}

pub fn load(path: &Path) -> Result<DynamicImage> {
    let native = (|| -> Result<DynamicImage> {
        let mut r = ImageReader::open(path)
            .map_err(io_at(path))?
            .with_guessed_format()
            .map_err(io_at(path))?;
        r.limits(limits());
        Ok(r.decode()?)
    })();
    match native {
        Ok(img) => Ok(img),
        Err(Error::Image(native_err)) => {
            decode_with_ffmpeg(path).map_err(|_| Error::Image(native_err))
        }
        Err(e) => Err(e),
    }
}

fn decode_with_ffmpeg(path: &Path) -> Result<DynamicImage> {
    let ff = Ffmpeg::get()?;
    let dir = tempfile_dir()?;
    let tmp = dir.join("frame.png");
    let out = command(&ff.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(path)
        .args(["-frames:v", "1"])
        .arg(&tmp)
        .output()?;
    let result = if out.status.success() {
        let mut r = ImageReader::open(&tmp).map_err(io_at(&tmp))?;
        r.limits(limits());
        r.decode().map_err(Error::from)
    } else {
        Err(Error::ToolFailed {
            tool: "ffmpeg",
            code: out.status.code(),
            message: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        })
    };
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn tempfile_dir() -> Result<std::path::PathBuf> {
    let base = std::env::temp_dir();
    for n in 0..1000u32 {
        let d = base.join(format!("formatcraft-{}-{n}", std::process::id()));
        if std::fs::create_dir(&d).is_ok() {
            return Ok(d);
        }
    }
    Err(Error::Internal(
        "could not create a temporary folder".into(),
    ))
}

pub fn transform(mut img: DynamicImage, o: &ImageOptions) -> DynamicImage {
    img = match o.rotate {
        Rotation::None => img,
        Rotation::Cw90 => img.rotate90(),
        Rotation::Ccw90 => img.rotate270(),
        Rotation::R180 => img.rotate180(),
        Rotation::FlipH => img.fliph(),
        Rotation::FlipV => img.flipv(),
    };
    let (w, h) = (img.width().max(1), img.height().max(1));
    let target = match (o.width, o.height, o.scale_percent) {
        (Some(tw), Some(th), _) => Some((tw, th)),
        (Some(tw), None, _) => Some((tw, scale_dim(h, tw, w))),
        (None, Some(th), _) => Some((scale_dim(w, th, h), th)),
        (None, None, Some(p)) if p != 100 => Some((scale_dim(w, p, 100), scale_dim(h, p, 100))),
        _ => None,
    };
    if let Some((tw, th)) = target {
        let (tw, th) = (tw.clamp(1, MAX_DIM), th.clamp(1, MAX_DIM));
        if (tw, th) != (w, h) {
            img = img.resize_exact(tw, th, FilterType::Lanczos3);
        }
    }
    if o.grayscale {
        img = img.grayscale();
    }
    img
}

/// `v * num / den` rounded to nearest in u64, clamped to at least 1.
fn scale_dim(v: u32, num: u32, den: u32) -> u32 {
    let den = u64::from(den.max(1));
    let r = u64::from(v)
        .saturating_mul(u64::from(num))
        .saturating_add(den / 2)
        / den;
    u32::try_from(r).unwrap_or(MAX_DIM).max(1)
}

/// Converts `input` to `output` (written via a temp file, then renamed).
pub fn convert(
    input: &Path,
    output: &Path,
    fmt: &Format,
    o: &ImageOptions,
) -> Result<std::path::PathBuf> {
    if fmt.category != Category::Image {
        return Err(Error::WrongCategory {
            format: fmt.id.into(),
            what: "image conversion",
        });
    }
    let img = transform(load(input)?, o);
    crate::atomic::write_with(output, |tmp| save(&img, tmp, fmt, o))
}

pub fn save(img: &DynamicImage, path: &Path, fmt: &Format, o: &ImageOptions) -> Result<()> {
    match fmt.id {
        "jpg" => {
            let q = o.jpeg_quality.unwrap_or(90).clamp(1, 100);
            let f = File::create(path).map_err(io_at(path))?;
            let mut enc = JpegEncoder::new_with_quality(BufWriter::new(f), q);
            enc.encode_image(&DynamicImage::ImageRgb8(img.to_rgb8()))?;
            Ok(())
        }
        "ico" => {
            let small = if img.width() > 256 || img.height() > 256 {
                img.resize(256, 256, FilterType::Lanczos3)
            } else {
                img.clone()
            };
            DynamicImage::ImageRgba8(small.to_rgba8()).save_with_format(path, ImageFormat::Ico)?;
            Ok(())
        }
        "bmp" | "tga" => {
            let f = if fmt.id == "bmp" {
                ImageFormat::Bmp
            } else {
                ImageFormat::Tga
            };
            DynamicImage::ImageRgba8(img.to_rgba8()).save_with_format(path, f)?;
            Ok(())
        }
        "pnm" => {
            // PPM has no alpha channel.
            DynamicImage::ImageRgb8(img.to_rgb8()).save_with_format(path, ImageFormat::Pnm)?;
            Ok(())
        }
        "gif-image" => {
            DynamicImage::ImageRgba8(img.to_rgba8()).save_with_format(path, ImageFormat::Gif)?;
            Ok(())
        }
        "webp" => {
            DynamicImage::ImageRgba8(img.to_rgba8()).save_with_format(path, ImageFormat::WebP)?;
            Ok(())
        }
        "png" => Ok(img.save_with_format(path, ImageFormat::Png)?),
        "tif" => Ok(img.save_with_format(path, ImageFormat::Tiff)?),
        "qoi" => {
            DynamicImage::ImageRgba8(img.to_rgba8()).save_with_format(path, ImageFormat::Qoi)?;
            Ok(())
        }
        other => Err(Error::UnknownFormat(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::{FORMATS, find};
    use image::RgbaImage;

    #[test]
    fn converts_to_every_image_format() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src.png");
        RgbaImage::from_fn(300, 200, |x, y| image::Rgba([x as u8, y as u8, 128, 200]))
            .save(&src)
            .unwrap();
        for f in FORMATS.iter().filter(|f| f.category == Category::Image) {
            let out = d.path().join(format!("out-{}.{}", f.id, f.ext));
            let o = ImageOptions {
                width: Some(100),
                ..Default::default()
            };
            let out = convert(&src, &out, f, &o).unwrap_or_else(|e| panic!("{}: {e}", f.id));
            let back = load(&out).unwrap();
            assert_eq!(back.width(), 100, "{}", f.id);
            assert_eq!(back.height(), 67, "{}", f.id);
        }
    }

    #[test]
    fn transform_handles_extremes() {
        let img = DynamicImage::ImageRgba8(RgbaImage::new(3, 5));
        let o = ImageOptions {
            rotate: Rotation::Cw90,
            scale_percent: Some(0),
            ..Default::default()
        };
        let r = transform(img, &o);
        assert_eq!((r.width(), r.height()), (1, 1));
        assert_eq!(scale_dim(u32::MAX, u32::MAX, 1), MAX_DIM);
    }

    #[test]
    fn garbage_input_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bad.png");
        std::fs::write(&p, b"\x89PNG\r\n\x1a\nnot really").unwrap();
        let out = d.path().join("o.jpg");
        assert!(convert(&p, &out, find("jpg").unwrap(), &ImageOptions::default()).is_err());
        assert!(!out.exists(), "failed conversions leave no output");
    }
}
