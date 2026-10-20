# Roadmap

Status: 0.1.0 — first public build. Last updated 2026-10-19.

| Milestone | Scope | Status |
|---|---|---|
| M0 | Workspace, never-crash lints, asset policy, CI, installers | done |
| M1 | Engine: format catalogue, FFmpeg runner with progress/cancel, atomic outputs | done |
| M2 | Video/audio conversion with full option set; join, mux, split | done |
| M3 | Images (native) and PDF tools (merge/split/compress/images→PDF) | done |
| M4 | Desktop app: sidebar, dialogs, task queue, drag & drop, IT/EN | done |
| M5 | Automation tools + CLI with `--root` confinement | done |
| M6 | Gaps from `docs/gaps.md`: subtitles, text watermark, PDF→image/text, image PDF compression | next |
| M6a | Disc tools: audio CD and DVD to audio | done (synthetic-disc tests) |
| M7 | Signed/notarised releases, auto-update check (opt-in) | later |

## Progress log

- 2026-10-20 — Disc tools: audio CD (.cda or drive) and DVD (drive, VIDEO_TS, .iso) to audio, in the engine, automation (`cd_track`, `dvd_to_audio`, `cd_list`, `dvd_list`), CLI and app. Tested on synthetic discs only.
- 2026-10-19 — 0.1.0: engine, CLI, desktop app, Windows installer and macOS DMG pipeline. 38 tests, including FFmpeg end-to-end conversions to 10 video and 11 audio formats, join/mux/split and cancellation. GUI checked by screenshots (MP4 and GIF conversions from the app).

## Revision history

- 2026-10-19 — created.
