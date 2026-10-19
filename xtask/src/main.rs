//! Build tasks: `cargo xtask <command>`.
//!
//! - `icon`            render the app icon (contributor-original, drawn in code)
//! - `iconset DIR`     write a macOS .iconset folder (for `iconutil`)
//! - `dist-windows`    release build + FFmpeg + Inno Setup installer -> dist/
//! - `dist-macos TRIPLE` release build + .app bundle + FFmpeg + DMG -> dist/
//! - `assets`          check that every asset file is listed in ATTRIBUTION.toml

use std::path::{Path, PathBuf};
use std::process::Command;

use image::imageops::FilterType;
use image::{Rgba, RgbaImage};
use sha2::Digest;

type R<T> = Result<T, String>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("icon") => icon(),
        Some("iconset") => iconset(args.get(1).map(PathBuf::from)),
        Some("dist-windows") => dist_windows(),
        Some("dist-macos") => dist_macos(args.get(1).cloned()),
        Some("assets") => assets(),
        _ => Err(
            "usage: cargo xtask icon | iconset DIR | dist-windows | dist-macos TRIPLE | assets"
                .into(),
        ),
    };
    if let Err(e) = r {
        eprintln!("xtask: {e}");
        std::process::exit(1);
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn run(cmd: &mut Command) -> R<()> {
    eprintln!("$ {cmd:?}");
    let st = cmd.status().map_err(|e| format!("{cmd:?}: {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("{cmd:?} failed with {st}"))
    }
}

fn version() -> String {
    let toml = std::fs::read_to_string(root().join("Cargo.toml")).unwrap_or_default();
    toml.lines()
        .skip_while(|l| !l.starts_with("[workspace.package]"))
        .find_map(|l| l.strip_prefix("version = "))
        .map(|v| v.trim_matches('"').to_string())
        .unwrap_or_else(|| "0.0.0".into())
}

// ---------------------------------------------------------------- icon

fn lerp(a: u8, b: u8, t: f32) -> f32 {
    f32::from(a) + (f32::from(b) - f32::from(a)) * t
}

fn rounded_rect_sd(x: f32, y: f32, half: f32, r: f32) -> f32 {
    let qx = x.abs() - half + r;
    let qy = y.abs() - half + r;
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

fn in_triangle(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let s = |p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)| {
        (p1.0 - p3.0) * (p2.1 - p3.1) - (p2.0 - p3.0) * (p1.1 - p3.1)
    };
    let d1 = s(p, a, b);
    let d2 = s(p, b, c);
    let d3 = s(p, c, a);
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// Sample of the icon at (x, y) in [-0.5, 0.5]^2, y up. Returns
/// (background coverage, white foreground coverage).
fn sample(x: f32, y: f32) -> (bool, bool) {
    let bg = rounded_rect_sd(x, y, 0.44, 0.1) <= 0.0;
    if !bg {
        return (false, false);
    }
    let r = (x * x + y * y).sqrt();
    let ang = y.atan2(x).to_degrees().rem_euclid(360.0);
    let ring = (r - 0.27).abs() < 0.04
        && ((25.0..=160.0).contains(&ang) || (205.0..=340.0).contains(&ang));
    let mut arrow = false;
    for end in [160.0f32, 340.0] {
        let t = end.to_radians();
        let p = (0.27 * t.cos(), 0.27 * t.sin());
        let tan = (-t.sin(), t.cos());
        let n = (t.cos(), t.sin());
        let tip = (p.0 + tan.0 * 0.11, p.1 + tan.1 * 0.11);
        let a = (p.0 + n.0 * 0.095, p.1 + n.1 * 0.095);
        let b = (p.0 - n.0 * 0.095, p.1 - n.1 * 0.095);
        arrow |= in_triangle((x, y), tip, a, b);
    }
    let play = in_triangle((x, y), (-0.07, 0.11), (-0.07, -0.11), (0.12, 0.0));
    (true, ring || arrow || play)
}

fn render_icon(size: u32) -> RgbaImage {
    const SS: u32 = 4;
    let top = (0x2E, 0x8B, 0xC0);
    let bottom = (0x5B, 0x3F, 0xB0);
    let mut img = RgbaImage::new(size, size);
    for py in 0..size {
        for px in 0..size {
            let (mut cov_bg, mut cov_fg) = (0u32, 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (px as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32 - 0.5;
                    let y = 0.5 - (py as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    let (b, f) = sample(x, y);
                    cov_bg += u32::from(b);
                    cov_fg += u32::from(f);
                }
            }
            let n = (SS * SS) as f32;
            let a_bg = cov_bg as f32 / n;
            let a_fg = cov_fg as f32 / n;
            let t = ((px + py) as f32 / (2.0 * size as f32)).clamp(0.0, 1.0);
            let base = [
                lerp(top.0, bottom.0, t),
                lerp(top.1, bottom.1, t),
                lerp(top.2, bottom.2, t),
            ];
            let fg_share = if a_bg > 0.0 { a_fg / a_bg } else { 0.0 };
            let c = |i: usize| (base[i] * (1.0 - fg_share) + 255.0 * fg_share).round() as u8;
            img.put_pixel(
                px,
                py,
                Rgba([c(0), c(1), c(2), (a_bg * 255.0).round() as u8]),
            );
        }
    }
    img
}

fn icon() -> R<()> {
    let dir = root().join("assets").join("icon");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let big = render_icon(1024);
    big.save(dir.join("formatcraft-1024.png"))
        .map_err(|e| e.to_string())?;
    let s256 = image::imageops::resize(&big, 256, 256, FilterType::Lanczos3);
    s256.save(dir.join("formatcraft-256.png"))
        .map_err(|e| e.to_string())?;
    // ICO: the encoder takes one frame; Windows scales the 256 px frame.
    s256.save_with_format(dir.join("formatcraft.ico"), image::ImageFormat::Ico)
        .map_err(|e| e.to_string())?;
    eprintln!("icon written to {}", dir.display());
    Ok(())
}

fn iconset(dir: Option<PathBuf>) -> R<()> {
    let dir = dir.ok_or("iconset needs a DIR (ending in .iconset)")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let src = image::open(root().join("assets/icon/formatcraft-1024.png"))
        .map_err(|e| e.to_string())?
        .to_rgba8();
    for s in [16u32, 32, 128, 256, 512] {
        for (scale, suffix) in [(1, ""), (2, "@2x")] {
            let px = s * scale;
            let img = image::imageops::resize(&src, px, px, FilterType::Lanczos3);
            img.save(dir.join(format!("icon_{s}x{s}{suffix}.png")))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- dist

fn download(url: &str, to: &Path) -> R<()> {
    run(Command::new("curl")
        .args(["-fL", "--retry", "3", "-o"])
        .arg(to)
        .arg(url))
}

fn copy(from: &Path, to: &Path) -> R<()> {
    if let Some(p) = to.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("copy {} -> {}: {e}", from.display(), to.display()))
}

fn find_file(dir: &Path, name: &str, depth: u32) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.is_file() && p.file_name().is_some_and(|n| n == name) {
            return Some(p);
        }
        if p.is_dir() {
            subdirs.push(p);
        }
    }
    if depth == 0 {
        return None;
    }
    subdirs
        .into_iter()
        .find_map(|d| find_file(&d, name, depth - 1))
}

fn stage_common(stage: &Path) -> R<()> {
    let r = root();
    for f in ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "README.md"] {
        copy(&r.join(f), &stage.join(f))?;
    }
    Ok(())
}

fn dist_windows() -> R<()> {
    let r = root();
    run(Command::new("cargo").current_dir(&r).args([
        "build",
        "--release",
        "-p",
        "formatcraft",
        "-p",
        "formatcraft-cli",
    ]))?;
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| r.join("target"));
    let stage = r.join("dist/windows/FormatCraft");
    let _ = std::fs::remove_dir_all(&stage);
    copy(
        &target.join("release/formatcraft.exe"),
        &stage.join("formatcraft.exe"),
    )?;
    copy(
        &target.join("release/formatcraft-cli.exe"),
        &stage.join("formatcraft-cli.exe"),
    )?;
    stage_common(&stage)?;

    // FFmpeg (GPL build, shipped as a separate program with its licence).
    let url = std::env::var("FFMPEG_WIN_URL").unwrap_or_else(|_| {
        "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip".into()
    });
    let dl = r.join("dist/downloads");
    std::fs::create_dir_all(&dl).map_err(|e| e.to_string())?;
    let zip = dl.join("ffmpeg-win64.zip");
    if !zip.is_file() {
        download(&url, &zip)?;
    }
    let ex = dl.join("ffmpeg-win64");
    let _ = std::fs::remove_dir_all(&ex);
    std::fs::create_dir_all(&ex).map_err(|e| e.to_string())?;
    run(Command::new("tar").arg("-xf").arg(&zip).arg("-C").arg(&ex))?;
    for exe in ["ffmpeg.exe", "ffprobe.exe"] {
        let f = find_file(&ex, exe, 4).ok_or(format!("{exe} not in the FFmpeg archive"))?;
        copy(&f, &stage.join("ffmpeg").join(exe))?;
    }
    if let Some(lic) = find_file(&ex, "LICENSE.txt", 3) {
        copy(&lic, &stage.join("ffmpeg/LICENSE.txt"))?;
    }
    std::fs::write(
        stage.join("ffmpeg/SOURCE.txt"),
        format!("FFmpeg binaries from {url}\nSource code: https://ffmpeg.org/download.html and https://github.com/BtbN/FFmpeg-Builds\nFFmpeg is licensed under the GPL; it runs as a separate program.\n"),
    )
    .map_err(|e| e.to_string())?;

    let iscc = find_iscc().ok_or("Inno Setup (ISCC.exe) not found; install Inno Setup 6")?;
    run(Command::new(iscc)
        .current_dir(&r)
        .arg(format!("/DAppVersion={}", version()))
        .arg(format!("/DStageDir={}", stage.display()))
        .arg(format!("/DOutDir={}", r.join("dist").display()))
        .arg(r.join("installer/windows/formatcraft.iss")))?;
    eprintln!(
        "installer: dist/FormatCraft-{}-windows-x64-setup.exe",
        version()
    );
    Ok(())
}

fn find_iscc() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ISCC") {
        return Some(PathBuf::from(p));
    }
    let mut c = Vec::new();
    for var in ["LOCALAPPDATA", "ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(base) = std::env::var_os(var) {
            let base = PathBuf::from(base);
            c.push(base.join("Programs/Inno Setup 6/ISCC.exe"));
            c.push(base.join("Inno Setup 6/ISCC.exe"));
        }
    }
    c.into_iter().find(|p| p.is_file())
}

fn dist_macos(triple: Option<String>) -> R<()> {
    let triple = triple.unwrap_or_else(|| "aarch64-apple-darwin".into());
    let arch = if triple.starts_with("x86_64") {
        "x86_64"
    } else {
        "arm64"
    };
    let r = root();
    run(Command::new("cargo").current_dir(&r).args([
        "build",
        "--release",
        "--target",
        &triple,
        "-p",
        "formatcraft",
        "-p",
        "formatcraft-cli",
    ]))?;
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| r.join("target"));
    let bin = target.join(&triple).join("release");
    let out = r.join("dist/macos").join(arch);
    let _ = std::fs::remove_dir_all(&out);
    let app = out.join("FormatCraft.app/Contents");
    copy(&bin.join("formatcraft"), &app.join("MacOS/formatcraft"))?;
    copy(
        &bin.join("formatcraft-cli"),
        &app.join("MacOS/formatcraft-cli"),
    )?;
    stage_common(&app.join("Resources"))?;
    let v = version();
    let plist = include_str!("../../installer/macos/Info.plist").replace("@VERSION@", &v);
    std::fs::write(app.join("Info.plist"), plist).map_err(|e| e.to_string())?;

    let iconset_dir = out.join("AppIcon.iconset");
    iconset(Some(iconset_dir.clone()))?;
    run(Command::new("iconutil")
        .args(["-c", "icns", "-o"])
        .arg(app.join("Resources/AppIcon.icns"))
        .arg(&iconset_dir))?;
    let _ = std::fs::remove_dir_all(&iconset_dir);

    // FFmpeg static builds (GPL), shipped as separate programs.
    let ff_arch = if arch == "x86_64" { "amd64" } else { "arm64" };
    let dl = r.join("dist/downloads");
    std::fs::create_dir_all(&dl).map_err(|e| e.to_string())?;
    let mut bundled = true;
    for tool in ["ffmpeg", "ffprobe"] {
        let var = format!("FFMPEG_MAC_{}_URL", tool.to_uppercase());
        let url = std::env::var(&var).unwrap_or_else(|_| {
            format!(
                "https://ffmpeg.martin-riedl.de/redirect/latest/macos/{ff_arch}/release/{tool}.zip"
            )
        });
        let zip = dl.join(format!("{tool}-mac-{ff_arch}.zip"));
        if download(&url, &zip).is_err() {
            eprintln!(
                "warning: could not download {tool}; the app will use a system FFmpeg (Homebrew)"
            );
            bundled = false;
            continue;
        }
        let ex = dl.join(format!("{tool}-mac-{ff_arch}"));
        let _ = std::fs::remove_dir_all(&ex);
        std::fs::create_dir_all(&ex).map_err(|e| e.to_string())?;
        run(Command::new("unzip").arg("-o").arg(&zip).arg("-d").arg(&ex))?;
        let f = find_file(&ex, tool, 3).ok_or(format!("{tool} not in archive"))?;
        let dest = app.join("Resources/ffmpeg").join(tool);
        copy(&f, &dest)?;
        run(Command::new("chmod").arg("+x").arg(&dest))?;
    }
    if bundled {
        std::fs::write(
            app.join("Resources/ffmpeg/SOURCE.txt"),
            "FFmpeg static builds from https://ffmpeg.martin-riedl.de\nSource code: https://ffmpeg.org/download.html\nFFmpeg is licensed under the GPL; it runs as a separate program.\n",
        )
        .map_err(|e| e.to_string())?;
    }

    // Ad-hoc signature (required on Apple silicon). Not notarised.
    run(Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(out.join("FormatCraft.app")))?;

    let dmg_root = out.join("dmg");
    std::fs::create_dir_all(&dmg_root).map_err(|e| e.to_string())?;
    run(Command::new("cp")
        .arg("-R")
        .arg(out.join("FormatCraft.app"))
        .arg(&dmg_root))?;
    run(Command::new("ln")
        .args(["-s", "/Applications"])
        .arg(dmg_root.join("Applications")))?;
    let dmg = r
        .join("dist")
        .join(format!("FormatCraft-{v}-macos-{arch}.dmg"));
    let _ = std::fs::remove_file(&dmg);
    run(Command::new("hdiutil")
        .args([
            "create",
            "-volname",
            "FormatCraft",
            "-fs",
            "HFS+",
            "-format",
            "UDZO",
            "-srcfolder",
        ])
        .arg(&dmg_root)
        .arg(&dmg))?;
    eprintln!("dmg: {}", dmg.display());
    Ok(())
}

// ---------------------------------------------------------------- assets

/// Every file under assets/ must have an entry (by path) in ATTRIBUTION.toml.
fn assets() -> R<()> {
    let r = root();
    let attribution =
        std::fs::read_to_string(r.join("ATTRIBUTION.toml")).map_err(|e| e.to_string())?;
    let mut missing = Vec::new();
    let mut stack = vec![r.join("assets")];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).map_err(|e| e.to_string())?.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p
                    .strip_prefix(&r)
                    .map(|x| x.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                if !attribution.contains(&format!("path = \"{rel}\"")) {
                    missing.push(rel);
                }
            }
        }
    }
    // Hashes: each [[asset]] block's path must match its sha256.
    for block in attribution.split("[[asset]]").skip(1) {
        let field = |k: &str| {
            block.lines().find_map(|l| {
                l.trim()
                    .strip_prefix(&format!("{k} = \""))
                    .and_then(|v| v.strip_suffix('"'))
                    .map(str::to_string)
            })
        };
        let (Some(path), Some(want)) = (field("path"), field("sha256")) else {
            return Err("an [[asset]] entry lacks path or sha256".into());
        };
        let bytes = std::fs::read(r.join(&path)).map_err(|e| format!("{path}: {e}"))?;
        let got: String = sha2::Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if got != want {
            return Err(format!(
                "{path}: sha256 {got} does not match ATTRIBUTION.toml"
            ));
        }
        if field("license").is_none() || field("author").is_none() {
            return Err(format!("{path}: missing license or author"));
        }
    }
    let lower = attribution.to_ascii_lowercase();
    if lower.contains("author = \"adobe") {
        return Err("ATTRIBUTION.toml lists an Adobe-authored asset".into());
    }
    if missing.is_empty() {
        eprintln!("assets: ok");
        Ok(())
    } else {
        Err(format!(
            "assets without an ATTRIBUTION.toml entry: {missing:?}"
        ))
    }
}
