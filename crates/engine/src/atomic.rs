//! Atomic outputs: work happens in a temporary sibling, which is renamed into
//! place only on success. A failed or cancelled job leaves nothing behind and
//! never replaces an existing file.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result, io_at};
use crate::job::unique_path;

fn split_name(path: &Path) -> (PathBuf, String, Option<String>) {
    let dir = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".into());
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned());
    (dir, stem, ext)
}

fn tmp_sibling(path: &Path) -> PathBuf {
    let (dir, stem, ext) = split_name(path);
    let base = format!("{stem}.fctmp{}", std::process::id());
    unique_path(&dir, &base, ext.as_deref())
}

/// Moves `tmp` to `want`, or to a fresh name if `want` appeared meanwhile.
fn finish(tmp: &Path, want: &Path) -> Result<PathBuf> {
    let target = if want.exists() {
        let (dir, stem, ext) = split_name(want);
        unique_path(&dir, &stem, ext.as_deref())
    } else {
        want.to_path_buf()
    };
    std::fs::rename(tmp, &target).map_err(io_at(&target))?;
    Ok(target)
}

/// Runs `write` against a temporary file, then moves it to `output`.
/// Returns the final path.
pub fn write_with(output: &Path, write: impl FnOnce(&Path) -> Result<()>) -> Result<PathBuf> {
    if let Some(dir) = output.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).map_err(io_at(dir))?;
    }
    let tmp = tmp_sibling(output);
    match write(&tmp) {
        Ok(()) if tmp.is_file() => finish(&tmp, output).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        }),
        Ok(()) => Err(Error::Internal(
            "the converter produced no output file".into(),
        )),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// A private temporary folder, deleted (best effort) when dropped.
pub struct ScratchDir(PathBuf);

impl ScratchDir {
    pub fn new() -> Result<ScratchDir> {
        let base = std::env::temp_dir();
        for n in 0..1000u32 {
            let d = base.join(format!("formatcraft-{}-{n}", std::process::id()));
            if std::fs::create_dir(&d).is_ok() {
                return Ok(ScratchDir(d));
            }
        }
        Err(Error::Internal(
            "could not create a temporary folder".into(),
        ))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Same as [`write_with`] for a job producing a folder of files.
pub fn write_dir_with(output: &Path, write: impl FnOnce(&Path) -> Result<()>) -> Result<PathBuf> {
    let tmp = tmp_sibling(output);
    std::fs::create_dir_all(&tmp).map_err(io_at(&tmp))?;
    match write(&tmp) {
        Ok(()) => finish(&tmp, output).inspect_err(|_| {
            let _ = std::fs::remove_dir_all(&tmp);
        }),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_leaves_nothing() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("x.txt");
        let r = write_with(&out, |tmp| {
            std::fs::write(tmp, b"partial").unwrap();
            Err(Error::Cancelled)
        });
        assert!(r.is_err());
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 0);
    }

    #[test]
    fn never_replaces_existing() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("x.txt");
        let got = write_with(&out, |tmp| {
            std::fs::write(&out, b"someone else").unwrap();
            Ok(std::fs::write(tmp, b"ours")?)
        })
        .unwrap();
        assert_eq!(got.file_name().unwrap(), "x (2).txt");
        assert_eq!(std::fs::read(&out).unwrap(), b"someone else");
    }
}
