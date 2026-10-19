//! `formatcraft-cli`: headless FormatCraft.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use formatcraft_automation::{Root, call, run_script, tools};
use serde_json::{Value, json};

const USAGE: &str = "formatcraft-cli — headless FormatCraft

USAGE:
  formatcraft-cli tools                          list automation tools
  formatcraft-cli formats [CATEGORY]             list output formats
  formatcraft-cli probe FILE                     show media information
  formatcraft-cli convert FILE... --to FORMAT [--out DIR] [--options JSON]
  formatcraft-cli call TOOL JSON [--root DIR]    run one tool
  formatcraft-cli run --script FILE [--root DIR] run a JSON script of tool steps

Paths in `call` and `run` are confined to --root (default: current folder).";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match real_main(&args) {
        Ok(v) => {
            if !v.is_null() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    let pos = args.iter().position(|a| a == name)?;
    args.get(pos + 1).cloned()
}

fn positional(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if a.starts_with("--") {
            skip = true;
            continue;
        }
        out.push(a.clone());
    }
    out
}

fn root(args: &[String]) -> Result<Root, String> {
    let dir = flag(args, "--root").unwrap_or_else(|| ".".into());
    Root::new(Path::new(&dir)).map_err(|e| e.to_string())
}

fn real_main(args: &[String]) -> Result<Value, String> {
    let Some((cmd, rest)) = args.split_first() else {
        println!("{USAGE}");
        return Ok(Value::Null);
    };
    match cmd.as_str() {
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(Value::Null)
        }
        "--version" | "version" => Ok(json!(env!("CARGO_PKG_VERSION"))),
        "tools" => Ok(Value::Array(
            tools()
                .map(|t| json!({"name": t.name, "summary": t.summary, "example": t.example}))
                .collect(),
        )),
        "formats" => {
            let r = root(rest)?;
            let args = match positional(rest).first() {
                Some(c) => json!({"category": c}),
                None => json!({}),
            };
            call(&r, "formats", &args).map_err(|e| e.to_string())
        }
        "probe" => {
            let file = positional(rest)
                .into_iter()
                .next()
                .ok_or("probe needs a FILE")?;
            let info = formatcraft_engine::probe::probe(&PathBuf::from(file))
                .map_err(|e| e.to_string())?;
            serde_json::to_value(info).map_err(|e| e.to_string())
        }
        "convert" => convert(rest),
        "call" => {
            let pos = positional(rest);
            let tool = pos.first().ok_or("call needs TOOL and JSON arguments")?;
            let args: Value = match pos.get(1) {
                Some(s) => serde_json::from_str(s).map_err(|e| format!("bad JSON: {e}"))?,
                None => Value::Null,
            };
            call(&root(rest)?, tool, &args).map_err(|e| e.to_string())
        }
        "run" => {
            let script = flag(rest, "--script").ok_or("run needs --script FILE")?;
            let text = std::fs::read_to_string(&script).map_err(|e| format!("{script}: {e}"))?;
            let v: Value = serde_json::from_str(&text).map_err(|e| format!("{script}: {e}"))?;
            run_script(&root(rest)?, &v).map_err(|e| e.to_string())
        }
        other => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    }
}

/// Convenience front end: picks the right task for each file by format.
fn convert(rest: &[String]) -> Result<Value, String> {
    use formatcraft_engine::formats::{self, Category};
    use formatcraft_engine::{ImageOptions, MediaOptions, Task};

    let to = flag(rest, "--to").ok_or("convert needs --to FORMAT")?;
    let fmt = formats::find(&to).ok_or_else(|| format!("unknown format `{to}`"))?;
    let out = PathBuf::from(flag(rest, "--out").unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let opts = flag(rest, "--options").unwrap_or_else(|| "{}".into());
    let files = positional(rest);
    if files.is_empty() {
        return Err("convert needs at least one FILE".into());
    }
    let mut results = Vec::new();
    for f in files {
        let input = PathBuf::from(&f);
        let task = match fmt.category {
            Category::Video | Category::Audio => Task::Convert {
                input,
                format: fmt.id.into(),
                options: serde_json::from_str::<MediaOptions>(&opts)
                    .map_err(|e| format!("bad --options: {e}"))?,
            },
            Category::Image => Task::ConvertImage {
                input,
                format: fmt.id.into(),
                options: serde_json::from_str::<ImageOptions>(&opts)
                    .map_err(|e| format!("bad --options: {e}"))?,
            },
            Category::Document => Task::ImagesToPdf {
                inputs: vec![input],
            },
        };
        let r = task.into_job(&out).and_then(|job| {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            formatcraft_engine::run_job(&job, &|p| eprint!("\r{f}: {:>3.0}%", p * 100.0), &cancel)
        });
        eprintln!();
        match r {
            Ok(p) => results.push(json!({"input": f, "output": p})),
            Err(e) => results.push(json!({"input": f, "error": e.to_string()})),
        }
    }
    if results.iter().any(|r| r.get("error").is_some()) {
        return Err(serde_json::to_string_pretty(&results).unwrap_or_default());
    }
    Ok(Value::Array(results))
}
