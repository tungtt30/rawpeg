# rawpeg — Kế hoạch phát triển (Implementation Plan)

Trang này là nguồn tham chiếu duy nhất cho toàn bộ dự án **RAW → JPEG batch
processor** (tên app: `rawpeg`). Cập nhật trạng thái sau mỗi milestone và ghi
lại các quyết định kỹ thuật cùng bài học khi triển khai để không quên.

---

## 1. Tổng quan & Tech Stack (đã chốt)

| Hạng mục | Lựa chọn | Ghi chú |
|---|---|---|
| Ngôn ngữ | Rust 2024 edition, toolchain 1.97+ | `eframe 0.37` cần rust ≥ 1.95 |
| GUI | `eframe` / `egui` 0.37 | **Trait đổi**: `App::ui(&mut self, ui, frame)` thay vì `App::update(ctx, frame)`; `CentralPanel::show(ui, ...)` |
| RAW engine | **`imagepipe` 0.5.1** (+ `rawloader` 0.37.2 kèm sẵn) | Pipeline đầy đủ: gofloat → demosaic → toLab → basecurve → fromLab → gamma |
| JPEG export (M4) | **`jpeg-encoder` 0.7.1** | Pure Rust; `set_sampling_factor` (4:4:4); `add_exif_metadata`; `add_icc_profile`; quality 100 |
| Xử lý ảnh | `image` 0.25, `rayon` 1 | resize thumbnail, đa luồng |
| EXIF đọc | `kamadak-exif` 0.6 (crate name: **`exif`**) | lib rename thành `exif`; `Reader::new().read_from_container(&mut BufReader<File>)` |
| EXIF ghi (M4) | Nhúng block EXIF thô qua `jpeg-encoder::Encoder::add_exif_metadata` | Trích từ file gốc |
| File dialog | `rfd` 0.18 | Native picker |
| I/O + lỗi | `anyhow` 1, `log` + `env_logger` | |
| Đa luồng | `std::sync::mpsc` + `rayon` | UI thread không bao giờ block |

## 2. Kiến trúc & Cấu trúc dự án

```
rawpeg/
├── Cargo.toml
├── PLAN.md                        # file này
├── .github/
│   ├── workflows/ci.yml           # ✅ CI build check: Windows x64 + macOS universal
│   └── macos/Info.plist           # bundle metadata cho rawpeg.app
├── scripts/
│   ├── fetch_samples.sh           # tải RAW mẫu CC0 từ raw.pixls.us
│   └── package_macos.sh           # gói binary(s) → rawpeg.app + zip (CI + local)
├── src/
│   ├── lib.rs                     # pub mod app / models / raw_engine
│   ├── main.rs                    # eframe entry (windows_subsystem cho release)
│   ├── app/                       # ✅ M3 XONG: darkroom UI
│   │   ├── mod.rs                 # RawpegApp + eframe::App::ui + dark theme
│   │   ├── state.rs               # AppState: library, viewport, clipboard, histogram
│   │   ├── worker.rs              # thread + mpsc + job coalescing (không block UI)
│   │   └── panels.rs              # top bar / library / viewport / adjustments
│   ├── raw_engine/
│   │   ├── decoder.rs             # ✅ M1 XONG
│   │   ├── processor.rs           # ✅ M2 XONG
│   │   ├── auto_tone.rs           # ✅ M2 XONG
│   │   └── exporter.rs            # ⏳ M4: batch export rayon + jpeg-encoder
│   └── models/
│       ├── edit_params.rs         # ✅ M1 XONG (EditParams + tests)
│       └── image_item.rs          # ✅ M1 XONG (ImageItem, ExifSummary, EditStatus)
└── tests/
    ├── common/mod.rs              # fixture DNG 12x12 RGGB tự sinh (TIFF writer)
    ├── decoder_tests.rs           # ✅ 8 tests + 1 ignored (RAW thật)
    ├── processor_tests.rs         # ✅ 6 tests (apply/encode_srgb/auto_tone trên fixture)
    ├── app_worker_tests.rs        # ✅ M3: 5 tests worker jobs + headless AppState::poll
    └── real_pipeline_tests.rs     # ✅ ignored: full pipeline trên RAW thật (RAW_DIR, mặc định ./rawimage)
```

Ngoài ra (công cụ dev, không nằm trong `cargo test` gate):
```
examples/render_previews.rs      # render PNG+JPEG full-res để soi bằng mắt (--max-dim N)
```

## 3. Data flow (bắt buộc giữ đúng)

```
RAW file
 └─ imagepipe::Pipeline::new_from_file(path)
     ├─ maxwidth/maxheight = giới hạn cạnh dài (0 = full-res)
     ├─ output_16bit(None)  → SRGBImage16  (KHÔNG gamma khi settings.linear=true
     │     → data là LINEAR f32 scale 0..65535)      ─── DÙNG CHO CHỈNH SỬA
     └─ output_8bit(None)   → sRGB u8     (có basecurve + gamma) ── DÙNG CHO THUMBNAIL
```

- **Buffer chỉnh sửa:** `Vec<[f32;3]>` linear, chuẩn hóa `0.0..=1.0` (`/65535.0`).
  Mọi phép toán (Exposure, Contrast, Highlights/Shadows, WB) chạy **trong không
  gian linear**; chỉ sRGB-encode ở bước cuối (hiển thị / export).
- **Base tone curve** của imagepipe bị tắt khi decode linear
  (`pipeline.ops.basecurve.points = vec![]; .exposure = 0.0`) để giữ highlight headroom.
- **Proxy image:** decode full-res 1 lần khi chọn ảnh → downscale ~1600px cho
  slider real-time (<16ms); chỉ decode full-res lúc Export.
- **EXIF:** đọc qua `exif` crate → `ExifSummary` (Model, ISO, Aperture, Shutter, FocalLength).
  Khi export: chèn block EXIF TIFF thô trích từ file gốc.

## 4. Tính năng (theo spec)

- **Format RAW:** CR2/CR3/NEF/ARW/RAF/ORF/RW2/DNG (+ srw/pef/kdc/erf/... danh sách trong `RAW_EXTENSIONS`).
- **Preview nhanh:** thumbnail qua pipeline ở kích thước nhỏ (rawloader 0.37 **không có** API `decode_thumb`).
- **Điều chỉnh:** Exposure EV (-3..3), Contrast (-50..50, S-curve quanh 0.18),
  Highlights/Shadows (-100..100, luminance mask), WB Temp (2000K-10000K) + Tint (-100..100) + "As Shot".
- **Auto tone:** histogram linear RGB → exposure đưa median về ~18% gray, clip ≤0.5% pixel.
- **UI darkroom:** top bar (Open Folder/Add Files, EXIF, Export Batch), left sidebar
  (thumbnails + checkbox), center viewport (zoom/pan + Before/After giữ chuột),
  right sidebar (sliders, Auto Balance, Reset, Copy/Paste settings, histogram RGB live).
- **Export batch:** rayon, JPEG quality 98, 4:4:4, resize tùy chọn (4000px), copy EXIF,
  progress + Cancel.

## 5. Milestones & trạng thái

### ✅ Milestone 1 — Setup + Core RAW Pipeline (XONG)
- Cargo.toml + scaffold đầy đủ, decoder + models + tests.
- **Gate đã qua:** `cargo check`, `cargo test`, `cargo clippy --all-targets` (0 warning), `cargo build` debug + release, smoke test GUI, decode thật CR2/NEF.

### ✅ Milestone 2 — Image Processing & Auto-Exposure (XONG)
1. `processor.rs`: `apply(&DecodedImage, &EditParams) -> Vec<[f32;3]>` (rayon `par_iter`).
   - Thứ tự cố định: **WB → exposure → highlights/shadows → contrast**.
   - exposure: nhân `2^ev` trên linear.
   - contrast: S-curve power quanh 0.18: `0.18·(c/0.18)^γ`, `γ = 1 + contrast·0.01`.
   - highlights/shadows: mask lũy thừa bậc 2 theo luminance
     (`(1−Y/0.5)²` dưới mid, `((Y−0.5)/0.5)²` trên mid), signed param, additive lift / multiplicative compress.
   - WB: gain `[r,g,b]` tuyến tính tương đối as-shot; identity tại 6500K/0;
     temp↑ → đỏ lên/xanh xuống; tint>0 → magenta.
   - Buffer linear KHÔNG bị clip giữa pipeline; chỉ rolloff/clip lúc encode.
2. `encode_srgb(&[[f32;3]]) -> Vec<u8>` — sRGB piecewise chuẩn + **soft shoulder**
   cho giá trị > 0.8: `0.8 + 0.2·(1 − e^(−5(c−0.8)))`, C1-continuous tại 0.8, tiệm cận 1.0.
3. `auto_tone.rs`: `auto_exposure(&DecodedImage) -> EditParams` — histogram luminance 256 bins;
   exposure = `log2(0.18/median)` clamp ±3 EV; highlights từ 99.5th percentile sau exposure
   (clip ≤ 0.5%); shadows từ 1st percentile; WB giữ nguyên, `is_auto_applied = true`.
4. Tests: 17 unit (processor 12 + auto_tone 5) + 6 integration trên fixture DNG.
- **Gate đã qua:** `cargo check`, `cargo test` (31 tests), `cargo clippy --all-targets` (0 warning), `cargo build` debug.

### ✅ Milestone 3 — Darkroom UI (XONG, branch `milestone-3`)
1. **Worker thread** (`app/worker.rs`): `std::sync::mpsc`, `Job` → `Message`; UI poll
   `try_recv` mỗi frame, worker gọi `ctx.request_repaint()` sau mỗi kết quả.
   - `Job`: ScanDir · LoadThumbnail · LoadProxy · RenderAfter · AutoTone.
   - **Coalescing**: mỗi vòng lặp drain channel; chỉ giữ `LoadProxy`/`AutoTone`/
     `RenderAfter` **mới nhất**, thumbnails giữ hết theo thứ tự; proxy luôn chạy trước render.
   - Proxy linear ~1600px giữ trong worker (UI không giữ bản sao); `RenderAfter` tái dùng nó.
   - **Proxy token**: tăng mỗi lần chọn ảnh; message cũ bị bỏ (chống race khi đổi ảnh nhanh).
2. **State** (`app/state.rs`): library (`LibraryEntry` = ImageItem + probe status + thumbnail
   texture), viewport (zoom/pan), clipboard params, `RgbHistogram` 3×256 bins từ buffer linear
   sau apply. `poll()` áp message → tạo/cập nhật texture bằng `ctx.load_texture`/`TextureHandle::set`.
3. **UI** (`app/panels.rs`, theme trong `app/mod.rs`):
   - Top bar: Open Folder (`rfd`) → ScanDir; Add Files → thêm trực tiếp; EXIF của ảnh chọn;
     Export Batch disabled (M4).
   - Library trái: thumbnail + checkbox batch + badge `edited/original`; file unsupported hiện
     lý do từ `DecodeStatus` (guard M2).
   - Viewport giữa: fit-to-view, zoom bằng scroll wheel (0.1×–16×), pan bằng drag,
     double-click reset; **giữ Space = Before**, thả = After; overlay % và nhãn BEFORE.
   - Adjustments phải: sliders Exposure/Contrast/Highlights/Shadows/Temp/Tint; Auto Balance,
     Reset, Copy/Paste settings; histogram RGB live.
4. Tests: 12 unit trong `app` (state 8 + worker 4: coalescing/token/badge/paste) +
   5 integration `tests/app_worker_tests.rs` (worker jobs + `AppState::poll` headless trên fixture).
- **Gate đã qua:** `./scripts/verify_local.sh` ALL GREEN (`cargo check`, `cargo test` 33 lib +
  19 integration, `cargo clippy --all-targets` 0 warning, `cargo build`), smoke test `cargo run`
  mở cửa sổ không panic.

### ⏳ Milestone 4 — Batch Exporter
- `exporter.rs` với rayon; decode full → edit → sRGB → `jpeg-encoder` (quality 98, 4:4:4) → ghi EXIF/ICC.
- Progress modal + Cancel.
- ⚠️ **Đánh giá demosaic chất lượng cao** (xem mục 7).

### ⏳ Milestone 5 — GitHub Actions CI/CD (đang làm dở)
- ✅ **CI build check đã chạy thành công** (run #45201774320, 2026-11-23):
  - `windows-latest` → `rawpeg-windows-x64.zip` (rawpeg.exe, PE32+ GUI x86-64, 8.8MB)
  - `macos-latest` → `rawpeg-macos-universal.zip` (rawpeg.app, fat binary x86_64+arm64, 12.2MB)
- ⏳ Còn lại: trigger tag `v*` → tạo GitHub Release tự động (softprops/action-gh-release),
  icon Windows (dùng `embed-resource`, không dùng `winres` đã stale).

## 6. Testing strategy
- **Fixture DNG 12x12 RGGB** sinh trong `tests/common/mod.rs` (TIFF writer thủ công):
  Make/Model, DNGVersion, ColorMatrix1 = identity, AsShotNeutral (1,1,1), IFD raw
  với 16-bit uncompressed strip. **Quy tắc quan trọng:** `count` của TIFF entry là
  số VALUE (không phải số byte); imagepipe cần ảnh ≥ 10x10 (`owidth-10`).
- Test ignored `decode_real_sample_requires_env`: cần `RAW_SAMPLE=<path>`
  (xem `scripts/fetch_samples.sh`).
- **Real-sample pipeline test** `real_pipeline_tests.rs` (ignored): chạy toàn bộ
  pipeline trên mọi RAW trong thư mục `RAW_DIR` (mặc định `./rawimage`):
  `RAW_DIR=rawimage cargo test --test real_pipeline_tests -- --ignored --nocapture`.
  File decode-fail do rawloader không hỗ trợ được báo `UNSUPPORTED` (không fail
  test), nhưng nếu **không file nào** decode được thì test fail.
- Không assert channel-dominance trên fixture sạch — demosaic của imagepipe là
  window-average nên màu tinh khiết bị trộn (xem mục 7).

## 7. Bài học / rủi ro đã ghi nhận (quan trọng)

1. **Demosaic imagepipe là fast-path thô** (neighbor averaging, không AHD/DCB).
   Preview/edit tạm chấp nhận được; **M4 nên cân nhắc demosaic riêng** trên
   `rawloader::RawImage` (CFA + white/black level) để chất lượng export tối đa.
2. **Thumbnail:** rawloader 0.37 không expose `decode_thumb` → dùng pipeline
   nhỏ. Nếu cần preview <50ms/ảnh sau này, cân nhắc `rawler 0.8` (dnglab) làm
   nguồn thumbnail riêng.
3. **egui 0.37 API mới** (`App::ui`, `CentralPanel::show(ui,...)`) — đừng quay lại
   `App::update`.
4. **`kamadak-exif` lib name = `exif`** khi import.
5. **`output_16bit` KHÔNG gamma** (linear) dù struct doc ghi "sRGB": dựa vào `settings.linear`.
6. **Basecurve** phải tắt để có highlight headroom trong buffer chỉnh sửa.
7. **CI: không có thư viện C ngoài** (mọi crate đều pure Rust) — bỏ `mozjpeg`/`libraw-sys`.
8. **Runner macOS Intel đã retire** (`macos-13`/`macos-14` không còn trên GitHub Actions,
   11/2026). Standard runner `macos-latest` chỉ là ARM64. Giải pháp: build **universal
   binary** (lipo) ngay trên runner arm64 — 1 artifact chạy cả Intel lẫn Apple Silicon.
9. **Soft shoulder cho highlight (M2):** knee dạng `1 + w(1−e^(−(c−1)/w))` gần như vô dụng
   trong 8-bit (mọi c > 1.0 đều ra cùng code 255). Phải đặt shoulder bắt đầu tại **0.8 linear**
   (`0.8 + 0.2·(1−e^(−5(c−0.8)))`, w = 0.2 để C1-tiếp nối) mới thấy rolloff (1.0 → ~247, 4.0 → 255).
10. **Histogram quantization trong auto-tone:** giá trị bin = bin/255 → test phải dùng tolerance
    (≈ ±0.05 EV) thay vì so sánh chính xác; tail > 1.0 bị nhét vào bin 255, nên highlights phải
    tính trên `hi_after = hi·2^ev` chứ không so hi với 1.0.
11. **Thứ tự pipeline cố định:** WB → exposure → H/S → contrast. Mask H/S tính trên luminance
    sau WB+exposure (theo "look" hiện tại); contrast là S-curve power quanh 0.18 với fixed point
    chính xác trong f32 (`c/c = 1.0`) giúp test deterministic.
12. **rawloader 0.37.2 chỉ hỗ trợ camera table tới ~2019** (Sony đến ILCE-7M3/ILCE-6500). Verify
    11/2026 trên `rawimage/`: Canon 60D CR2, Nikon D750 NEF, Sony ILCE-6500 ARW decode OK;
    **fail** — Sony ILCE-7M4/7RM5 ARW (`Couldn't find camera` trong `check_supported`) và DNG
    Samsung Galaxy S25 (compression `0xffffcd42` → panic `ifds[0]` trong `dng.rs`, bị catch →
    "Don't know how to decode"). **Guard đã thêm**: `decoder::probe_support(path)` probe nhẹ
    (rawloader `decode_dummy`, không decode pixel) trả về `DecodeStatus::Supported |
    UnsupportedCamera(reason) | UnsupportedFormat(reason) | Error(reason)` — UI import dùng để
    báo rõ "camera chưa được hỗ trợ" thay vì lỗi chung chung, và `scan_directory` không phải
    bỏ file. M4 cân nhắc `rawler 0.8`/libraw để mở rộng hỗ trợ máy ảnh mới.
13. **Thêm `--max-dim` cho tool render preview:** full-res 9 ảnh ~1GB đĩa + ~2 phút; cap
    `--max-dim 2560` đủ soi chi tiết mà đĩa nhanh hơn nhiều.
14. **Coalescing job trong worker (M3):** kéo slider sinh ra `RenderAfter` mỗi frame; worker
    drain channel rồi chỉ giữ **request mới nhất** cho `LoadProxy`/`AutoTone`/`RenderAfter`
    (thumbnails giữ hết, theo thứ tự). Nếu không coalesce, hàng đợi phình và ảnh hiển thị trễ.
    Proxy load phải chạy **trước** render trong cùng batch để render áp lên ảnh mới.
15. **Proxy token chống race:** mỗi lần select tăng `proxy_token`; `ProxyReady`/`RenderReady`/
    `AutoToneReady` mang token, UI bỏ message lệch token. Không có nó, chuyển ảnh nhanh sẽ render
    kết quả của ảnh cũ đè lên ảnh mới.
16. **Texture upload egui 0.37:** `ctx.load_texture` để tạo, `TextureHandle::set` để cập nhật tại
    chỗ. Proxy ~1600px upload mỗi tick slider vẫn mượt; không cần tạo texture mới mỗi frame
    (rò rỉ texture manager nếu tạo liên tục).
17. **Panels egui 0.37:** `egui::Panel::{top,left,right}(id).show(ui, ...)` (không còn
    `SidePanel`/`TopBottomPanel`), `CentralPanel::default().show(ui, ...)`; `App::ui` nhận `&mut Ui`
    gốc — panel thêm theo thứ tự, CentralPanel cuối cùng.
18. **Test UI headless:** `eframe::egui::Context::default()` có `max_texture_side = 2048`, đủ để
    gọi `AppState::poll` (load texture) trong integration test mà không cần mở cửa sổ — dùng
    sleep-poll có deadline, không flaky.
19. **Before/After = giữ Space** (đã chốt thay cho "giữ chuột" trong spec gốc) để chuột trái
    rảnh cho pan; zoom bằng scroll wheel, double-click reset viewport.

## 8. Command thường dùng
```bash
cargo check                    # gate biên dịch
cargo test                     # unit + integration
cargo clippy --all-targets     # 0 warning bắt buộc
./scripts/verify_local.sh      # gate 4 bước (check/test/clippy/build)
cargo run                      # chạy GUI darkroom
cargo test --test app_worker_tests   # worker jobs + AppState::poll headless
./scripts/fetch_samples.sh [dir]
RAW_SAMPLE=dir/canon_6d.cr2 cargo test --test decoder_tests decode_real_sample -- --ignored
RAW_DIR=rawimage cargo test --test real_pipeline_tests -- --ignored --nocapture
cargo run --example render_previews -- rawimage --max-dim 2560
```
