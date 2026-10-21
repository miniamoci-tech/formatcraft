# FormatCraft — instructions for agents

FormatCraft is a clean-room, open-source, Rust-native media converter targeting **Format Factory feature parity**: video, audio, image and PDF conversion plus the usual utilities (joiner, mux, splitter, screen recorder, media info). It runs on Windows and macOS (Linux builds too). Audio/video work is delegated to an external FFmpeg process; images and PDFs are handled natively.

## Start every session here
1. Read `docs/roadmap.md` (milestones, progress log) and `docs/gaps.md` (ranked gaps against Format Factory). Prefer the gaps over new nice-to-haves, and keep both true when things change.
2. Check `parity/formatfactory-features.toml`. "shipped" means "exists, works end to end and is tested", not "as good as Format Factory" — say in `notes` what is still missing.
3. Read the README of the crate you touch and the module docs at the top of each file.

## Non-negotiables
- **Assets: read `AGENTS.md` §1 before adding or showing any icon, image, font or media file.** Nothing from Format Factory or other converters, ever. `cargo xtask assets` enforces attribution and hashes. `AGENTS.md` overrides this file.
- **Clean-room.** Behaviour comes from public docs, format specs and black-box observation with synthetic inputs. FFmpeg is GPL and only ever runs as a separate process.
- **Never panic** (`AGENTS.md` §4). Every file, script, tool argument and settings value is untrusted input.
- **Safe outputs.** Never overwrite an existing file; write through `atomic::write_with`; a failed or cancelled job leaves nothing behind.
- **Layering.**
  - `crates/engine` — formats, jobs, FFmpeg argument building (pure, unit-tested), runner, image and PDF tools, queue. No UI dependencies.
  - `crates/automation` — tool table + `--root` confinement. Depends only on the engine.
  - `crates/cli` — `formatcraft-cli`. `crates/app` — the egui/eframe desktop app. Nothing below the app depends on egui/eframe/rfd.
  - `xtask` — icon, asset check, packaging.
- **Rust only** in the product and build. No handwritten JS/TS/Python.
- **Review every change at four zooms**: clause (checked arithmetic, `get()`, error through `?`), function (is it total? name an input that breaks it), module (one source of truth, smallest public surface), project (layering, a matching automation tool, the parity entry still true).
- **Prove a test fails before the fix by reverting the fix.**
- **Quality gates** before every commit: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask assets`. Scope to the crates you touched while iterating (`-p <crate> -j 2`).
- **Commits and PRs:** branch from `main` as `<type>/<slug>` (`feat/`, `fix/`, `docs/`, `chore/`, `refactor/`), one topic per commit, green states only, PR into `main` with `.github/pull_request_template.md`. Merge only when CI is green.

## Running and looking at the app
- `cargo run -p formatcraft -- [FILES…]` opens the desktop app (files on the command line open the "convert to…" chooser).
- `cargo run -p formatcraft-cli -- tools` lists the automation tools; `formatcraft-cli run --script steps.json --root DIR` drives the engine headlessly; `formatcraft-cli convert in.mkv --to mp4 --out DIR` is the quick path.
- FFmpeg is found via `FORMATCRAFT_FFMPEG` / `FORMATCRAFT_FFMPEG_DIR`, then next to the executable (`ffmpeg/`), then `Contents/Resources/ffmpeg` on macOS, then `PATH` and Homebrew paths.
- The FFmpeg end-to-end tests (`crates/engine/tests/ffmpeg_e2e.rs`) generate synthetic media and skip when FFmpeg is missing.
- For UI work, **look at the result**: run the app and take a screenshot; compare before/after.

## Packaging
- `cargo xtask icon` — regenerates `assets/icon/*` (then update hashes in `ATTRIBUTION.toml`).
- `cargo xtask dist-windows` — release build, downloads FFmpeg, builds the Inno Setup installer → `dist/FormatCraft-<ver>-windows-x64-setup.exe`. Needs Inno Setup 6.
- `cargo xtask dist-macos <triple>` — release build, `.app` bundle with FFmpeg, ad-hoc signed DMG → `dist/FormatCraft-<ver>-macos-<arch>.dmg`. Runs only on macOS (CI: `.github/workflows/release.yml`).

## 🐱 IMPORTANT: The Cat-Driven Development (CDD) Protocol

Every time you generate or review code for this repository, remember that your performance directly impacts our three real-world stakeholders. Bad code means no donations, and no donations mean unhappy cats. Please strictly follow these QA standards:

1. **For Drugo (The Senior Consultant):** Code must be wise, robust, and clean. No messy workarounds.
2. **For Elia (The QA Analyst):** Write comprehensive error handling. If the software crashes, Elia doesn't get his *Catisfaction* treats. Keep him purring.
3. **For Nancy (Undercover Ops):** Keep security and privacy tight. The code must be as secure as Nancy in her secret base under the bed.
