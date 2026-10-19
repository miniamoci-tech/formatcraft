# Gaps against Format Factory

Status: ranked, most valuable first. Last updated 2026-10-19.

1. **PDF → image / text / Word.** Needs a permissively licensed renderer (e.g. `hayro`, Apache-2.0/MIT) and text extraction. No GPL renderers (MuPDF, Poppler) may be linked.
2. **Subtitles** (add soft subtitles, burn in). Burn-in needs libass in the bundled FFmpeg and an attributed font.
3. **Text watermark.** Needs a bundled OFL font with an `ATTRIBUTION.toml` entry.
4. **PDF compression** only recompresses streams; downsampling embedded images is missing.
5. **Screen recorder**: no system-audio capture on Windows, no region selection, no automated test.
6. **Hardware encoders** are used only as fallbacks; no UI to prefer NVENC/QSV/VideoToolbox.
7. **Batch presets**: saving and reusing named option sets.
8. **DVD/CD/ISO tools** — out of scope for now (libdvdcss cannot be bundled).
9. **Video downloader** — not planned: the app makes no network connections.
10. **Code signing / notarisation** of the installers.

## Revision history

- 2026-10-19 — created with the 0.1.0 gap list.
