# Architecture Improvement Plan

**Status** — Owner decisions recorded on November 27, 2026. Items in this plan are not part of the [roadmap](roadmap.md) until the owner approves them there. Once approved, they follow the roadmap's sign-off workflow in [`AGENTS.md`](../AGENTS.md).

This plan consolidates two independent architecture reviews of OpenPaintR. Both reviews were checked against the working tree (base commit `92fdc75` plus uncommitted changes as of November 27, 2026). It covers maintainability, security, efficiency, and performance. Each item lists the problem, the change, the files involved, the acceptance criteria, and how to verify it. Items cite function and type names instead of line numbers because the tree is changing quickly.

## Principles

- **Measure first.** Performance work starts from the per-tool benchmarks in Stage 0 and records numbers before and after each change. The speedups estimated in the reviews (for example 10×–50× for transforms) are hypotheses until measured.
- **No behavior change unless stated.** `tests/pdn_compat.rs`, the phase 1 workflows, and every headless UI test must pass after each item. Optimizations must produce byte-identical pixels unless the item says otherwise.
- **Small, independently mergeable steps.** One item per branch and pull request. Mechanical refactors (moves and renames) never share a pull request with behavior changes.
- **Project rules still apply.** New user-facing text gets stable keys in all ten catalogs in `translations/`. Settings and shortcut files preserve existing customizations. Paint.NET compatibility is validated against Paint.NET 5.2 only.

## Summary

| Stage | Theme | Items | Rough size | Depends on |
| --- | --- | --- | --- | --- |
| 0 | Guardrails | CI, translation check, per-tool benchmarks, `unsafe` lint | S–M | — |
| 1 | Security quick wins | Decode limits, translation loading, bounded reads, XDG paths and portable mode, English names | S–M | 0.1 |
| 2 | Performance quick wins | `Pixel` zero-copy, affine resampling, gradient, vector rasterizer, outline-based text, thumbnails, save dialog | S–M each | 0.3 |
| 3 | History memory | Rectangle snapshots, accurate accounting, global budget, Clone Stamp | M | 0.3 |
| 4 | Preview pipeline | `Arc` masks, composite cache, session dirty rectangles, parallelism | M | 2.1, 3.1 |
| 5 | Maintainability | Split `gui/mod.rs`, `ToolController`, `ToolSettings`, typed core API, i18n decoupling, Cargo workspace with a separate CLI crate, CLI on the engine | M–L | 0 |
| 6 | Structural (phases 4 and 5) | Copy-on-write tiles, tiled textures, asynchronous I/O, `wgpu` renderer with `glow` fallback, plugin host hardening | L | 4, 5 |

Sizes are relative guidance only: **S** is under a day, **M** is one to three days, and **L** is one to two weeks.

## Owner decisions

Recorded on November 27, 2026. The items below already reflect them.

1. **Text follows the global antialiasing mode through outline-based coverage.** The editor uses egui only for text layout (glyph positions, line breaks, bidirectional text) and rasterizes glyph outlines with the same rasterizer as shapes (item 2.5). Gradients keep 4×4 supersampling of the color ramp: it smooths conical and spiral seams inside the ramp and is not edge antialiasing. Gradient edges follow the selection's coverage.
2. **Plugin commits use the selection's own coverage.** The host blends plugin results using the selection's `TMask` coverage as rasterized under the antialiasing mode that was active when the selection was created. There is no re-rasterization. `AGENTS.md`, `docs/plugin-architecture.md`, `docs/extensibility.md`, and the open phase 5 roadmap items now say "selection coverage" instead of "4×4 SSAA coverage".
3. **External translations load only from trusted locations**, with this precedence (later locations win): bundled catalogs → `$XDG_DATA_DIRS/openpaintr/translations` (for example `/usr/share`) → `translations/` beside the executable → `$XDG_DATA_HOME/openpaintr/translations`. The current working directory is never scanned (item 1.2).
4. **Portable mode uses a marker file.** When a file named `portable` exists beside the executable, configuration, state, and user translations are stored beside it. `OPENPAINTR_DATA_DIR=<path>` redirects them for development and tests (item 1.4).
5. **English names everywhere.** The runtime files become `shortcuts.json`, `palette.txt`, and `recent.json` during the XDG migration, and the schema becomes `docs/shortcuts.schema.json` (item 1.4). Portuguese document names and internal egui IDs are renamed in a separate mechanical pull request (item 1.5).
6. **The history budget is proportional to RAM.** One budget for all tabs: 25% of physical memory (read from `/proc/meminfo`), clamped between 512 MiB and 4 GiB, and adjustable in Settings (item 3.3).
7. **`wgpu` becomes the renderer, with `glow` kept as a fallback.** `wgpu` is the target so that the UI and WGSL plugins share one device. A validation of one to two days gates the switch. `glow` stays compiled in and is selected automatically under WSL or when no usable `wgpu` adapter exists (item 6.4). The development environment moved from WSL2 to native Ubuntu 26.04, so the performance baseline is recorded again (item 0.3).
8. **Copy-on-write tiles are 64 × 64**, aligned with `TMask` and `StrokeTile`. The design note for item 6.1 confirms the size with a benchmark.
9. **CLI scope.** The first commands are `info` (with `--json`), `convert` (with `png` and `pdn` kept as aliases), `flatten`, `resize`, `canvas-size`, `rotate`, `flip`, `export-layer`, and batch mode with `--out-dir`. Arguments are parsed with `clap` derive from the first new command, so the same argument structs can later generate MCP tool schemas. CLI output stays in English, and `AGENTS.md` records this exception (item 5.7).

---

## Stage 0 — Guardrails

### 0.1 Continuous integration

- **Problem.** The repository has no CI. Formatting, lints, tests, and dependency audits run only by hand.
- **Change.** Add `.github/workflows/ci.yml` on `ubuntu-latest` with the Rust 1.98 toolchain (the README's minimum) and stable:
  - `cargo fmt --all --check`
  - `cargo clippy --locked --all-targets --all-features -- -D warnings`
  - `cargo test --locked` and `cargo test --locked --no-default-features` (CLI-only build)
  - `cargo deny check` with a new `deny.toml` covering advisories, licenses, bans, and sources
  - Cache `~/.cargo` and `target/`. Install the system packages that eframe needs to compile, such as `libwayland-dev`, `libxkbcommon-dev`, and the OpenGL headers.
- **Notes.** If the first clippy run reports many warnings, fix them in a dedicated pull request or start with `-W` and tighten to `-D` afterwards.
- **Acceptance.** CI passes on `main` and is a required check for pull requests.

### 0.2 Translation catalog check

- **Problem.** `AGENTS.md` requires matching keys and placeholders across the ten catalogs, but nothing verifies it.
- **Change.** Add a test, for example `tests/translations.rs` or a test in `src/gui/i18n.rs`, asserting that every bundled catalog has exactly the keys of `en-us.json`, the same numbered placeholders (`{0}`, `{1}`, …), and no empty values.
- **Acceptance.** Removing a key from any catalog, or changing one of its placeholders, fails CI.

### 0.3 Per-tool performance baseline

- **Problem.** `phase1_interactive_performance_baseline` measures opening, strokes, zoom, and export only. The bottlenecks found in the reviews (transforms, gradients, shapes, text, bucket fill, thumbnails) have no numbers yet.
- **Environment.** The phase 1 baseline in the roadmap was recorded on WSL2 (`Linux 6.18.33.2-microsoft-standard-WSL2`, i7-13700K). Development moved to native Ubuntu 26.04 (decision 7), so first record the phase 1 workloads again on the new environment and note the change in the roadmap. Compare later numbers only with measurements from the same environment.
- **Change.** Extend the benchmark, or add an `#[ignore]` test run with `--release --ignored`, using the existing typical (800 × 600, 4 layers) and large (2560 × 1440, 8 layers) workloads:
  - Transform drag of a whole layer and of a 64 × 64 floating paste; Rotate/Zoom preview
  - Gradient drag (linear and conical); large star shape drag; freeform shape
  - Typing text at 24, 96, and 240 px
  - Global and contiguous bucket fill; Magic Wand; Clone Stamp stroke
  - Layer opacity drag; thumbnail refresh after a stroke
  - Compression slider in the save options dialog; PDN open and save; PNG export
  - **Metrics:** median and maximum frame time, `VmHWM`, and undo bytes per history entry.
- **Output.** JSON lines in `target/perf/`, archived by CI but not used to fail builds. Record the baseline table in the roadmap's phase 4 **Performance** item.
- **Acceptance.** Every later performance item reports before and after numbers from this benchmark.

### 0.4 `unsafe` lint for the whole binary

- **Problem.** `#![forbid(unsafe_code)]` covers only `src/lib.rs`. The GUI, which is most of the code, has no lint, although it needs a single `unsafe` block (`Backend::from_foreign_display` in `src/gui/clipboard/wayland.rs`).
- **Change.** Add `#![deny(unsafe_code)]` and `#![deny(unsafe_op_in_unsafe_fn)]` to `src/main.rs`. Put `#[expect(unsafe_code, reason = "...")]` on that statement only. Keep `forbid` in `lib.rs`. `forbid` cannot be overridden locally, so it can't be used in the binary.
- **Acceptance.** Any new `unsafe` outside the Wayland statement fails to compile.

---

## Stage 1 — Security quick wins

### 1.1 Decode and allocation limits

- **Problem.**
  - `decode_png_rgba` (`src/png_io.rs`) allocates `output_buffer_size()` before checking any pixel limit. A PNG of about 100 KB declaring 32768 × 32768 requests 8 GiB. With Linux overcommit, `try_reserve_exact` succeeds and filling the buffer can exhaust memory.
  - Images pasted through Wayland (`receive(offer, PNG, MAX_IMAGE_BYTES)`) and files referenced by the clipboard (`clipboard::files::read_image`) are checked for compressed size only, not for dimensions.
  - `Document::new`, layer insertion, image resizing, and canvas resizing rely on UI limits (`max_texture_side` and `max_canvas_side`) instead of enforcing `MAX_PIXELS` in the core.
- **Change.**
  - Move `MAX_PIXELS` from `pdn_io` to a core `limits` module and expose `check_image_dimensions(width, height, layers)`, built on `check_layer_pixel_limit`.
  - In `decode_png_rgba`, check `reader.info()` dimensions right after `read_info()` and before allocating. Set explicit `png::Limits`.
  - Enforce the limit in `Document::new`, `insert_blank_layer`, resizing, and canvas resizing, counting pixels × layers.
  - In `clipboard/files.rs`, open the `File` once and build `ImageReader::new(BufReader::new(file)).with_guessed_format()`. Set `image::Limits` (`max_image_width`, `max_image_height`, `max_alloc`) from `MAX_PIXELS`, and check the decoder dimensions before decoding. Opening the file once also removes the metadata/open race, a robustness issue rather than a security one.
  - Route PNG bytes from the Wayland clipboard through the same limited decoding path.
  - Add translation keys for the new error messages to all ten catalogs.
- **Acceptance.**
  - Small crafted PNGs declaring 32768 × 32768 and 65535 × 1 are rejected with a localized error, and peak RSS grows by less than 10 MiB during the test.
  - The same file is rejected through a `file://` URI and through the bytes path.
  - Neither the GUI nor the CLI can create a document above `MAX_PIXELS`.
  - Existing fixtures still open.

### 1.2 Translation loading

- **Problem.** At startup, `CATALOGS` (`src/gui/i18n.rs`) loads every `translations/*.json` from the current working directory and overrides bundled strings. Launching OpenPaintR from a downloaded folder lets that folder change interface labels (for example, turning "Save" into "Discard"). `fs::read` has no size cap and blocks forever on a FIFO named `*.json`.
- **Change.** Remove the `current_dir()` scan. Load catalogs in this order, each location overriding the previous ones (decision 3):
  1. bundled catalogs;
  2. `$XDG_DATA_DIRS/openpaintr/translations` (default `/usr/local/share` and `/usr/share`);
  3. `translations/` beside the executable;
  4. `$XDG_DATA_HOME/openpaintr/translations` (default `~/.local/share/openpaintr/translations`).

  In portable mode (item 1.4), the user location is the portable data directory. Accept only regular files of up to 1 MiB. Update the **Interface language** section of the README.
- **Acceptance.** A `translations/pt-br.json` in the working directory has no effect. A FIFO named `xx-xx.json` in a search directory does not block startup. Files over the cap are skipped. Tests confirm the precedence order.

### 1.3 Bounded reads

- **Problem.** `storage::read_json`, palette loading (`ColorPalette::new` and `load_from` in `colors.rs`), and `Catalogs::scan` read whole files without a cap.
- **Change.** Add `storage::read_capped(path, max) -> io::Result<Vec<u8>>`, which opens the file, requires `metadata().is_file()`, and reads with `take(max + 1)`. Use it for settings, language, shortcuts, and recent files (1 MiB), the palette (64 KiB), and catalogs (1 MiB). Add a translated error for oversized files.
- **Acceptance.** Oversized or non-regular files produce an error or fall back to defaults. They never hang or allocate without bound.

### 1.4 XDG paths, portable mode, and migration

- **Problem.** `settings.json` and `language.json` use `$XDG_CONFIG_HOME/openpaintr/`, while `atalhos.json` (`shortcuts.rs`), `paleta.txt` (`colors.rs`), and `recentes.json` (`dialogs.rs`) use `exe_adjacent`. Installed in `/usr/bin`, the application cannot save them. In a shared, writable directory, users can read or modify each other's shortcuts and recent-file lists.
- **Change.**
  - **Locations.** Settings, language, shortcuts, and palette go to `config_file()` (`$XDG_CONFIG_HOME/openpaintr/`). Recent files go to a new `state_file()` (`$XDG_STATE_HOME/openpaintr/`, default `~/.local/state/openpaintr/`).
  - **English names (decision 5).** `atalhos.json` → `shortcuts.json`, `paleta.txt` → `palette.txt`, `recentes.json` → `recent.json`, and `docs/atalhos.schema.json` → `docs/shortcuts.schema.json` (update the `include_str!` in `shortcuts.rs`). The suggested file name when exporting the palette comes from a translation key instead of the hard-coded `paleta.txt`.
  - **Migration.** When the new file is missing and the old file (old name, beside the executable) exists, read it and write the new one atomically under the new name and location. Leave the old file untouched. Users migrate once.
  - **Portable mode (decision 4).** When a file named `portable` exists beside the executable, configuration, state, and user translations live in a data directory beside it. `OPENPAINTR_DATA_DIR=<path>` overrides the base directory in any mode, for development and tests. In portable mode, a non-writable executable directory produces a visible error when saving; there is no silent fallback to XDG.
  - Update the README, `.gitignore`, `AGENTS.md`, `docs/plugin-architecture.md`, and the open roadmap items that mention the old names or locations. Completed roadmap items keep their original text, with a short note pointing to the new names.
- **Acceptance.**
  - With a read-only executable directory, saving shortcuts, the palette, and recent files works.
  - Customizations from old files show up after the upgrade.
  - Portable mode and `OPENPAINTR_DATA_DIR` keep everything inside their directory.
  - Tests cover temporary `XDG_*` directories, portable mode, and the migration path.

### 1.5 English names for documents and internal identifiers

- **Problem.** Some documents and identifiers are still in Portuguese, although `AGENTS.md` requires repository documentation in English (decision 5).
- **Change.** In one mechanical pull request with no other changes:
  - Rename `docs/arquitetura-plugins.md` → `docs/plugin-architecture.md`, `docs/extensibilidade.md` → `docs/extensibility.md`, and `docs/referencias.md` → `docs/references.md`, and update every link (README, `AGENTS.md`, roadmap, this plan, and the documents themselves).
  - Rename internal egui IDs such as `"configurar_atalhos"`, `"perfil_atalhos"`, and `"comandos_atalhos"` to English. eframe persistence is not enabled, so no saved window state depends on them.
- **Acceptance.** `git diff --stat` shows only renames and reference updates. No broken relative links (checked by a link checker in CI or a test).

---

## Stage 2 — Performance quick wins

Each item starts and ends with numbers from item 0.3.

### 2.1 Zero-copy `Pixel`

- **Problem.** `Pixel(pub [u8; 4])` has no guaranteed layout. `Document::composite_rect` copies each row of each visible layer into `src_row` (`pixels_to_bytes`), then allocates a second full buffer in `Pixel::from_rgba_bytes`. `pixels_to_color_image` allocates a third one. `Document::read_png` holds three full buffers at once: decoder bytes, decoded pixels, and a throwaway blank layer from `Document::new`.
- **Change.**
  - `#[repr(transparent)]` and `#[derive(bytemuck::Pod, bytemuck::Zeroable)]` on `Pixel`. `bytemuck` 1.25 and `bytemuck_derive` are already in `Cargo.lock` through eframe. Add `bytemuck` as a direct dependency with the `derive` feature.
  - Allocate `composite_rect`'s destination as `Vec<Pixel>` and blend with `bytemuck::cast_slice` / `cast_slice_mut`. Remove `src_row`, `pixels_to_bytes`, `bytes_to_pixels`, and the final `from_rgba_bytes`.
  - Turn `Pixel::to_rgba_bytes` / `from_rgba_bytes` into casts where ownership allows (`bytemuck::allocation::cast_vec`).
  - Add an internal `Document::from_layer_pixels(width, height, pixels)` constructor for `read_png`.
  - Make `pixels_to_color_image` a single pass.
- **Acceptance.** Composites are byte-identical to the current implementation for every PDN fixture (golden test). Open and export times and `VmHWM` are recorded.

### 2.2 Affine resampling

- **Problem.** `Affine::inverse` and `forward` call `sin_cos()` on every call. `geometry::resample` calls `inverse` 16 times per pixel in `Resampling::Antialiased`. `Editor::resample_over` walks the whole canvas even when the transformed content is a small floating paste.
- **Change.**
  - A `PreparedAffine` holding the precomputed inverse matrix, built once per frame and used by `resample`.
  - In `resample_over`, compute the destination bounding box (the forward-transformed bounds of the floating image, the selection, or the layer) clipped to the canvas, and sample only inside it. Outside, copy `base_at` or transparent pixels directly.
  - Parallelism comes later, in item 4.4.
- **Acceptance.** Output is pixel-identical across a test matrix (0°, 15°, 45°, 90°, arbitrary angles, non-uniform scale, the three `Resampling` methods). Transform and Rotate/Zoom frame times are recorded for whole-layer and 64 × 64 sources.

### 2.3 Gradient row buffer

- **Change.** Allocate `acc_rgba` once, before `for y in 0..h` in `gradient_tool.rs`, and clear it with `fill([0; 4])` on each row.
- **Acceptance.** Identical output. This is a small fix with a small gain.

### 2.4 Vector rasterization without `HashMap`

- **Problem.** `rasterize_path_analytical` and `rasterize_path_ssaa` (`vector_tools.rs`) insert every covered pixel into a `HashMap<Point, _>` with SipHash, although `fill_rows` already emits spans in scanline order.
- **Change.** A `CoverageMask { origin, width, height, data }` (`u8` coverage, or `u16` SSAA bits) sized to the path's bounding box clipped to the canvas. `fill_rows` callbacks write into it directly. Consumers (LineCurve and Shape previews and commits, and shape-based selections in `selection.rs`) iterate rows instead of map entries.
- **Acceptance.** Coverage is identical to the `HashMap` version for the 26 presets, Freeform, dashes, and caps in both AA modes. Large-shape drag time is recorded.

### 2.5 Outline-based text rendering

- **Problem.** Text ignores the global antialiasing mode. `FontCatalog::layout_4x` renders glyphs through the egui font atlas at 4× and accumulates 4×4 coverage in a `BTreeMap<Point, [u8; 16]>`. It also copies the whole atlas (`self.fonts.image()`) on every keystroke and drag, and at 240 px the glyphs reach 960 px in an atlas of at most 4096 px.
- **Change (decision 1).**
  - Keep egui for layout only: glyph positions, line breaks, alignment, and the bidirectional and Arabic shaping already applied through `i18n::display`.
  - Get each glyph's outline from the same font data that egui uses for layout, transform it to the glyph position and size, and rasterize it with the shape rasterizer into a `CoverageMask` (item 2.4) using the active antialiasing mode.
  - Underline and strikethrough become rectangles in the same path.
  - Remove the atlas copy and the `BTreeMap`.
- **Risks.** Outline metrics must match the egui layout. Check baseline, advance, and kerning against the current output, at 24, 96, and 240 px, for Latin, Arabic, CJK, and the bundled Noto fonts.
- **Depends on.** Item 2.4.
- **Acceptance.**
  - Text edges change with the antialiasing setting, as for shapes.
  - Glyph placement matches the current implementation within one pixel.
  - Long text at 240 px renders completely.
  - Typing latency recorded at 24, 96, and 240 px.

### 2.6 Incremental thumbnails

- **Problem.** A single `thumbs_dirty: bool` regenerates the thumbnails of every layer, and `make_thumbnail_texture(…, None, …)` calls `composite_thumbnail`, which recomposes the whole document again after every stroke.
- **Change.**
  - Track dirty layers individually (a vector of flags or a bitset), plus a flag for the tab thumbnail. Editor edits report the layers they touched.
  - Build the tab thumbnail by downsampling the composite already computed for display, instead of recompositing.
  - Refresh at the end of a gesture and at most every 250 ms during long sessions.
- **Acceptance.** After a stroke on layer *k*, only the thumbnail of layer *k* and the tab thumbnail are regenerated, with no extra full composite.

### 2.7 Save options dialog off the UI thread

- **Problem.** `SaveOptionsDialog::refresh` encodes the whole PDN or PNG on the UI thread on every slider change, to estimate the size.
- **Change.** Wait about 200 ms after the last slider change, then encode on a worker thread (`std::thread` plus a channel) from a snapshot of the document. Until item 6.1 exists, the snapshot is a clone. Show an "estimating" state, discard stale results using a generation counter, and add translation keys.
- **Acceptance.** On the large workload, the frame time while dragging the slider stays under 16 ms.

---

## Stage 3 — History memory

### 3.1 Rectangle snapshots

- **Problem.**
  - `Edit::Raster` stores `((u32, u32), Pixel, Pixel)` for every changed pixel, which is 16 bytes. Committing a full-canvas gradient at 2560 × 1440 through `commit_layer_preview` takes about 59 MiB, four times the layer itself.
  - `bucket_fill` and `commit_floating` go through `replace_layer_pixels` (`Edit::LayerPixels`) and store the whole layer (14.7 MiB) even for a 20 × 20 change.
- **Change.**
  - Add `Edit::RasterRect { label, layer, rect, other, other_selection }` with the swap semantics of `LayerPixels`: applying it swaps the buffer with that region of the layer, at 4 bytes per pixel of the rectangle.
  - A single helper picks the representation from the changed pixel count and the bounding box:
    - `Raster` (sparse, 16 B per changed pixel) when fewer than a quarter of the box's pixels changed;
    - `RasterRect` (dense, 4 B per box pixel) otherwise;
    - `LayerPixels` only when the box is essentially the whole layer.
  - Use the helper in `commit_layer_preview` (gradient, line and curve, shapes, text), `bucket_fill`, `commit_floating` / paste, transform commits, `fill_selection` / `edit_selection`, and the end of a stroke (`Stroke::into_changes`).
- **Acceptance.**
  - Full-canvas 2560 × 1440 gradient: at most 14.8 MB per entry, down from about 59 MB.
  - 20 × 20 bucket fill: at most 4 KB, down from 14.7 MB.
  - The existing undo, redo, and `jump_to_state` tests pass.
  - A property test applies random sequences of edits, undoes all of them, and recovers the original pixels.

### 3.2 Accurate accounting

- **Problem.** `Edit::approx_bytes` counts `Edit::Selection` as a fixed 256 bytes, even when the selection holds a large mask (`Region::Mask`).
- **Change.** Count the bytes of the mask's allocated tiles. Masks are shared between consecutive entries, so count only each entry's "after" mask, because the "before" mask is the previous entry's "after".
- **Acceptance.** The `undo_bytes` reported for a sequence of large selections stays within 10% of the measured allocation.

### 3.3 Global history budget

- **Problem.** `MAX_HISTORY_BYTES` (512 MiB) applies to each tab, so five open documents can hold 2.5 GiB.
- **Change.**
  - A `HistoryBudget` owned by `App`, shared by all tabs (decision 6). The default is 25% of physical memory, read from `MemTotal` in `/proc/meminfo`, clamped between 512 MiB and 4 GiB. If the value cannot be read, use 1 GiB.
  - Add a history memory setting in **Settings**, saved in `settings.json`, with translation keys in the ten catalogs.
  - After each recorded entry, the app trims the oldest entries, starting with the least recently activated tabs. It never removes a tab's last entry.
  - Tests set an explicit budget, so they don't depend on the machine.
- **Acceptance.** A test with five tabs making large edits stays within the budget. The active tab keeps its recent history. Changing the setting takes effect without restarting.

### 3.4 Clone Stamp without a full-layer copy

- **Problem.** The Clone Stamp handler in `gui/mod.rs` copies the whole layer (`layer_pixels(...).to_vec()`) at the start of every stroke.
- **Change.** `Stroke` already stores each touched pixel's original value in its `StrokeTile` entries. Sample the source from `stroke.get(point)` when the stroke has touched that pixel, and from the current layer otherwise, then remove the snapshot.
- **Acceptance.** Results are identical to the current implementation, including strokes where the source and destination overlap. No full-layer allocation at stroke start.

---

## Stage 4 — Preview pipeline

### 4.1 `Arc` masks (prerequisite for 4.4 and 6.3)

- **Problem.** `Region::Mask(Rc<Mask>)` (`selection.rs`) makes `Selection`, and therefore `Editor`, neither `Send` nor `Sync`. Closures that read the selection cannot run on rayon threads, and the editor cannot hand a snapshot to a worker.
- **Change.** Replace `Rc` with `Arc`. It is the only `Rc` in `src/`.
- **Acceptance.** `Selection: Send + Sync` is asserted at compile time.

### 4.2 Composite cache for sessions and gestures

- **Problem.** During Transform, Gradient, LineCurve, Shape, and Text sessions, `refresh_image` clones the active layer, swaps it into the document (`swap_layer_pixels`), recomposes every visible layer of the whole document, and uploads the whole texture (`texture.set`, about 14.7 MiB per frame at 2560 × 1440).
- **Change.**
  - When a session or gesture starts, compute `below = composite(layers[..active])` once.
  - **Layers above:** when every visible layer above the active one uses Normal blending, precompose them into one premultiplied buffer, which is valid because Normal "over" is associative. Otherwise, blend each one per frame.
  - **Per frame:** copy the region of `below`, blend the active preview, then the layers above.
  - **Invalidate** when layer properties change, layers are added, removed, or moved, a non-active layer is edited, or the document is resized.
  - Expose it in the core, for example as `Document::composite_with_override(rect, layer, pixels, cache)`, so that `refresh_image` stops swapping pixels in and out of the document.
- **Acceptance.** Byte-identical to the full composite for all 14 blend modes, including non-Normal layers above the active one (golden tests). Session frame times on the large workload are recorded.

### 4.3 Session dirty rectangles

- **Change.**
  - `ActiveSession::bounds()` returns the union of the previous and current content bounds. LineCurve, Shape, Text, and Transform have real bounds. Gradients cover the selection or the whole canvas, so they benefit from 4.2 only.
  - `render_layer_preview` receives a region and renders only that region.
  - `refresh_image` composites the region using 4.2 and uploads it with `set_partial`, as strokes already do.
- **Acceptance.** While dragging a shape on the large workload, the bytes uploaded per frame are proportional to the shape's bounding box.

### 4.4 Data parallelism

- **Change.**
  - Add `rayon`. It is a new dependency, so approve its license through `cargo deny`.
  - Parallelize by rows: `composite_rect`, `resample_over`, gradient rendering, global bucket fill and Magic Wand matching, row-separable feather passes, and thumbnail sampling.
  - Keep results deterministic: rows are independent and there are no floating-point reductions across rows.
  - Put it behind a default `parallel` feature, so the CLI benefits too.
- **Acceptance.** Identical output. Speedups recorded on 2, 4, and 8 threads. No added frame stutter.

---

## Stage 5 — Maintainability

### 5.1 Split `src/gui/mod.rs`

The file has about 6,500 lines and `App::canvas` about 1,375. Split it mechanically, in this order:

1. **Tests:** move the `mod tests` block (about 2,000 lines) to `src/gui/tests/`, with `driver.rs` (`TestDriver`) and one file per area (workflows, selection, tools, layout, persistence, performance).
2. **Panels and workspace:**
   - `panels/layers.rs` and `panels/history.rs`
   - `menus.rs` (`menu_bar`, `file_menu`, `main_menus`)
   - `workspace.rs` (`DocumentTab`, tab handling, close flow)
   - `command_bar.rs`
   - `canvas/overlays.rs` (`paint_selection_overlay`, `paint_dashed_line`, `paint_selection_coverage`, guides)
3. **`phase2.rs`:** it is named after a roadmap phase and mixes menus (`edit_menu`, `view_menu`, `image_menu`, `layer_menu`), command execution (`execute_command`), clipboard actions, and geometry dialogs. Split it into `menus.rs`, `commands.rs`, `clipboard_actions.rs`, and `geometry_dialogs.rs`.

- **Acceptance.** The diffs contain only moves and `use` changes, and every test passes. `mod.rs` ends up under 1,500 lines.

### 5.2 `ToolController` trait

- **Problem.** `canvas()` mixes the viewport (zoom, pan, rulers, canvas resizing) with the event handling of every tool. Each new tool edits that function.
- **Change.**
  ```rust
  trait ToolController {
      fn tool(&self) -> Tool;
      fn options_ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext);
      fn on_pointer(&mut self, event: &CanvasPointerEvent, cx: &mut ToolContext) -> ToolOutcome;
      fn on_key(&mut self, event: &egui::Event, cx: &mut ToolContext) -> ToolOutcome {
          ToolOutcome::Ignored
      }
      fn cursor(&self, cx: &ToolContext) -> Option<egui::CursorIcon>;
      fn paint_overlay(&self, painter: &egui::Painter, cx: &ToolContext) {}
  }
  ```
  - `ToolContext` borrows the active tab (`Editor`, `View`, session), colors, global settings, and shortcuts.
  - `canvas()` keeps the viewport and dispatches events to the active tool's controller.
  - `mouse_shortcuts.rs` stays the single place that turns profile bindings into canonical events.
  - **Migration order:** Hand, Zoom, Eyedropper → Rectangle and Ellipse Select, Lasso, Magic Wand → Brush, Pencil, Eraser, Recolor, Clone Stamp → Paint Bucket → session tools (Gradient, Line/Curve, Shapes, Text, Move).
- **Acceptance.** `canvas()` under 300 lines, each tool's UI tests unchanged, and adding a tool needs no change to `canvas()`.

### 5.3 `ToolSettings`

- **Change.** Move the per-tool fields from `App` into a `ToolSettings` struct: `diameter`, `pencil_diameter`, `hardness`, `spacing`, `fill_pattern`, `stroke_blend`, `selection_combine`, `selection_feather`, `color_tolerance`, `alpha_match`, `wand_*`, `bucket_*`, `eyedropper_*`, `gradient`, `line_curve`, `shape`, `text`, and `shape_thickness`. The accessors in `tool_options.rs` become methods of that struct.
- **Benefit.** Prepares "tool defaults and settings" persistence from roadmap phase 4.

### 5.4 Typed core API

- **Change.**
  - A `BlendMode` enum with `pdn_id()`, `from_pdn_id()`, and translation keys, replacing the `u8` and the `BLEND_MODES` / `BLEND_MODE_KEYS` arrays.
  - `LayerProps { visible, opacity }` replaces `(bool, u8)` in `layer_properties` and `set_layer_properties`. Move `LayerMeta` to the core.
  - Remove the `unwrap()` calls in the GUI that follow these lookups.
  - Add region accessors (`rows(rect)`, `row_mut(y)`) and inventory the direct uses of `layer_pixels() -> &[Pixel]`, to prepare item 6.1.
- **Acceptance.** No `u8` blend identifiers outside the codecs. No `unwrap()` on layer lookups in non-test GUI code.

### 5.5 Decouple the domain from i18n

- **Problem.** `editor.rs`, `selection.rs`, `tolerance.rs`, `geometry.rs`, and `vector_tools.rs` call `i18n::text` for labels and errors, which ties the editing engine to the GUI's thread-local locale.
- **Change.** Domain enums expose `label_key() -> &'static str`, and errors use `Diagnostic` keys instead of `io::Error::other(tr(...))`. The GUI translates at the boundary, where `i18n::error_message` already exists.
- **Acceptance.** No `i18n` import in the modules that move to the engine in item 5.6.

### 5.6 Cargo workspace with separate core, engine, CLI, and GUI crates

```
crates/openpaintr-core     # Pixel, Document, BlendMode, codecs, Diagnostic, limits
crates/openpaintr-engine   # Editor and history, Selection/TMask, rasterizers, geometry,
                           # tolerance, feather, bucket_edges, non-UI session math
crates/openpaintr-cli      # argument parsing and command implementations; no egui
crates/openpaintr-gui      # egui application, shortcuts, i18n, clipboard, dialogs
crates/openpaintr          # thin binary: main() dispatches to the CLI or, with the
                           # `gui` feature, to the GUI
# phase 5: crates/rustpdn-plugin-sdk, crates/openpaintr-plugin-host
```

- **Dependency direction:** `core` ← `engine` ← `cli` and `gui` ← `openpaintr`. `cli` and `gui` never depend on each other, and only `gui` depends on egui, eframe, winit, rfd, arboard, and the Wayland crates.
- **CLI crate.** Move the argument parsing, `USAGE`, the commands (`demo`, `demo-png`, `png`, `pdn`), and helpers such as `new_output`, `export_png`, and `demo` from `src/main.rs` into `openpaintr-cli`. It exposes something like `run(args: impl Iterator<Item = OsString>) -> io::Result<()>` so commands can be tested without spawning a process. Keep today's behavior: the same usage text, outputs created with `File::create_new` (existing files are never overwritten), and the same errors.
- **Binary crate.** `main()` only decides between the GUI (no arguments or `gui`, when the `gui` feature is on) and `openpaintr_cli::run`. The executable stays a single `openpaintr`, as described in the README, and `--no-default-features` still produces a CLI-only build.
- `transform.rs` uses egui's `Vec2` for bounds and gestures. Split it into an engine-side `TransformState` (affine, bounds as its own vector type) and GUI-side handle interaction.
- Make the root `Cargo.toml` a workspace with `default-members`, so that `cargo run --locked` and the README commands keep working. Put shared lints in `[workspace.lints]`, including `unsafe_code` (item 0.4).
- Move pure-domain tests to the engine crate, CLI tests to `crates/openpaintr-cli/tests/`, and cross-crate integration tests to `tests/`.
- **Acceptance.**
  - `cargo tree -p openpaintr-engine` and `cargo tree -p openpaintr-cli` contain no egui, eframe, winit, or rfd.
  - The CLI produces the same output files, usage text, and errors as before (tests compare against the current commands).
  - The CLI-only build (`--no-default-features`) works, and every test passes.

### 5.7 CLI on the engine

- **Problem.** The CLI only converts files (`demo`, `demo-png`, `png`, `pdn`). The editing operations live in the GUI, so the command line cannot use them, and there is no way to process files in batches.
- **Change.** After item 5.6, add commands built on `openpaintr-engine` (decision 9):
  - `info`: dimensions, DPI, and each layer's name, visibility, opacity, and blend mode, as text or as JSON with `--json`.
  - `convert <input> <output>`: chooses the codecs by extension and covers the current `png` and `pdn` commands, which stay as aliases.
  - `flatten`, `resize` (resampling method), `canvas-size` (anchor), `rotate`, `flip`, and `export-layer <index>`.
  - Batch mode: several inputs with `--out-dir`, keeping the no-overwrite rule.
- **Rules.**
  - Parse arguments with `clap` derive from the first new command. Each command's arguments are one struct, so the future MCP server (roadmap) can derive its tool schemas from the same structs.
  - Output, usage, and help stay in English. `AGENTS.md` records this exception to the translation rule.
  - Each command calls the same engine function as the matching GUI action. No algorithm is duplicated in the CLI.
  - Commands apply the decode limits from item 1.1 and write atomically.
  - Errors print the `Diagnostic` message and return a non-zero exit code.
- **Acceptance.**
  - For each command, an integration test checks that the result is byte-identical to the same engine operation used by the GUI.
  - The README documents the commands.
  - If approved, this becomes a new roadmap item, since no roadmap item covers it today. The MCP server in the roadmap's **Future** section builds on it.

---

## Stage 6 — Structural work aligned with phases 4 and 5

### 6.1 Copy-on-write tiled layers

- **Change.** Start with a design note in `docs/` covering:
  - tiles of 64 × 64 (decision 8), confirmed by a benchmark against 128 and 256, and storage as `Vec<Arc<Tile>>`, with `Arc::make_mut` on write;
  - history entries holding tile `Arc`s, so a snapshot is a cheap clone of the tile list, and Clone Stamp and save snapshots become almost free;
  - plugin tiles (256 × 256 with halos) assembled from 64 × 64 layer tiles, since plugins receive copies.
  
  Migrate through the region accessors from item 5.4. Then replace `RasterRect`, `LayerPixels`, and `Document` edits with tile snapshots, and account for memory as the bytes of unique tiles.
- **Acceptance.** Undo memory equals the changed tiles. Benchmarks are no worse than after Stage 4.

### 6.2 Tiled display textures

- **Problem.** Each document is a single texture, which limits the canvas to `max_canvas_side` (at most 8192 px) and forces large uploads.
- **Change.** A grid of 512 × 512 textures in which dirty rectangles update only the affected tiles. Raise the canvas limit to the core's `MAX_PIXELS`. Evaluate downsampled levels for zoomed-out views of very large documents.
- **Acceptance.** A 16384 × 4096 document opens, pans, and zooms smoothly. Uploads are proportional to the dirty region.

### 6.3 Asynchronous I/O

- **Problem.** Opening, saving (PDN files up to 256 MiB), and exporting run inside the egui `update`, so the window freezes.
- **Change.**
  - A worker thread for decoding, encoding and the atomic write, and export, with progress and cancellation.
  - The worker receives a document snapshot (cheap after item 6.1, a clone before it), using `rfd::AsyncFileDialog` or portal calls off the UI thread.
  - While saving, conflicting actions on that tab are disabled and a second `Ctrl+S` waits for the first.
  - Requires `Document: Send` and item 4.1.
- **Acceptance.** The UI keeps responding (frames under 50 ms) while saving the large workload. The existing atomic save, failure, and cancel tests pass.

### 6.4 `wgpu` renderer with `glow` fallback

This maps to the roadmap's phase 4 **Performance** item ("portable GPU backend"). Decision 7 makes `wgpu` the target renderer.

- **Why.** With the UI on `wgpu`, WGSL plugin previews render into egui textures on the same device, without copying results back to the CPU.
- **Current state.** The application uses only egui's texture API, with no direct OpenGL calls or paint callbacks, so most of the code does not depend on the renderer.
- **Change.**
  - Enable both eframe features, `wgpu` and `glow`, and choose the renderer at startup through `NativeOptions::renderer`:
    1. `OPENPAINTR_RENDERER=wgpu|glow` when set;
    2. otherwise `glow` when WSL is detected (same detection as `linux_backend_choice`);
    3. otherwise `wgpu`, after checking that a usable adapter exists, and `glow` if none does.
  - Probe the adapter before opening the window. winit allows only one event loop per process, so switching renderers after the window opens would require restarting the process.
  - Put the plugin preview path behind one abstraction: a shared-device texture with `wgpu`, and a CPU upload with `glow`. With `glow`, plugins use a separate `wgpu` device when available, or the CPU Wasm path, which every plugin must provide anyway.
  - CI builds both renderers. Headless tests do not depend on the renderer.
- **Validation (one to two days) before switching the default.** Native Ubuntu 26.04 on Wayland; Intel or AMD with Mesa; NVIDIA with the proprietary driver; a virtual machine without a GPU (Vulkan in software); and WSLg running with `glow`. Check startup time, memory, frame times, correctness, and the transparent borderless window with rounded corners (`with_transparent(true)`) on each renderer.
- **Output.** A decision record. After a successful validation, `wgpu` becomes the default and `glow` stays as the fallback, possibly replaced later by `wgpu`'s own OpenGL backend.

### 6.5 Plugin host hardening checklist (phase 5)

- **Package:** maximum entry count, per-entry and total uncompressed size, compression-ratio cap, and rejection of absolute paths, `..`, and symlinks. Validate the manifest against a schema.
- **Wasmtime:** `StoreLimits` (memory, tables, instances), epoch interruption for cancellation, optional fuel, and no filesystem or network access.
- **WGSL:** `naga` validation, workgroup and buffer size limits, dispatch timeouts, and recovery from device loss by falling back to the CPU path.
- **Trust:** show the requested capabilities when a plugin is installed, and decide on provenance or signatures.
- **Commit:** blend results with the selection's own `TMask` coverage (decision 2).
- **Tests:** malicious packages (ZIP bomb, path traversal, infinite loop, memory exhaustion).

---

## Sequencing

1. **Stage 0** in full.
2. **Stage 1** (items 1.1 to 1.5).
3. **Stage 2**, one item per pull request, each with numbers. Item 2.5 comes after 2.4. In parallel, item 5.1 step 1 (moving the tests), which is mechanical and reduces later conflicts.
4. **Stage 3.**
5. **Stage 4:** 4.1, then 4.2 and 4.3, then 4.4.
6. **Stage 5:** 5.1 steps 2 and 3, 5.2 to 5.5, then 5.6 (including the CLI crate), then 5.7.
7. **Stage 6:** 6.4 can start at any point in phase 4. 6.1 to 6.3 follow Stage 5. 6.5 comes with phase 5.

### Mapping to the roadmap

| Roadmap item | Plan items |
| --- | --- |
| Phase 4 — Performance | 0.3, Stage 2, Stage 4, 6.2, 6.4 |
| Phase 4 — Desktop and support | 1.4, 5.3, 6.3 |
| Phase 5 — Shared infrastructure and plugin engine (prerequisites) | 5.5, 5.6, 6.1, 6.5 |
| Not in the roadmap yet (new item to propose) | 5.7 |

## Checklist

- [x] 0.1 Continuous integration
- [x] 0.2 Translation catalog check
- [x] 0.3 Per-tool performance baseline
- [x] 0.4 `unsafe` lint for the whole binary
- [x] 1.1 Decode and allocation limits
- [x] 1.2 Translation loading
- [x] 1.3 Bounded reads
- [x] 1.4 XDG paths, portable mode, and migration
- [x] 1.5 English names for documents and internal identifiers
- [x] 2.1 Zero-copy `Pixel`
- [x] 2.2 Affine resampling
- [ ] 2.3 Gradient row buffer
- [ ] 2.4 Vector rasterization without `HashMap`
- [ ] 2.5 Outline-based text rendering
- [ ] 2.6 Incremental thumbnails
- [ ] 2.7 Save options dialog off the UI thread
- [ ] 3.1 Rectangle snapshots
- [ ] 3.2 Accurate accounting
- [ ] 3.3 Global history budget
- [ ] 3.4 Clone Stamp without a full-layer copy
- [ ] 4.1 `Arc` masks
- [ ] 4.2 Composite cache for sessions and gestures
- [ ] 4.3 Session dirty rectangles
- [ ] 4.4 Data parallelism
- [ ] 5.1 Split `src/gui/mod.rs`
- [ ] 5.2 `ToolController` trait
- [ ] 5.3 `ToolSettings`
- [ ] 5.4 Typed core API
- [ ] 5.5 Decouple the domain from i18n
- [ ] 5.6 Cargo workspace with separate core, engine, CLI, and GUI crates
- [ ] 5.7 CLI on the engine
- [ ] 6.1 Copy-on-write tiled layers
- [ ] 6.2 Tiled display textures
- [ ] 6.3 Asynchronous I/O
- [ ] 6.4 `wgpu` renderer with `glow` fallback
- [ ] 6.5 Plugin host hardening checklist
