//! The automation tool table. Every user-facing feature is a tool here, so it
//! can run without the GUI (`formatcraft-cli run --script steps.json`).
//!
//! All paths are confined to a root folder: a tool can never read or write
//! outside it.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;

use formatcraft_engine::formats::{self, Category};
use formatcraft_engine::job::Watermark;
use formatcraft_engine::{Task, probe, run_job};
use serde_json::{Value, json};

pub struct ToolInfo {
    pub name: &'static str,
    pub summary: &'static str,
    pub example: &'static str,
}

/// Tools that turn into a [`Task`] (their args are the task's fields plus
/// an optional `output_dir`).
const TASK_TOOLS: &[ToolInfo] = &[
    ToolInfo {
        name: "convert",
        summary: "Convert audio/video (also extract audio, video to GIF, trim, crop, rotate, watermark)",
        example: r#"{"input":"in.mkv","format":"mp4","options":{"quality":"high","width":1280,"start":5,"end":20}}"#,
    },
    ToolInfo {
        name: "convert_image",
        summary: "Convert an image (resize, rotate, grayscale)",
        example: r#"{"input":"a.png","format":"jpg","options":{"width":800,"jpeg_quality":85}}"#,
    },
    ToolInfo {
        name: "join",
        summary: "Join videos (or audio files when format is audio) in order",
        example: r#"{"inputs":["a.mp4","b.mp4"],"format":"mp4"}"#,
    },
    ToolInfo {
        name: "mux",
        summary: "Replace a video's audio with another file's audio",
        example: r#"{"video":"v.mp4","audio":"music.mp3","format":"mp4"}"#,
    },
    ToolInfo {
        name: "split",
        summary: "Split media into fixed-length parts without re-encoding",
        example: r#"{"input":"long.mp4","segment_secs":60}"#,
    },
    ToolInfo {
        name: "screen_record",
        summary: "Record the screen for max_secs seconds",
        example: r#"{"format":"mp4","fps":30,"max_secs":10}"#,
    },
    ToolInfo {
        name: "pdf_merge",
        summary: "Merge PDFs in order",
        example: r#"{"inputs":["a.pdf","b.pdf"]}"#,
    },
    ToolInfo {
        name: "pdf_split",
        summary: "Split a PDF into one file per page",
        example: r#"{"input":"a.pdf"}"#,
    },
    ToolInfo {
        name: "pdf_compress",
        summary: "Rewrite a PDF with compressed streams and unused objects removed",
        example: r#"{"input":"a.pdf"}"#,
    },
    ToolInfo {
        name: "images_to_pdf",
        summary: "Make a PDF with one page per image",
        example: r#"{"inputs":["1.jpg","2.png"]}"#,
    },
];

const INFO_TOOLS: &[ToolInfo] = &[
    ToolInfo {
        name: "formats",
        summary: "List output formats (optionally {\"category\":\"video\"})",
        example: r#"{"category":"audio"}"#,
    },
    ToolInfo {
        name: "probe",
        summary: "Media information (duration, streams, codecs)",
        example: r#"{"path":"in.mp4"}"#,
    },
    ToolInfo {
        name: "ffmpeg_info",
        summary: "Which FFmpeg is used and whether it was found",
        example: "{}",
    },
];

pub fn tools() -> impl Iterator<Item = &'static ToolInfo> {
    INFO_TOOLS.iter().chain(TASK_TOOLS.iter())
}

/// A tool error an agent can act on.
#[derive(Debug)]
pub struct ToolError(pub String);

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ToolError {}

fn err(msg: impl Into<String>) -> ToolError {
    ToolError(msg.into())
}

/// Resolves paths against a root folder and refuses anything outside it.
pub struct Root {
    root: PathBuf,
}

/// Lexical normalisation (no filesystem access): removes `.` and resolves
/// `..` without ever climbing above the start.
fn normalize(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

/// Drops the Windows `\\?\` verbatim prefix that `canonicalize` adds to
/// drive paths, so paths stay readable and work with every tool.
fn simplify(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\")
        && rest.as_bytes().get(1) == Some(&b':')
    {
        return PathBuf::from(rest);
    }
    p
}

impl Root {
    pub fn new(root: &Path) -> Result<Root, ToolError> {
        let root = std::fs::canonicalize(root)
            .map(simplify)
            .map_err(|e| err(format!("root folder {}: {e}", root.display())))?;
        Ok(Root { root })
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    /// `p` relative to the root (or absolute inside it), confined to it.
    pub fn resolve(&self, p: &Path) -> Result<PathBuf, ToolError> {
        let joined = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        };
        let outside = || {
            err(format!(
                "path {} is outside the allowed root {}; use a path inside it",
                p.display(),
                self.root.display()
            ))
        };
        let norm = normalize(&joined).ok_or_else(outside)?;
        // Follow symlinks for the part that exists.
        let mut existing = norm.clone();
        let mut tail = Vec::new();
        while !existing.exists() {
            match (
                existing.file_name().map(|s| s.to_os_string()),
                existing.parent(),
            ) {
                (Some(name), Some(parent)) => {
                    tail.push(name);
                    existing = parent.to_path_buf();
                }
                _ => return Err(outside()),
            }
        }
        let mut real = std::fs::canonicalize(&existing)
            .map(simplify)
            .map_err(|_| outside())?;
        for name in tail.into_iter().rev() {
            real.push(name);
        }
        if real.starts_with(&self.root) {
            Ok(real)
        } else {
            Err(outside())
        }
    }
}

fn confine_task(task: &mut Task, root: &Root) -> Result<(), ToolError> {
    let fix = |p: &mut PathBuf| -> Result<(), ToolError> {
        *p = root.resolve(p)?;
        Ok(())
    };
    match task {
        Task::Convert { input, options, .. } => {
            fix(input)?;
            if let Some(Watermark { image, .. }) = options.watermark.as_mut() {
                fix(image)?;
            }
        }
        Task::Join {
            inputs, options, ..
        } => {
            for p in inputs.iter_mut() {
                fix(p)?;
            }
            if let Some(Watermark { image, .. }) = options.watermark.as_mut() {
                fix(image)?;
            }
        }
        Task::PdfMerge { inputs } | Task::ImagesToPdf { inputs } => {
            for p in inputs.iter_mut() {
                fix(p)?;
            }
        }
        Task::ConvertImage { input, .. }
        | Task::Split { input, .. }
        | Task::PdfSplit { input }
        | Task::PdfCompress { input } => fix(input)?,
        Task::Mux { video, audio, .. } => {
            fix(video)?;
            fix(audio)?;
        }
        Task::ScreenRecord { .. } => {}
    }
    Ok(())
}

/// Runs one tool. `args` is a JSON object.
pub fn call(root: &Root, tool: &str, args: &Value) -> Result<Value, ToolError> {
    let obj = match args {
        Value::Object(m) => m.clone(),
        Value::Null => serde_json::Map::new(),
        _ => return Err(err("tool arguments must be a JSON object")),
    };
    match tool {
        "formats" => {
            let cat = obj.get("category").and_then(Value::as_str);
            let cats: Vec<Category> =
                match cat {
                    None => Category::ALL.to_vec(),
                    Some(c) => {
                        let found = Category::ALL.into_iter().find(|x| x.id() == c).ok_or_else(
                            || {
                                err(format!(
                                    "unknown category `{c}`; use video, audio, image or document"
                                ))
                            },
                        )?;
                        vec![found]
                    }
                };
            let list: Vec<Value> = cats
                .into_iter()
                .flat_map(formats::in_category)
                .map(
                    |f| json!({"id": f.id, "ext": f.ext, "label": f.label, "category": f.category}),
                )
                .collect();
            Ok(Value::Array(list))
        }
        "probe" => {
            let p = obj
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| err("probe needs {\"path\": \"file\"}"))?;
            let path = root.resolve(Path::new(p))?;
            let info = probe::probe(&path).map_err(|e| err(e.to_string()))?;
            serde_json::to_value(info).map_err(|e| err(e.to_string()))
        }
        "ffmpeg_info" => Ok(match formatcraft_engine::ffmpeg::Ffmpeg::get() {
            Ok(f) => {
                json!({"found": true, "ffmpeg": f.ffmpeg, "ffprobe": f.ffprobe, "version": f.version})
            }
            Err(e) => json!({"found": false, "error": e.to_string()}),
        }),
        name if TASK_TOOLS.iter().any(|t| t.name == name) => {
            let mut obj = obj;
            let out_dir = match obj.remove("output_dir") {
                Some(Value::String(s)) => root.resolve(Path::new(&s))?,
                Some(_) => return Err(err("output_dir must be a string")),
                None => root.path().to_path_buf(),
            };
            obj.insert("kind".into(), Value::String(name.to_string()));
            let mut task: Task = serde_json::from_value(Value::Object(obj))
                .map_err(|e| err(format!("bad arguments for `{name}`: {e}")))?;
            confine_task(&mut task, root)?;
            std::fs::create_dir_all(&out_dir)
                .map_err(|e| err(format!("cannot create {}: {e}", out_dir.display())))?;
            let job = task.into_job(&out_dir).map_err(|e| err(e.to_string()))?;
            let cancel = AtomicBool::new(false);
            let out = run_job(&job, &|_| {}, &cancel).map_err(|e| err(e.to_string()))?;
            Ok(json!({"output": out}))
        }
        other => Err(err(format!(
            "unknown tool `{other}`; run `formatcraft-cli tools` for the list"
        ))),
    }
}

/// Runs a script: `{"steps":[{"tool":"convert","args":{...}}, ...]}` or a
/// bare array of steps. Stops at the first failing step.
pub fn run_script(root: &Root, script: &Value) -> Result<Value, ToolError> {
    let steps = match script {
        Value::Array(a) => a,
        Value::Object(o) => o
            .get("steps")
            .and_then(Value::as_array)
            .ok_or_else(|| err("script needs a `steps` array"))?,
        _ => return Err(err("script must be an object with `steps` or an array")),
    };
    let mut results = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        let tool = step
            .get("tool")
            .and_then(Value::as_str)
            .ok_or_else(|| err(format!("step {}: missing `tool`", i + 1)))?;
        let args = step.get("args").cloned().unwrap_or(Value::Null);
        let r =
            call(root, tool, &args).map_err(|e| err(format!("step {} ({tool}): {e}", i + 1)))?;
        results.push(json!({"tool": tool, "result": r}));
    }
    Ok(Value::Array(results))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_confinement() {
        let d = tempfile::tempdir().unwrap();
        let root = Root::new(d.path()).unwrap();
        assert!(root.resolve(Path::new("a/b.png")).is_ok());
        assert!(root.resolve(Path::new("../x.png")).is_err());
        assert!(root.resolve(Path::new("a/../../x.png")).is_err());
        let outside = d.path().parent().unwrap().join("x.png");
        assert!(root.resolve(&outside).is_err());
    }

    #[test]
    fn script_runs_image_and_pdf_tools() {
        let d = tempfile::tempdir().unwrap();
        image::RgbImage::new(20, 10)
            .save(d.path().join("a.png"))
            .unwrap();
        let root = Root::new(d.path()).unwrap();
        let script = json!({"steps":[
            {"tool":"convert_image","args":{"input":"a.png","format":"webp","output_dir":"out"}},
            {"tool":"images_to_pdf","args":{"inputs":["a.png","out/a.webp"],"output_dir":"out"}},
            {"tool":"formats","args":{"category":"image"}}
        ]});
        let r = run_script(&root, &script).unwrap();
        assert_eq!(r.as_array().unwrap().len(), 3);
        assert!(d.path().join("out/a.webp").is_file());
        assert!(d.path().join("out/a.pdf").is_file());
    }

    #[test]
    fn task_paths_are_confined() {
        let d = tempfile::tempdir().unwrap();
        let root = Root::new(d.path()).unwrap();
        let e = call(&root, "pdf_compress", &json!({"input":"../../etc/passwd"})).unwrap_err();
        assert!(e.0.contains("outside"), "{e}");
        let e = call(
            &root,
            "convert_image",
            &json!({"input":"a.png","format":"jpg","output_dir":"../.."}),
        )
        .unwrap_err();
        assert!(e.0.contains("outside"), "{e}");
        let e = call(&root, "nope", &json!({})).unwrap_err();
        assert!(e.0.contains("unknown tool"));
    }

    #[test]
    fn every_tool_example_parses() {
        for t in TASK_TOOLS {
            let mut v: Value = serde_json::from_str(t.example).unwrap();
            v.as_object_mut()
                .unwrap()
                .insert("kind".into(), json!(t.name));
            let _: Task = serde_json::from_value(v).unwrap_or_else(|e| panic!("{}: {e}", t.name));
        }
    }
}
