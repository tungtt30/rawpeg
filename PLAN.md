# rawpeg — Kế hoạch phát triển (Implementation Plan)

Trang này là nguồn tham chiếu duy nhất cho toàn bộ dự án **RAW → JPEG/PNG/TIFF
batch processor** (tên app: `rawpeg`). Cập nhật trạng thái sau mỗi milestone và
ghi lại các quyết định kỹ thuật cùng bài học khi triển khai để không quên.

---

## 1. Tổng quan & Tech Stack (đã chốt)

| Hạng mục | Lựa chọn | Ghi chú |
|---|---|---|
| Ngôn ngữ | Rust 2024 edition, toolchain 1.97+ | `eframe 0.37` cần rust ≥ 1.95 |
| GUI | `eframe` / `egui` 0.37 | **Trait đổi**: `App::ui(&mut self, ui, frame)` thay vì `App::update(ctx, frame)`; `Panel::top/left/right/bottom(…).show(ui, …)`; `CentralPanel::show(ui, …)` |
| RAW engine | **`imagepipe` 0.5.1** (+ `rawloader` 0.37.2 kèm sẵn) | Pipeline đầy đủ: gofloat → demosaic → toLab → basecurve → fromLab → gamma |
| JPEG export | **`jpeg-encoder` 0.7.1** | Pure Rust; quality **98**; mặc định **4:4:4**; `add_exif_metadata` |
| PNG/TIFF export | `image` 0.25 | PNG lossless + EXIF (`PngEncoder::set_exif_metadata`); TIFF lossless 8-bit (chưa nhúng EXIF) |
| Xử lý ảnh | `image` 0.25, `rayon` 1 | resize/thumbnail; export song song có giới hạn |
| EXIF đọc | `kamadak-exif` 0.6 (crate name: **`exif`**) | `Reader::new().read_from_container(…)`; chấp nhận `PartialResult` |
| EXIF ghi | `exif::experimental::Writer` + `jpeg-encoder` / `image` | Tái tạo block TIFF nhỏ; bỏ `MakerNote`/`UserComment`/`Value::Unknown` |
| File dialog | `rfd` 0.18 | Native picker |
| I/O + lỗi | `anyhow` 1, `log` + `env_logger` | |
| Đa luồng | `std::sync::mpsc` + `rayon` | UI thread không bao giờ block; export thread riêng |

## 2. Kiến trúc & Cấu trúc dự án

```text
rawpeg/
├── Cargo.toml                         # + jpeg-encoder 0.7.1; build-dep embed-resource (Windows)
├── PLAN.md                            # file này
├── build.rs                           # ✅ M5: nhúng icon vào .exe Windows
├── rawpeg.rc                          # ✅ M5: resource script (IDI_ICON1)
├── assets/                            # ✅ M5: icon-source.png, rawpeg.ico, rawpeg.icns, rawpeg-256.png
├── .github/
│   ├── workflows/ci.yml               # ✅ CI build check + release khi tag v*
│   └── macos/Info.plist               # bundle metadata + CFBundleIconFile
├── scripts/
│   ├── make_icons.py                  # ✅ M5: sinh icon assets (macOS, stdlib + sips/iconutil)
│   ├── fetch_samples.sh               # tải RAW mẫu CC0 từ raw.pixls.us
│   └── package_macos.sh               # gói binary(s) → rawpeg.app + icon + version (CI + local)
├── src/
│   ├── lib.rs                         # pub mod app / models / raw_engine
│   ├── main.rs                        # eframe entry; window 1320×860, min 1040×680
│   ├── app/
│   │   ├── mod.rs                     # RawpegApp + theme + shortcuts + drag/drop
│   │   ├── state.rs                   # AppState, selection, export session, toasts
│   │   ├── worker.rs                  # Probe/Thumbnail/Proxy/Render/AutoTone + export thread
│   │   └── panels/
│   │       ├── mod.rs                 # dispatch, dialogs, modal help/confirm, toast, drop overlay
│   │       ├── top_bar.rs             # Open/Add, Library|Develop, Export, EXIF, help
│   │       ├── library.rs             # left filmstrip + filter + select all
│   │       ├── grid.rs                # Library mode: virtualized grid + bulk actions + empty state
│   │       ├── develop.rs             # Develop viewport + Develop inspector
│   │       ├── export.rs              # Export inspector: settings, progress, results
│   │       ├── status.rs              # bottom status bar + export progress
│   │       └── widgets.rs             # colors, buttons, badges, histogram, EXIF format
│   ├── raw_engine/
│   │   ├── decoder.rs                 # ✅ M1
│   │   ├── processor.rs               # ✅ M2
│   │   ├── auto_tone.rs               # ✅ M2
│   │   └── exporter.rs                # ✅ M4: decode full/proxy → apply → sRGB → JPEG/PNG/TIFF + EXIF
│   └── models/
│       ├── edit_params.rs             # ✅ M1
│       ├── export_settings.rs         # ✅ M4: ExportFormat + ExportSettings
│       └── image_item.rs              # ✅ M1
└── tests/
    ├── common/mod.rs                  # fixture DNG 12x12 RGGB tự sinh
    ├── decoder_tests.rs               # ✅ 8 tests + 1 ignored (RAW thật)
    ├── processor_tests.rs             # ✅ 6 tests
    ├── app_worker_tests.rs            # ✅ 8 tests: probe/thumbnails/proxy/auto-tone/state export/proxy failure
    ├── exporter_tests.rs              # ✅ 11 tests: JPEG/PNG/TIFF, EXIF, resize, skip, cancel, lỗi, duplicate output
    ├── ui_smoke_tests.rs              # ✅ 2 tests: render headless empty + Library/Develop
    ├── assets_tests.rs                 # ✅ 5 tests M5: PNG/ICO/ICNS, rc + Info.plist
    └── real_pipeline_tests.rs          # ✅ ignored: full pipeline + exporter trên RAW thật
```

Ngoài ra (công cụ dev, không nằm trong `cargo test` gate):
```text
examples/render_previews.rs      # render PNG+JPEG full-res để soi bằng mắt (--max-dim N)
```

## 3. Data flow (bắt buộc giữ đúng)

```text
RAW file
 └─ imagepipe::Pipeline::new_from_file(path)
     ├─ maxwidth/maxheight = giới hạn cạnh dài (0 = full-res)
     ├─ output_16bit(None) → SRGBImage16 (KHÔNG gamma khi settings.linear=true
     │     → data LINEAR f32 scale 0..65535) ─── DÙNG CHO CHỈNH SỬA / EXPORT
     └─ output_8bit(None)  → sRGB u8 ─────────── DÙNG CHO THUMBNAIL
```

- **Import:** `scan_directory` → `Probe` từng file (metadata-only) → status + EXIF;
  thumbnail chỉ decode khi file lọt vào vùng nhìn thấy (lazy, tránh decode 1000 ảnh).
- **Buffer chỉnh sửa:** `Vec<[f32;3]>` linear, chuẩn hóa `0.0..=1.0` (`/65535.0`).
  Mọi phép toán (Exposure, Contrast, Highlights/Shadows, WB) chạy trong không gian
  linear; chỉ sRGB-encode ở bước cuối.
- **Base tone curve** của imagepipe bị tắt khi decode linear
  (`pipeline.ops.basecurve.points = vec![]; .exposure = 0.0`) để giữ highlight headroom.
- **Proxy image:** decode full-res 1 lần khi mở Develop → downscale ~1600px cho slider
  real-time; click trong Library chỉ set current, không decode proxy.
- **Render:** `RenderAfter` tái dùng proxy trong worker; coalescing chỉ giữ request mới nhất.
- **Export:** snapshot selection → jobs → `decode_linear_scaled(max_dim)` → `apply` →
  `encode_srgb` → JPEG/PNG/TIFF → EXIF → ghi file tạm → rename. File lỗi không dừng batch.
- **EXIF:** đọc `ExifSummary` để hiện UI; block EXIF nhỏ được dựng lại bằng
  `exif::experimental::Writer`, chuẩn hoá `Orientation = 1` và cập nhật pixel dimensions.
- **Threading:** worker thread xử lý scan/probe/thumbnail/proxy/render; export chạy
  thread riêng với progress `mpsc`; cancel bằng `Arc<AtomicBool>`.

## 4. Tính năng (theo spec)

### Format RAW
CR2/CR3/NEF/ARW/RAF/ORF/RW2/DNG (+ srw/pef/kdc/erf/... danh sách trong `RAW_EXTENSIONS`).

### UI/UX batch-first (M3-R)
- **Library mode mặc định**: grid ảnh full-width, tick chọn hàng loạt, filter
  `All / Selected / Edited / Issues`, `Select all` tri-state, `Clear`.
- **Top bar**: `Open Folder`, `Add Files`, `Library | Develop`, `Export N ▾`
  (mở Export panel / `Quick export`), EXIF ảnh hiện tại, nút `?` help.
- **Bulk actions**: `Sync current to N`, `Reset selected` (có confirm), `Export N`.
- **Develop mode**: double-click/Enter từ grid; `‹ ›` đổi ảnh, `Fit`, `100%`,
  `Before`; `Space` giữ để so sánh; zoom bằng scroll, pan bằng drag, double-click reset.
- **Develop inspector**: Sliders Light/WB, `Auto Balance`, `Reset`, `Copy`,
  `Paste to N`, `Sync to selected (N)`, histogram RGB, `As Shot`.
- **Library inspector trái**: filmstrip + checkbox + badge `edited/original/unsupported`,
  filter, lazy thumbnail, tooltip lý do unsupported.
- **Export panel**: destination (same source/custom), format JPEG/PNG/TIFF,
  quality 98/4:4:4, long-edge resize, preserve EXIF, overwrite/skip, progress,
  cancel, result + lỗi từng file, `Open folder`.
- **Status bar**: trạng thái worker, số selected/edited/unsupported, progress export,
  `Cancel`, summary.
- **Empty state + drag/drop** toàn cửa sổ; toast; help modal; shortcut:
  `Cmd/Ctrl+O`, `Cmd/Ctrl+Shift+O`, `Cmd/Ctrl+E`, `Cmd/Ctrl+Shift+E`,
  `Cmd/Ctrl+A`, `Cmd/Ctrl+Shift+A`, `L`, `Enter`, `Space`, `←/→`, `F`, `1`, `B`, `?`, `Esc`.

### Xuất ảnh
- JPEG quality 98, 4:4:4 (tuỳ chọn 4:2:0), pure Rust.
- PNG lossless 8-bit + EXIF; TIFF lossless 8-bit (chưa nhúng EXIF).
- `max_dim` (long edge) decode trực tiếp qua imagepipe, không phóng to.
- EXIF copy cho JPEG/PNG; orientation được chuẩn hoá; bỏ metadata quá lớn.
- Batch song song: full-res tối đa 2 job, có `max_dim` tối đa 4 job (giới hạn RAM).
- Overwrite hoặc Skip file đích; ghi file tạm rồi rename; `catch_unwind` per file.
- Progress theo từng file; Cancel sau file đang chạy; summary + danh sách lỗi.

### Auto tone
Histogram linear RGB → exposure đưa median về ~18% gray, clip ≤0.5% pixel; WB giữ
nguyên; `is_auto_applied = true`.

## 5. Milestones & trạng thái

### ✅ Milestone 1 — Setup + Core RAW Pipeline (XONG)
- Cargo.toml + scaffold, decoder + models + tests.
- Gate đã qua: check/test/clippy/build; decode thật CR2/NEF.

### ✅ Milestone 2 — Image Processing & Auto-Exposure (XONG)
- `processor.rs` (WB → exposure → H/S → contrast; soft shoulder sRGB; rayon).
- `auto_tone.rs`; 17 unit + 6 integration trên fixture.
- Gate đã qua.

### ✅ Milestone 3 (bản cũ) — Darkroom UI (đã bị thay thế)
- Bản darkroom đầu tiên với top bar, library list, viewport, adjustments panel.
- Đã merge branch `milestone-3`; được thay bằng M3-R bên dưới.

### ✅ Milestone 3-R — Batch-first UI/UX redesign (XONG)
1. **State** (`app/state.rs`):
   - `ViewMode`, `RightTab`, `LibraryFilter`, `SelectionStats`, `Toast`, `ConfirmAction`.
   - Selection helpers: `select_supported`, `deselect_unsupported`, `select_range`,
     `sync_to_selected`, `reset_selected`, `visible_indices`, `export_jobs`.
   - `ExportSession` theo dõi progress/cancel/summary; `poll` cập nhật từ `Message::Export`.
   - `select_only` cho Library click; `select_and_load` + `ensure_current_loaded` cho Develop.
2. **Worker** (`app/worker.rs`):
   - Tách `Job::Probe` (status + EXIF) khỏi `LoadThumbnail` (lazy, chỉ vùng nhìn thấy).
   - `Worker::start_export`: thread riêng `rawpeg-export`; progress `Message::Export`;
     `catch_unwind` toàn thread để báo `ExportEvent::Fatal`.
3. **UI** (`app/panels/`):
   - Library-first grid virtualized; filter; bulk bar; library filmstrip trái; inspector
     phải có tab `Develop`/`Export`; status bar có export progress/cancel.
   - Develop viewport: Fit/100%/Before, Space peek, thumbnail placeholder + spinner.
   - Export panel: destination/format/quality/size/EXIF/overwrite; start/quick export;
     summary, lỗi từng file, `Open folder`.
   - Empty state, drag/drop overlay, toast, help modal, confirm modal, keyboard shortcuts.
   - Theme darkroom tinh chỉnh spacing/typography; window min size.
4. **Tests**:
   - 51 lib tests (thêm selection/export session/show filter/regression review).
   - `app_worker_tests` 8 (thêm Probe, ProxyFailed + state begin_export end-to-end).
   - `ui_smoke_tests` 2 (headless, `Context::run_ui`, clear `textures_delta`).
   - Real-sample ignored test cho exporter.
- **Gate đã qua:** `./scripts/verify_local.sh` ALL GREEN
  (`cargo test`: **86 passed, 4 ignored**; clippy 0 warning; build debug).
  `cargo run` mở cửa sổ, không panic.

### ✅ Milestone 4 — Batch Exporter (XONG)
- `models/export_settings.rs`: `ExportFormat` (JPEG/PNG/TIFF), `ExportSettings`
  (quality, 4:4:4, max_dim, EXIF, overwrite, output_dir), `output_path`.
- `raw_engine/exporter.rs`: `ExportJob`, `ExportOutcome`, `ExportSummary`, `ExportEvent`,
  `export_batch`; rayon pool; temp file + rename; skip/overwrite; per-file error;
  cancel; EXIF writer; PNG eXIf; TIFF pixels.
- `exporter_tests.rs` 11 tests: JPEG/PNG/TIFF, EXIF, max_dim, skip/overwrite,
  cancel, một file lỗi không dừng batch, temp cleanup, duplicate output.
- `state.begin_export` + `Worker::start_export` + Export panel + status bar.
- Real-sample ignored test: `exporter_handles_real_sample` (RAW_DIR) pass với
  `rawimage/DHD_5747.nef` → JPEG 1069×1600.

### ⏳ Milestone 5 — GitHub Actions CI/CD (code xong, chờ tag release)
- ✅ CI build check đã chạy thành công (run #45201774320, 2026-11-23):
  Windows x64 zip + macOS universal zip.
- ✅ Release pipeline trong `ci.yml`:
  - trigger thêm `tags: ["v*"]`; `concurrency` không cancel tag release.
  - `preflight`: đọc version từ `Cargo.toml` bằng `tomllib`, fail nếu tag khác `v{version}`.
  - `release`: `needs: [preflight, build]`, tải artifact, sinh `SHA256SUMS.txt`,
    `softprops/action-gh-release@v2` với `generate_release_notes`,
    `fail_on_unmatched_files`, prerelease cho tag có `-`.
  - `permissions: contents: read` mặc định; job release có `contents: write`.
- ✅ Windows icon:
  - `build.rs` + `rawpeg.rc` + `embed-resource 3.0` (chỉ build-dependency Windows);
    icon nhúng vào `.exe` (Explorer/taskbar).
  - Runtime window icon qua `eframe::icon_data::from_png_bytes` + `ViewportBuilder::with_icon`.
- ✅ macOS icon/bundle:
  - `assets/rawpeg.icns` → `Contents/Resources/`; `CFBundleIconFile` trong Info.plist.
  - `package_macos.sh` inject version từ `Cargo.toml` bằng `plutil` (không hardcode).
- ✅ Icon assets: `scripts/make_icons.py` sinh PNG master 1024, ICO 6 kích thước
  (PNG payload), ICNS; output commit sẵn; `tests/assets_tests.rs` kiểm tra.
- ⏳ Còn lại (chỉ người dùng thực hiện): push `main` → CI xanh; tạo tag `v0.1.0`
  và push tag → xác nhận GitHub Release có 2 zip + `SHA256SUMS.txt`; tải/mở thử app.
  Sau khi xác nhận mới đổi milestone này sang ✅ (xem runbook mục 8).
- Không push/trigger CI từ môi trường này; người dùng thực hiện.

## 6. Testing strategy
- **Fixture DNG 12x12 RGGB** trong `tests/common/mod.rs` (TIFF writer thủ công):
  Make/Model, DNGVersion, ColorMatrix1 = identity, AsShotNeutral (1,1,1), raw IFD
  16-bit uncompressed strip. Quy tắc: `count` TIFF entry là số VALUE, không phải byte;
  imagepipe cần cạnh ≥ 10 (`owidth-10`).
- **Exporter tests** dùng fixture: JPEG/PNG/TIFF decode lại bằng `image`, EXIF đọc bằng
  `kamadak-exif`, kiểm tra max_dim, temp cleanup, skip/overwrite, cancel, lỗi.
- **State/worker tests**: selection stats, filter, sync/reset, export jobs, export
  session events, probe/thumbnail/proxy jobs, `AppState::poll` headless.
- **UI smoke**: `egui::Context::run_ui` render empty + Library/Develop; phải gọi
  `output.textures_delta.clear()` trước khi drop `FullOutput`.
- **Assets (M5)**: `assets_tests.rs` decode PNG master/runtime bằng `image`,
  parse ICONDIRENTRY của ICO (PNG payload, đủ 6 kích thước), kiểm tra magic ICNS,
  và đảm bảo `rawpeg.rc` + `Info.plist` tham chiếu đúng file icon.
- **Real RAW** (ignored): `RAW_DIR=rawimage cargo test --test real_pipeline_tests -- --ignored --nocapture`
  chạy full pipeline + exporter trên mẫu thật; file unsupported được báo, không fail.
- **Gate bắt buộc:** `./scripts/verify_local.sh` (check/test/clippy -D warnings/build).
  `cargo fmt --all` được chạy trước gate.

## 7. Bài học / rủi ro đã ghi nhận (quan trọng)

1. **Demosaic imagepipe là fast-path thô** (neighbor averaging, không AHD/DCB).
   Preview/edit tạm chấp nhận được; cải thiện chất lượng demosaic vẫn là follow-up
   chất lượng (không chặn UI/export hiện tại).
2. **Thumbnail:** rawloader 0.37 không expose `decode_thumb` → dùng pipeline nhỏ;
   nay chỉ decode lazy cho vùng nhìn thấy.
3. **egui 0.37 API mới:** `App::ui`, `Panel::top/left/right/bottom(…).show(ui, …)`,
   `CentralPanel::show(ui, …)`; `Context::run_ui` cho test headless (không dùng `run`).
4. **`kamadak-exif` lib name = `exif`.**
5. **`output_16bit` KHÔNG gamma** (linear) dù struct doc ghi "sRGB": dựa vào `settings.linear`.
6. **Basecurve** phải tắt để có highlight headroom trong buffer chỉnh sửa.
7. **CI: không thư viện C ngoài** (mọi crate pure Rust) — `jpeg-encoder`/`image`/rayon đều pure Rust.
8. **macOS runner Intel đã retire** — build universal binary bằng lipo trên runner ARM64.
9. **Soft shoulder highlight (M2):** knee tại 0.8 linear (`0.8 + 0.2·(1−e^(−5(c−0.8)))`).
10. **Histogram quantization trong auto-tone:** tolerance ±0.05 EV; tail > 1.0 vào bin cuối.
11. **Thứ tự pipeline cố định:** WB → exposure → H/S → contrast.
12. **rawloader 0.37.2 chỉ hỗ trợ camera tới ~2019**; `probe_support` báo
    UnsupportedCamera/Format thay vì lỗi chung; DNG lạ có thể panic và được catch.
13. **`--max-dim`** cho tool render preview; không cần full-res khi soi.
14. **Coalescing** job trong worker: giữ request mới nhất cho proxy/auto/render;
    thumbnails/probes giữ hết. Không coalesce → queue phình, ảnh hiển thị trễ.
15. **Proxy token** chống race khi chuyển ảnh nhanh.
16. **Texture upload:** `ctx.load_texture` + `TextureHandle::set`; không tạo texture mới mỗi frame.
17. **Panel API 0.37** như trên; `CentralPanel` cuối cùng.
18. **Headless texture test:** `Context::default()` max texture side 2048; proxy 1600 an toàn.
19. **Before/After = giữ Space** (Develop) + nút `Before`; Library dùng Space để toggle chọn.
20. **`egui::Context::run_ui`** là API headless đúng; `FullOutput.textures_delta.clear()`
    trước khi drop, nếu không panic “unapplied deltas”.
21. **Drag & drop 0.37:** `DroppedFile::path()` trả `&Path` (không phải `Option`);
    `hovered_files` là `Vec<HoveredFile>`; `Id::new` deprecated → `Id::unique`.
22. **Context API 0.37:** `content_rect()` thay `screen_rect`; `egui_wants_keyboard_input()`;
    `all_styles_mut`/`style_mut_of(Theme::Dark, …)` thay `style_mut`.
23. **EXIF writer:** dùng `exif::experimental::Writer`; lọc `Value::Unknown`,
    `MakerNote`, `UserComment`; đọc source bằng `continue_on_error(true)` rồi
    `distill_partial_result`; chuẩn hoá Orientation và pixel dimensions.
24. **JPEG APP1 giới hạn ~65,528 bytes**; nếu EXIF quá lớn, bỏ EXIF nhưng vẫn ghi ảnh.
25. **Export memory:** full-res f32 buffer lớn; full-res tối đa 2 job, có `max_dim`
    decode trực tiếp (không resize sau) để tránh OOM.
26. **Export thread riêng:** UI vẫn mượt; export là snapshot params/settings; thay đổi
    sau khi bắt đầu không ảnh hưởng batch đang chạy. Cancel không dừng file đang decode.
27. **Lazy thumbnail + Probe:** tránh decode toàn bộ thư viện; status/EXIF vẫn có sớm.
28. **Grid tile:** `ui.put` checkbox trên ảnh; response của checkbox chặn click toggle;
    `Response` clone cho tooltip + context menu.
29. **Keyboard guard:** bỏ shortcut khi `egui_wants_keyboard_input` hoặc modal mở;
    `Space` chỉ toggle ở Library, `B` chỉ ở Develop.
30. **`add_imported` không load proxy** (Library-first); `ensure_current_loaded` chỉ
    load khi vào Develop; `select_and_load` chống gọi lặp khi đang loading.
31. **Clear library khi export:** export dùng snapshot; right panel vẫn hiện nếu
    `state.export.is_some()` để còn Cancel/Dismiss.
32. **PNG EXIF** qua `PngEncoder::set_exif_metadata`; TIFF encoder của `image` chưa
    nhúng EXIF — UI ghi rõ.
33. **Open/Reveal** dùng `std::process::Command` (`open`/`explorer`/`xdg-open`),
    không thêm crate.
34. **Real-sample exporter test** với `RAW_DIR` (ignored) đã pass trên NEF thật;
    rawloader vẫn có thể panic ở DNG lạ nhưng được catch và báo unsupported.
35. **Duplicate output names:** hai RAW khác thư mục có thể cùng stem; `export_batch`
    phát hiện trùng output trước khi chạy, báo failure cho job sau thay vì để hai
    thread ghi đè nhau. Kiểm tra lại `overwrite` ngay trước rename để `Skip` vẫn đúng.
36. **Proxy failure phải có token/index:** `Message::ProxyFailed` thay cho lỗi chung;
    `AppState.proxy_failed` chặn auto-load lại trong Develop. Nếu không, một RAW
    unsupported bị load lại mỗi frame, tạo toast/log spam.
37. **Library epoch:** `clear_library` tăng `library_gen`; `Scanned`/`Probed`/`Thumbnail`
    mang epoch và bị bỏ nếu lệch. Không có epoch, kết quả cũ có thể ghi đè entry mới
    theo index sau khi clear.
38. **Row list:** không đặt `ui.interact` toàn row sau checkbox/thumbnail; nó nuốt click
    của checkbox. Dùng response của từng widget (checkbox, thumbnail, tên) và tooltip
    trên response của layout, không cần row-level click.
39. **Flush file:** không dựa vào `BufWriter` drop (lỗi flush bị bỏ). Dùng `File` trực
    tiếp + `sync_all()` trước khi rename file tạm.
40. **Export finished:** `begin_export` chỉ chặn khi session chưa finished; UI dùng
    `!session.finished` cho label/active. Nếu không, nút Export đứng ở “Exporting…”
    sau khi xong và không cho chạy batch mới cho tới khi Dismiss.
41. **`embed-resource` 3.x:** API là `compile(rc, NONE).manifest_optional()`
    (`CompilationResult` là `#[must_use]`). Dùng
    `[target.'cfg(windows)'.build-dependencies]` + `#[cfg(windows)]` trong `build.rs`
    để macOS không cần windres. Đặt `rawpeg.rc` ở crate root và tham chiếu
    `assets/rawpeg.ico` để tránh lệch đường dẫn.
42. **ICO PNG payload:** Windows Vista+ chấp nhận entry là PNG; kích thước 256 ghi
    byte width/height = 0. Test phải đọc ICONDIRENTRY (offset 6 + i*16) để kiểm tra
    payload và thứ tự kích thước.
43. **macOS icon/version:** cần cả `CFBundleIconFile` và file trong
    `Contents/Resources/`. Version bundle được `plutil -replace` từ `Cargo.toml`
    trong `package_macos.sh`, tránh Info.plist hardcode lệch tag.
44. **Release gate:** preflight so tag `v{version}` với `Cargo.toml` trước khi build;
    `cargo test --locked` chạy trong matrix; job release chỉ tải artifact
    (`pattern: rawpeg-*`, `merge-multiple`) rồi mới tạo release + checksums.
45. **Icon runtime:** `eframe::icon_data::from_png_bytes` + `with_icon` cho cửa sổ;
    icon trong `.exe` (Explorer) vẫn phải nhúng riêng bằng `embed-resource`.
46. **App chưa ký:** macOS Gatekeeper (“unidentified developer”) và Windows
    SmartScreen sẽ cảnh báo; signing/notarization là follow-up ngoài M5.

## 8. Command thường dùng
```bash
cargo check                    # gate biên dịch
cargo test                     # 91 passed, 4 ignored
cargo clippy --all-targets -- -D warnings
cargo build                    # debug
./scripts/verify_local.sh      # gate 4 bước đầy đủ
cargo run                      # chạy GUI batch-first

cargo test --test exporter_tests        # 11 test export
cargo test --test app_worker_tests      # 8 test worker/state
cargo test --test ui_smoke_tests        # 2 test render headless
cargo test --test assets_tests          # 5 test icon/plist (M5)

# Icon assets (macOS; output đã commit, chỉ chạy khi đổi icon):
python3 scripts/make_icons.py

# Release M5 (CHỈ người dùng chạy — không push/tag từ môi trường làm việc):
git push origin main
git tag -a v0.1.0 -m "rawpeg v0.1.0"
git push origin v0.1.0

# RAW thật (ignored):
RAW_SAMPLE=dir/canon_6d.cr2 cargo test --test decoder_tests decode_real_sample -- --ignored
RAW_DIR=rawimage cargo test --test real_pipeline_tests -- --ignored --nocapture

cargo run --example render_previews -- rawimage --max-dim 2560
```

## 9. Giới hạn hiện tại / follow-up
- Demosaic chất lượng cao (AHD/DCB hoặc rawler 0.8/libraw) chưa làm.
- TIFF chưa nhúng EXIF; PNG/TIFF 8-bit (chưa có 16-bit output).
- Chưa có preset export, undo/redo, lưu settings giữa các lần chạy, batch auto-tone
  theo từng ảnh, hay hỗ trợ camera mới hơn 2019.
- Icon hiện tại là placeholder sinh bởi `scripts/make_icons.py`; có thể thay artwork
  bằng PNG 1024×1024 rồi chạy lại script.
- App chưa ký/notarize: macOS có thể báo “unidentified developer”, Windows SmartScreen
  cảnh báo; Windows `.exe` chưa có VERSIONINFO (chỉ icon).
- CI release tag: code sẵn sàng nhưng cần người dùng push tag `v*` và xác nhận
  (M5 chỉ đổi ✅ sau đó). Không push/trigger CI từ môi trường làm việc này.
