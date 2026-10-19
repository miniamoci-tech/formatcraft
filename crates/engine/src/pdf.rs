//! PDF utilities on top of `lopdf` (MIT): merge, split, compress, images to PDF.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::DynamicImage;
use image::codecs::jpeg::JpegEncoder;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};

use crate::error::{Error, Result, io_at};

const MAX_PAGES: usize = 20_000;
const MAX_TREE_DEPTH: usize = 64;
/// PDF user-space limit for page sizes (points).
const MAX_PAGE_PT: f64 = 14_400.0;
const INHERITABLE: [&[u8]; 4] = [b"MediaBox", b"CropBox", b"Resources", b"Rotate"];

fn load(path: &Path) -> Result<Document> {
    if !path.is_file() {
        return Err(Error::MissingInput(path.to_path_buf()));
    }
    let doc = Document::load(path).map_err(|e| Error::Pdf(format!("{}: {e}", path.display())))?;
    if doc.is_encrypted() {
        return Err(Error::Pdf(format!(
            "{} is password-protected; remove the password first",
            path.display()
        )));
    }
    Ok(doc)
}

/// Copies attributes a page inherits from its ancestors into the page itself,
/// so the page keeps them after being re-parented.
fn flatten_inherited(doc: &Document, page_id: ObjectId) -> Option<Dictionary> {
    let mut page = doc.get_dictionary(page_id).ok()?.clone();
    let mut parent = page.get(b"Parent").and_then(Object::as_reference).ok();
    let mut depth = 0;
    while let Some(pid) = parent {
        depth += 1;
        if depth > MAX_TREE_DEPTH {
            break;
        }
        let Ok(node) = doc.get_dictionary(pid) else {
            break;
        };
        for key in INHERITABLE {
            if !page.has(key)
                && let Ok(v) = node.get(key)
            {
                page.set(key, v.clone());
            }
        }
        parent = node.get(b"Parent").and_then(Object::as_reference).ok();
    }
    if !page.has(b"MediaBox") {
        // US Letter is the spec's conventional fallback.
        page.set("MediaBox", vec![0.into(), 0.into(), 612.into(), 792.into()]);
    }
    Some(page)
}

fn is_type(obj: &Object, name: &[u8]) -> bool {
    obj.as_dict()
        .ok()
        .and_then(|d| d.get(b"Type").ok())
        .and_then(|t| t.as_name().ok())
        .is_some_and(|n| n == name)
}

/// Builds a document whose page tree holds `pages` (already present in
/// `doc.objects`) in order.
fn finish_tree(doc: &mut Document, pages: &[ObjectId]) {
    let pages_id = (doc.max_id.saturating_add(1), 0);
    let catalog_id = (doc.max_id.saturating_add(2), 0);
    doc.max_id = doc.max_id.saturating_add(2);
    for id in pages {
        if let Some(Object::Dictionary(d)) = doc.objects.get_mut(id) {
            d.set("Parent", pages_id);
        }
    }
    let kids: Vec<Object> = pages.iter().map(|id| Object::Reference(*id)).collect();
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => i64::try_from(pages.len()).unwrap_or(i64::MAX),
        }),
    );
    doc.objects.insert(
        catalog_id,
        Object::Dictionary(dictionary! { "Type" => "Catalog", "Pages" => pages_id }),
    );
    doc.trailer = Dictionary::new();
    doc.trailer.set("Root", catalog_id);
}

fn save(mut doc: Document, path: &Path) -> Result<()> {
    doc.prune_objects();
    doc.compress();
    doc.save(path).map_err(io_at(path))?;
    Ok(())
}

pub fn merge(inputs: &[PathBuf], output: &Path) -> Result<PathBuf> {
    if inputs.len() < 2 {
        return Err(Error::TooFewInputs {
            what: "PDF merge",
            min: 2,
            got: inputs.len(),
        });
    }
    let mut out = Document::with_version("1.7");
    let mut all_pages = Vec::new();
    let mut next_id = 1u32;
    for path in inputs {
        let mut doc = load(path)?;
        doc.renumber_objects_with(next_id);
        next_id = doc.max_id.saturating_add(1);
        let page_ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
        if all_pages.len().saturating_add(page_ids.len()) > MAX_PAGES {
            return Err(Error::Pdf(format!("more than {MAX_PAGES} pages")));
        }
        let flattened: Vec<(ObjectId, Dictionary)> = page_ids
            .iter()
            .filter_map(|id| flatten_inherited(&doc, *id).map(|d| (*id, d)))
            .collect();
        for (id, obj) in std::mem::take(&mut doc.objects) {
            if is_type(&obj, b"Catalog") || is_type(&obj, b"Pages") {
                continue;
            }
            out.objects.insert(id, obj);
        }
        for (id, dict) in flattened {
            out.objects.insert(id, Object::Dictionary(dict));
            all_pages.push(id);
        }
        out.max_id = out.max_id.max(doc.max_id);
    }
    finish_tree(&mut out, &all_pages);
    crate::atomic::write_with(output, |tmp| save(out, tmp))
}

/// One PDF per page in a new folder.
pub fn split(input: &Path, output_dir: &Path) -> Result<PathBuf> {
    let doc = load(input)?;
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "page".into());
    let pages: Vec<u32> = doc.get_pages().into_keys().collect();
    if pages.len() > MAX_PAGES {
        return Err(Error::Pdf(format!("more than {MAX_PAGES} pages")));
    }
    crate::atomic::write_dir_with(output_dir, |dir| {
        let width = pages.len().to_string().len().max(2);
        for n in &pages {
            let mut one = doc.clone();
            let others: Vec<u32> = pages.iter().copied().filter(|p| p != n).collect();
            one.delete_pages(&others);
            let path = dir.join(format!("{stem}_{n:0width$}.pdf"));
            save(one, &path)?;
        }
        Ok(())
    })
}

pub fn compress(input: &Path, output: &Path) -> Result<PathBuf> {
    let doc = load(input)?;
    crate::atomic::write_with(output, |tmp| save(doc, tmp))
}

fn image_xobject(img: &DynamicImage, as_jpeg: bool) -> Result<(Stream, Option<Stream>)> {
    let (w, h) = (i64::from(img.width()), i64::from(img.height()));
    if as_jpeg {
        let mut buf = Vec::new();
        JpegEncoder::new_with_quality(Cursor::new(&mut buf), 90)
            .encode_image(&DynamicImage::ImageRgb8(img.to_rgb8()))?;
        let dict = dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => w, "Height" => h,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "DCTDecode",
        };
        return Ok((Stream::new(dict, buf), None));
    }
    let rgba = img.to_rgba8();
    let has_alpha = rgba.pixels().any(|p| p.0[3] != 255);
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    let mut alpha = Vec::new();
    for p in rgba.pixels() {
        let [r, g, b, a] = p.0;
        rgb.extend_from_slice(&[r, g, b]);
        if has_alpha {
            alpha.push(a);
        }
    }
    let mut main = Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => w, "Height" => h,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
        },
        rgb,
    );
    // compress() only fails on encoder errors; uncompressed output is still valid.
    let _ = main.compress();
    let mask = if has_alpha {
        let mut s = Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => w, "Height" => h,
                "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            },
            alpha,
        );
        let _ = s.compress();
        Some(s)
    } else {
        None
    };
    Ok((main, mask))
}

fn is_jpeg(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 3];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_ok()
        && magic == [0xFF, 0xD8, 0xFF]
}

/// One page per image, each page sized to its image at 96 dpi.
pub fn images_to_pdf(inputs: &[PathBuf], output: &Path) -> Result<PathBuf> {
    if inputs.is_empty() {
        return Err(Error::TooFewInputs {
            what: "images to PDF",
            min: 1,
            got: 0,
        });
    }
    let mut doc = Document::with_version("1.7");
    let mut pages = Vec::new();
    for path in inputs.iter().take(MAX_PAGES) {
        let img = crate::image_conv::load(path)?;
        let (xobj, mask) = image_xobject(&img, is_jpeg(path))?;
        let mut xobj = xobj;
        if let Some(mask) = mask {
            let mask_id = doc.add_object(mask);
            xobj.dict.set("SMask", mask_id);
        }
        let img_id = doc.add_object(xobj);
        let mut pw = f64::from(img.width()) * 0.75;
        let mut ph = f64::from(img.height()) * 0.75;
        let big = pw.max(ph);
        if big > MAX_PAGE_PT {
            pw = pw * MAX_PAGE_PT / big;
            ph = ph * MAX_PAGE_PT / big;
        }
        let (pw, ph) = (pw.max(1.0), ph.max(1.0));
        let content = format!("q {pw:.3} 0 0 {ph:.3} 0 0 cm /Im0 Do Q");
        let content_id = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "MediaBox" => vec![0.into(), 0.into(), Object::Real(pw as f32), Object::Real(ph as f32)],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => img_id } },
            "Contents" => content_id,
        });
        pages.push(page_id);
    }
    finish_tree(&mut doc, &pages);
    crate::atomic::write_with(output, |tmp| save(doc, tmp))
}

/// Page count, for reporting.
pub fn page_count(path: &Path) -> Result<usize> {
    Ok(load(path)?.get_pages().len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    fn make_images(dir: &Path) -> Vec<PathBuf> {
        let a = dir.join("a.png");
        RgbaImage::from_fn(40, 20, |x, _| image::Rgba([x as u8, 0, 0, 128]))
            .save(&a)
            .unwrap();
        let b = dir.join("b.jpg");
        image::RgbImage::from_pixel(30, 60, image::Rgb([0, 200, 0]))
            .save(&b)
            .unwrap();
        vec![a, b]
    }

    #[test]
    fn images_merge_split_compress_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let imgs = make_images(d.path());
        let one = images_to_pdf(&imgs, &d.path().join("one.pdf")).unwrap();
        assert_eq!(page_count(&one).unwrap(), 2);
        let two = images_to_pdf(&imgs[..1], &d.path().join("two.pdf")).unwrap();
        let merged = merge(&[one.clone(), two], &d.path().join("m.pdf")).unwrap();
        assert_eq!(page_count(&merged).unwrap(), 3);
        // Page sizes survive the re-parenting.
        let doc = Document::load(&merged).unwrap();
        for id in doc.get_pages().values() {
            assert!(doc.get_dictionary(*id).unwrap().has(b"MediaBox"));
        }
        let parts = split(&merged, &d.path().join("parts")).unwrap();
        let n = std::fs::read_dir(&parts).unwrap().count();
        assert_eq!(n, 3);
        let c = compress(&one, &d.path().join("c.pdf")).unwrap();
        assert_eq!(page_count(&c).unwrap(), 2);
    }

    #[test]
    fn bad_pdf_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bad.pdf");
        std::fs::write(&p, b"%PDF-1.7\nnonsense").unwrap();
        assert!(compress(&p, &d.path().join("o.pdf")).is_err());
        assert!(merge(std::slice::from_ref(&p), &d.path().join("o.pdf")).is_err());
    }
}
