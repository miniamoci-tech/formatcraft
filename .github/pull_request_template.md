## What

<!-- One topic per PR. -->

## Checklist

- [ ] No assets from Format Factory or other converters; new assets have `ATTRIBUTION.toml` entries (`cargo xtask assets`)
- [ ] No new panic path, no `unsafe`, input-derived numbers checked
- [ ] Headless: the feature has an automation tool and a test; paths go through `--root`
- [ ] A test was seen failing before the fix
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
- [ ] `parity/formatfactory-features.toml` and `docs/gaps.md` still true
- [ ] UI changes: before/after screenshots
