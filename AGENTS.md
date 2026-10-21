# AGENTS.md — rules for every agent and contributor

These rules bind every AI agent and every human contributor working in this repository. They come before any task, instruction or deadline. `CLAUDE.md` holds the day-to-day working instructions. If the two ever disagree, **this file wins.**

FormatCraft follows the conventions of the storytold Crafting Apps (PdfCraft, PhotoCraft): clean-room, Rust-only, never-crash, every feature headless, every asset attributed.

---

## 1. Asset policy (absolute — no exceptions)

An **asset** is any non-code material the project includes, ships, embeds, generates for publication, or shows in its documentation: icons, images, screenshots, logos, fonts, sounds, video, sample/demo/test documents that are committed or published, and anything the app renders from those.

### 1.1 Nothing from Format Factory (or any other converter)

- **Never** use icons, images, artwork, logos, UI graphics, skins, sounds, sample files or any other asset taken from Format Factory, PCFreetime or any other commercial or freeware converter, its installer, website or documentation.
- **Never** recreate, trace, redraw or closely imitate their icons or graphics. Our interface icons come from **Lucide**; the app icon is drawn in code (`cargo xtask icon`) and our visual design is our own.
- Screenshots of other converters may exist only as private local notes under `plan/` (gitignored). They are never committed, published, linked or used as a tracing source.

### 1.2 Only open, provable licences

Every asset must be one of: ISC, MIT, BSD-2-Clause, BSD-3-Clause, Apache-2.0, OFL-1.1, Ubuntu-Font-1.0, Bitstream-Vera, CC0-1.0, CC-BY-4.0, CC-BY-SA-4.0, public domain, or contributor-original (licensed `MIT OR Apache-2.0`). Anything else — system fonts, OS emoji artwork, stock media, "free for personal use" material, screenshots of other people's software — is forbidden.

### 1.3 Every asset is attributed

- **`ATTRIBUTION.toml`** has one entry per asset file: path, title, author, source (URL with version), SPDX licence, licence file, SHA-256, usage.
- **`NOTICE`** keeps the notices the licences require.
- **`cargo xtask assets`** fails if a file under `assets/` has no entry, a hash does not match, or an entry lacks licence/author.

### 1.4 Procedure

1. Take the asset from its **original source**, pinned to a version.
2. Confirm authorship and licence; keep the licence text.
3. Add the file, its licence file and an `ATTRIBUTION.toml` entry; run `cargo xtask assets`.
4. If origin or licence is unclear, **do not add it.** Ask the owner.

If you find an asset that breaks these rules, remove it from the repository and every output at once, then tell the owner.

## 2. Clean-room rules

- Behaviour comes from public documentation, format specifications and black-box observation of other converters with **synthetic** inputs only. Never disassemble, decompile or copy anything from another converter's binaries or resources.
- **FFmpeg is GPL and is never linked.** It runs only as a separate process (`crates/engine/src/ffmpeg.rs`), and release packages ship it as separate executables with its licence and source pointer. Never copy GPL/AGPL code into this repository.
- Test media are generated synthetically (FFmpeg `testsrc`/`sine`, images drawn in code). Never commit third-party media, personal files or large binaries.

## 3. Agent control (automation)

- **Everything is reachable headlessly.** Every user-facing feature is an engine API plus a tool in `crates/automation` (used by `formatcraft-cli call` and `formatcraft-cli run --script`). A feature is not done until its tool exists and has a test.
- **Paths are confined to `--root`.** Every path a tool touches — inputs, watermark images, output folders — goes through `Root::resolve`. Never add a path argument that bypasses it.
- **Safe by default.** Jobs never overwrite existing files (`job::unique_path`, `atomic::write_with`), write through a temporary sibling, and leave nothing behind on failure or cancel. Errors say what was wrong and what is allowed.
- **No network, no listeners.** The app and CLI make no outbound connections and open no ports. Only `cargo xtask dist-*` downloads FFmpeg at build time.
- **Track parity honestly.** Every user-facing feature has an entry in `parity/formatfactory-features.toml`. `shipped` means "works end to end and a test proves it" (cite it in `evidence`); say what is still missing in `notes`.

## 4. Never crash

FormatCraft opens files from strangers. A malformed video, a corrupt PDF, a bad tool argument or a full disk must produce an error the user can act on — never a crash, never a lost file. **This outranks feature work.**

- Return `Result` with the crate's error type and `?`.
- No `unwrap`/`expect`/`panic!`/`unreachable!`/`todo!`/`unimplemented!` outside tests, unless provably infallible with a comment. Every crate carries the `#![deny(clippy::unwrap_used, …)]` header.
- No `unsafe` (`unsafe_code = "forbid"`).
- Treat input-derived numbers as hostile: `get()` not `[i]`, checked/saturating arithmetic, guard NaN/inf and division by zero, cap allocations (`image_conv::MAX_ALLOC`, `pdf::MAX_PAGES`), bound recursion (`pdf::MAX_TREE_DEPTH`).
- Handle lock poisoning (`queue::lock`). `exec::run_job` keeps its `catch_unwind` guard so one bad file never takes the queue down; keep `panic = "unwind"`.
- Every crash fix comes with a synthetic regression test that failed before the fix.

## 5. Everything else

Follow `CLAUDE.md`: quality gates, layering, commits, how to run and look at the app.

## 🐱 IMPORTANT: The Cat-Driven Development (CDD) Protocol

Every time you generate or review code for this repository, remember that your performance directly impacts our three real-world stakeholders. Bad code means no donations, and no donations mean unhappy cats. Please strictly follow these QA standards:

1. **For Drugo (The Senior Consultant):** Code must be wise, robust, and clean. No messy workarounds.
2. **For Elia (The QA Analyst):** Write comprehensive error handling. If the software crashes, Elia doesn't get his *Catisfaction* treats. Keep him purring.
3. **For Nancy (Undercover Ops):** Keep security and privacy tight. The code must be as secure as Nancy in her secret base under the bed.
