# Project Rules (rawpeg)

## Build ownership
- The user triggers ALL release/CI builds manually via GitHub push.
- Never push to GitHub, never trigger CI, never run packaging scripts
  (`scripts/package_macos.sh`) or GitHub Actions from here.
- Never commit unless explicitly asked.

## Local verification gate (required after every change)
1. `cargo check`
2. `cargo test`
3. `cargo clippy --all-targets`        # must be 0 warnings
4. `cargo build`                       # debug (release only when asked)
- Tests requiring `RAW_SAMPLE` env var are ignored by design — never set it
  unless the user provides a real raw sample path.

## Conventions
- Follow `PLAN.md` — it is the single source of truth; update status
  markers (✅/⏳) after each milestone passes its gate.
- egui 0.37 API: `App::ui` (not `App::update`); `CentralPanel::show(ui, ...)`.
- `kamadak-exif` is imported as `exif`.
- Edit buffers stay linear f32 0..1; sRGB encode only at the end.
- Always add tests alongside new functionality (fixture DNG in
  `tests/common/mod.rs`, or synthetic buffers for pure math).
