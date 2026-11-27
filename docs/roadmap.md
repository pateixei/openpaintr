# OpenPaintR Roadmap

**Status** — Approved on November 25, 2026. Last updated on November 27, 2026.

The first GitHub publication uses **OpenPaintR** as the project name and `openpaintr` as the executable. Phases 1 and 2 are complete. Phase 3 tools are implemented. Color profiles, broader format support, desktop integration, and performance are next in phase 4, followed by the plugin infrastructure, adjustments, and effects in phase 5. [GitHub repository](https://github.com/pateixei/openpaintr)

The roadmap starts with a Linux baseline and a layered PDN editing and saving workflow. Human usability (SUS) and broader layout redesign decisions run after phase 5 once adjustments and `.rpdn-plugin` effects are integrated into the workspace. Later phases cover selection, tools, transforms, color profiles, broader format support, desktop integration, performance, and the 13 adjustments and 48 effects shown in Paint.NET 5.2. Milestones specify dependencies and acceptance criteria.

OpenPaintR creates images, opens PNG and PDN, exports PNG, and supports brush and eraser strokes, zoom, layers, visibility, opacity, and undo and redo. PDN import preserves layer properties, including blend modes. Phase 1 records the Paint.NET 5.2 round-trip and compositing results. [Current capabilities](../README.md)

**Interoperability.** Store editable documents in interchange formats. Validate PDN exclusively against **Paint.NET 5.2** (relying on Paint.NET's own backward compatibility rather than testing older installations) and PSD with Adobe Photoshop. Record preserved features in the acceptance criteria.

**Antialiasing.** Lines, curves, strokes, fills, and selections support **Analytical / Distance-Field AA** (default, 256-level continuous coverage) and **4×4 SSAA**, selectable in global application settings (**Settings → Antialiasing**) for both previews and pixels committed to the layer.

**Plugins and effects.** Implement all **Effects** menu entries as portable, cross-platform `.rpdn-plugin` packages with WebAssembly, optional WGSL shaders, and a declarative UI. Use the public API available to third-party authors through the minimal `rustpdn-plugin-sdk`. [Plugin architecture](plugin-architecture.md) · [Extensibility guide](extensibility.md)

## Phase 1 — Documents, layers, and persistence

- [x] **Files and workspace.** Provide New, Open, Open Recent and Clear Recent Files, Save, Save As, save options with a preview, Export, Close, Close All, and Exit. Prompt for each document with unsaved changes. Open multiple documents in reorderable tabs with thumbnails, provide next and previous navigation and a document list, and keep history, selection, zoom, and position independent per document.
- [x] **Layers.** Add, delete, duplicate, merge down, toggle visibility, and import layers from a file. Navigate to the first, previous, next, or last layer. Move layers to the top or bottom, move them up or down, or reorder by dragging. Show thumbnails and expose name, visibility, opacity, and blend mode.
- [x] **Compositing.** Validate Normal, Multiply, Additive, Color Burn, Color Dodge, Reflect, Glow, Overlay, Difference, Negation, Lighten, Darken, Screen, and Xor against Paint.NET 5.2 using partial alpha, hidden layers, and overlapping layers, and record any rounding tolerances. [Blend modes](https://paint.net/doc/latest/BlendModes.html)
- [x] **History.** Undo and redo document, layer, and selection operations. Navigate to any state from the panel, group each gesture into one entry, and bound memory use. Evaluate temporary diff storage for large documents.
- [x] **Editable persistence.** Use atomic replacement for Save and Save As. Reopening a PDN must preserve dimensions, RGBA pixels, layer order, names, visibility, opacity, and blend modes, and allow editing to continue. Track the last saved revision independently of undo history. Preserve the open document and last valid save if the operation fails or is canceled. Decide which session state to persist, including zoom, selection, and history.
- [x] **Compatibility and export.** Test PDN import (including existing legacy fixtures) and round-trip compatibility against Paint.NET 5.2, and verify PNG compositing and alpha separately. Preserve the original when using Save As. Reopen generated files in OpenPaintR and in Paint.NET 5.2.
- [x] **Workspace interface.** Define access to the operations delivered in this phase with original icons, Color Ribbon, menus, contextual toolbar, and status bar. Validate the five phase 1 workflows automatically via headless UI tests, and defer broader panel placement and layout redesign evaluation until the post-phase 5 usability study. Expose selection, adjustment, and effect commands as they become available.
- [x] **Borderless window.** Integrate the light and dark theme buttons and the minimize, maximize or restore, and close controls in the menu bar. Keep window dragging, edge and corner resizing, fullscreen, and unsaved-document prompts available. Owner sign-off received on November 26, 2026.
- [x] **Linux baseline.** Pin the `openpaintr` release build (`target/release/openpaintr`) and verify the five phase 1 tasks automatically before human testing. Record CLI and interactive Linux performance baselines below.

**Acceptance criteria** — Open a PDN, reorder, duplicate, and merge layers, then undo the changes. Test Save, Close, and Reopen as a separate workflow and verify pixels and layer properties. Compare a PNG export with Paint.NET reference images. Record the Linux baseline and observed difficulties to guide the redesign scope and timing. [v3 menus and actions](https://github.com/nakijun/OpenPDN-1/tree/cca476b0df2a2f70996e6b9486ec45327631568c/src/Menus) · [v5 documentation](https://paint.net/doc/latest/index.html)

### First phase 1 delivery

The editor writes layered PDN files with `PDNE_BLENDOP` (`UserBlendOps+*BlendOp` + `blendMode`), atomic replacement for Save and Save As, independent `saved_revision` tracking, and original-file preservation. Paint.NET 5.2 opened and re-saved `pdn52_roundtrip_layers.pdn` (`96 × 64`, 4 layers, ~`143.99` DPI, `Multiply`, `Additive`, and hidden layer), preserving all layer names, visibility, opacity, blend modes, DPI, and RGBA pixels (`tests/pdn_compat.rs`), with a `1` LSB composite difference against Paint.NET 5.2's 32-bit PNG export. Automated black-box UI validation ([`scripts/validate_pdn52.ps1`](../scripts/validate_pdn52.ps1)) also exported PNG references from **Paint.NET 5.2** for:
- `FlattenBlendTest.pdn` (14 blend layers, `800 × 600`): **0 differing pixels (`0 / 480,000`, `max_lsb_diff = 0`)**.
- `pdn52_blend_mosaic.pdn` (all 14 blend modes across opaque, semitransparent, and 3-layer partial-opacity `149` stacks + hidden `Difference` trap, `280 × 120`): **`alpha_max = 1` across all 33,600 pixels** (hidden layer never leaked), **`<= 2` LSB across all 14 modes in 2-layer compositing** (`y < 60`), and measured 3-layer cascading FP32-vs-integer rounding tolerances (up to `2–5` LSB for continuous modes, `15` premul LSB for `Glow` near denominator `1`, and `98` premul LSB for bitwise `Xor` carry-flips on `±1` intermediate bytes).

The pinned release executable at `target/release/openpaintr` (`SHA-256` `ebe428359f6b036bb12ccbc0041ede2e5788daaff0102dda522a7ca71b438068`, `rustc 1.99.1`, `Linux 6.18.33.2-microsoft-standard-WSL2`, `13th Gen Intel(R) Core(TM) i7-13700K`) was benchmarked on release CLI conversions (`1` warm-up + `10` runs: `Untitled.pdn` `13.1 ms` median / `14.8 ms` max, `Untitled3.pdn` `13.8 ms` / `14.7 ms`, `oldPDN3510.pdn` `16.5 ms` / `20.3 ms`, `FlattenBlendTest.pdn` `35.7 ms` / `38.4 ms`) and headless GUI interactive workloads (`phase1_interactive_performance_baseline`, `2` warm-ups + `10` timed runs):

| Workload | Open to editable canvas | 4×4 SSAA stroke + frame | Zoom / scroll frame | PNG export | Peak `VmHWM` |
| --- | ---: | ---: | ---: | ---: | ---: |
| Typical (`800 × 600`, 4 layers) | `11.85 ms` med / `12.80 ms` max | `0.14 ms` med / `0.16 ms` max | `0.10 ms` med / `0.11 ms` max | `4.64 ms` med / `4.86 ms` max | `40,348 kB` |
| Large (`2560 × 1440`, 8 layers) | `134.22 ms` med / `137.25 ms` max | `0.18 ms` med / `0.27 ms` max | `0.12 ms` med / `0.16 ms` max | `58.38 ms` med / `62.22 ms` max | `281,608 kB` |

The five phase 1 workflows below, plus `Ctrl+double-click` bottom-right corner resizing and atomic persistence failure/cancel/Save-As checks, are verified automatically by `editable_persistence_atomic_save_as_preserves_original_and_handles_failure_or_cancel`, `phase1_five_workflows_end_to_end`, and `ctrl_double_click_bottom_right_enters_canvas_resize_and_commits_or_cancels` (`src/gui/mod.rs`).

### Automated workflows and interactive performance

| Workflow | Expected result |
| --- | --- |
| Open and understand a project | Open a PDN, locate the requested layer, and identify the hidden layer. |
| Edit a layer | Change the correct layer's opacity, draw a stroke, and undo the stroke while retaining the opacity change. |
| Navigate and edit a detail | Reach a region with zoom and scrolling, edit it, and return to the overview. |
| Export to PNG | Write a file to the requested destination and verify visible-layer compositing and transparency. |
| Resume an editable project | Save, close, and reopen the project, verify the layers, and continue editing. |

- [x] **Interactive performance.** Measure typical (`800 × 600 × 4`) and large (`2560 × 1440 × 8`) projects on Linux (`target/release/openpaintr`). Record time to an editable canvas, stroke-to-display latency, zoom and scroll frame times, export duration, and peak memory (`VmHWM`). Use the baseline to set performance targets and locate bottlenecks.

## Phase 2 — Selection, clipboard, navigation, and geometry

**Status** — Complete. Owner sign-off received on November 26, 2026.

- [x] **Selection.** Rectangle, ellipse, lasso, and Magic Wand. Support tolerance, layer or composite sampling, contiguous and global selection, Replace, Add, Subtract, Intersect, and XOR. Move or transform the mask separately from the pixels. Move, resize, and rotate selected pixels with handles, constrain proportions, and choose a resampler, including antialiased nearest neighbor.
- [x] **Selection feather.** Share the control across Rectangle Select, Ellipse Select, Lasso, and Magic Wand. At 0%, preserve the original SSAA selection. At 100%, fade coverage linearly to zero outside the selection over half the shorter side of its bounding box. Apply feather to each new gesture before combining it with an existing selection. Show partial coverage and preserve it when filling, erasing, copying, or moving the mask.
- [x] **Editing.** Cut, copy, copy flattened, paste, paste into a new layer or image, copy the selection mask, and paste masks with the applicable combination modes. Erase, fill, invert, select all, and deselect. Expose shortcuts and resolve the `Ctrl+Shift+Z` conflict between Redo and Rotate/Zoom.
- [x] **Viewing and navigation.** Zoom in and out, fit the window or selection, return to actual size, and support continuous zoom and a Zoom tool. Pan with the Hand tool or temporarily with Space. Provide a pixel grid and rulers, using document resolution to switch between pixels, inches, and centimeters.
- [x] **Image operations.** Crop to selection, resize with resampling and optional aspect lock, change canvas size with an anchor, flip horizontally or vertically, rotate 90° clockwise or counterclockwise or 180°, and flatten. Color Profile is planned for phase 4.
- [x] **Layer geometry.** Flip horizontally or vertically, rotate 180°, and provide Rotate/Zoom with preview, translation, scale, and rotation. Validate interpolation, edges, transparency, and the distinction between layer and document operations.

**Validation** — In Linux/X11, a UI test selected an irregular region with the lasso, filled it from the keyboard, copied the composite, and opened it in another tab. A separate X11 client received the clipboard PNG. The flow also verified moving the mask without touching pixels, previewing and applying a pixel transform, undo, rulers, and zoom to selection. Automated tests cover combination modes, Magic Wand, DPI, rotation, anchors, aspect ratio, transparency during resampling and cropping, and independent tab history. Native Wayland clipboard integration remains part of phase 4 platform validation.

**Acceptance criteria** — Select an irregular region, copy the composite into another image, move the mask without changing pixels, transform the pixels separately, and undo each step. Test selection, DPI, anchor, and dimensions at boundaries. [Paint.NET Image menu](https://paint.net/doc/latest/ImageMenu.html) · [v5 tools](https://paint.net/doc/latest/ToolsWindow.html) · [v3 source code](https://github.com/nakijun/OpenPDN-1/tree/cca476b0df2a2f70996e6b9486ec45327631568c/src)

## Phase 3 — Tools and colors

- [x] **Strokes.** Brush, Pencil (default `1 px`), and Eraser with widths from `1..=96 px`. Where applicable, support hardness (`0–100%`), spacing (`1–200%`), fill patterns (solid, stripes, diagonals, checkerboard, dots, and grid), always-on 4×4 SSAA, and Blend or Overwrite. Use primary and secondary mouse buttons for the two colors and offer temporary brush sampling with `Ctrl+Click` in Paint.NET or `Alt+Click` in Adobe.
- [x] **Contiguous Paint Bucket.** Fill edge-connected pixels on the active layer with the same RGBA value as the clicked pixel. Respect selection SSAA coverage and record one history entry. Provide a tool icon and profile shortcuts. The next item covers tolerance and global fill.
- [x] **Fill and sampling.** Paint Bucket supports contiguous or global scope (`Shift+Click` for global in Paint.NET), fill patterns, blend modes, and layer or composite sampling. Gradient provides linear, reflected, diamond, radial, conical, and clockwise and counterclockwise spiral modes, with repetition, color or transparency channels, dithering, 4×4 SSAA, and draggable handles. Eyedropper samples the layer or composite (`Ctrl` or `Alt`) and can remain active or restore the previous tool. Clone Stamp sets the source with `Ctrl+Click` in Paint.NET or `Alt+Click` in Adobe, displays a source marker, and snapshots the source at the start of each stroke. Recolor preserves shading.
- [x] **Color tolerance.** Magic Wand, Paint Bucket, and Recolor share a `0–100%` toolbar control and premultiplied or straight alpha matching. At 0%, require an exact match under the selected alpha mode. Magic Wand and Paint Bucket compare with the clicked sample. Recolor compares pixels under the stroke with the sampled target color (`Ctrl` or `Alt`) or the secondary color. Preserve clean separation across antialiased strokes. [Paint.NET toolbar](https://paint.net/doc/latest/Toolbar.html) · [Paint.NET Recolor](https://paint.net/doc/latest/RecolorTool.html)
- [x] **Geometric drawing.** Line/Curve offers `CubicBezier` and `Spline` with four draggable control points, width, dashes, and end caps including arrows. Shapes offer outline, fill, or both, plus patterns and blending. The 26 presets span basic shapes (including rounded rectangles with an adjustable radius), polygons and stars, arrows, symbols, and callouts. Paint.NET v3-style Freeform is also supported. Shapes use 4×4 SSAA with preview and resize and rotate handles. Commit with `Enter` or click, or cancel with `Esc`.
- [x] **Antialiasing in existing tools.** Brush, Eraser, selections, and elliptical fill or erase always use **4×4 SSAA** and preserve partial alpha in history and PDN saves.
- [x] **Always-on SSAA.** The editor applies 4×4 SSAA to outlines and fills during previews and when committing pixels. SSAA is the only antialiasing mode.
- [x] **Configurable antialiasing.** Support both **Analytical / Distance-Field AA** (default, 256-level continuous coverage) and **4×4 SSAA** across brush/eraser/recolor/clone strokes, lines/curves, shapes, selections, and feathering, selectable in **Settings → Antialiasing** and persisted in global application settings (`settings.json`). Preserve partial alpha in history, PDN saves, and antialiased Paint Bucket edge recovery across both modes. Owner sign-off received on November 26, 2026.
- [x] **Text.** Click to position and drag the anchor. Type directly on the canvas or use the contextual toolbar field. Press `Shift+Enter` for a line break, `Enter` to commit, and `Esc` to cancel. Choose a detected system or bundled font, size (`6..=240 px`), bold, italic, underline, strikethrough, and left, center, or right alignment. Rasterize with 4×4 SSAA and record one history entry. *(Text now rasterizes glyph outlines with the global antialiasing mode, in a Sharp or Smooth style chosen in Settings; architecture plan item 2.5.)*
- [x] **Colors.** Primary and secondary colors, swap (`X`), reset (`D`), HSVA wheel, brightness and opacity controls, and a compact or expanded panel with synchronized RGB, HSV, hex, and alpha fields. Provide 32 editable swatches with `+` to add or right-click to remove, Default, Pastel, and Grayscale presets, Paint.NET-compatible `.txt` import and export (`AARRGGBB`), and automatic saving to `paleta.txt`. *(The palette is now `palette.txt` in the configuration directory.)*
- [ ] **Color Ribbon swatches and opacity.** Keep the custom, black, and white swatches visible beside an opacity slider and the hue spectrum. Preserve each swatch's opacity when switching. Use the selected primary swatch's RGBA in painting and keep the alpha controls in sync with the Colors panel. Support configurable primary and secondary swatch gestures in both shortcut profiles.

**Acceptance criteria** — Each tool follows its active-layer and selection rules, maintains a preview during the gesture, and produces one reversible edit on confirmation. Test sampling with hidden layers and strokes containing semitransparent pixels. [Tools and settings](https://paint.net/doc/latest/ToolsWindow.html) · [Paint.NET 5.2 changelog](https://paint.net/roadmap.html)

### Configurable keyboard and mouse shortcuts

- [x] **Command registry.** Centralize shortcuts and assign a stable identifier to each tool and command. Register configurable bindings in the JSON schema whenever a tool is added.
- [x] **Profiles.** Select, create, and edit profiles in **Settings → Keyboard and Mouse Shortcuts**. The default **Paint.NET** profile follows Paint.NET, and the **Adobe** profile follows Photoshop for the equivalent actions implemented here. Show binding conflicts. [Paint.NET](https://paint.net/doc/latest/KeyboardMouseCommands.html) · [Adobe Photoshop](https://helpx.adobe.com/content/dam/help/en/photoshop/using/default-keyboard-shortcuts/photoshop-keyboard-shortcuts.pdf)
- [x] **Local persistence.** Store profiles, bindings, and the active profile in `atalhos.json` next to the executable. Use the [versioned JSON schema](shortcuts.schema.json), preserve custom bindings when tools are added, and report read or write failures in Settings. *(Now `shortcuts.json` in the configuration directory.)*

- [x] **Mouse gestures.** Share the active profile and `atalhos.json` between keyboard and mouse. The **Mouse** tab remaps strokes, selection gestures, the color wheel, navigation, transforms, and layer commands. The schema distinguishes gesture identifiers from keyboard commands. *(The file is now `shortcuts.json`.)*
- [x] **Stroke width via the wheel.** In the Paint.NET and Adobe profiles, `Ctrl+Shift+Wheel` changes width in 1 px steps over the canvas. Offer the configurable gesture for all tools with pixel-width strokes and update the preview of a line, curve, or shape being edited.
- [ ] **Stroke-size cursor preview.** While the wheel gesture's key combination is held over the canvas, show a zoom-scaled outline of the current stroke width, with a dot for very small sizes. Apply wheel changes immediately and restore the normal cursor when the keys are released.
- [x] **Reference gesture parity.** The equivalent Paint.NET and Photoshop gestures are configurable in `atalhos.json` and documented in `atalhos.schema.json`. They include marquee zoom (`zoom.marquee` in Paint.NET), scrubby zoom (`zoom.scrubby` in Adobe), repositioning a selection while drawing (`selection.relocate`, right button in Paint.NET or `Space+Left` in Adobe), Eyedropper, temporary Brush sampling, global Paint Bucket, Clone Stamp source, Recolor target, and handles for gradients, curves, shapes, and text. [Paint.NET](https://paint.net/doc/latest/KeyboardMouseCommands.html) · [Adobe Photoshop](https://helpx.adobe.com/photoshop/desktop/get-started/learn-the-basics/zoom-images.html) *(These files are now `shortcuts.json` and `shortcuts.schema.json`.)*

**Shortcut acceptance criteria** — Switch between Paint.NET and Adobe, create a custom profile, change a binding, restart, and verify that the active profile is restored. Add a new action to the schema and verify that existing custom bindings still work.

Switching to Adobe, creating a custom profile, and restoring its binding after a restart were verified in the Linux/X11 interface.

### Interface localization

- [x] **Localized interface before phase 4.** Provide English, French, Spanish, German, Brazilian Portuguese, Italian, Russian, Arabic, Simplified Chinese, and Japanese. Keep one JSON catalog per locale under `translations/` and discover additional catalogs at startup. Allow changing the interface language without restarting, persist the choice, and use English as a fallback. Keep open documents, history, and shortcut bindings usable across a language switch. Verify catalog coverage, placeholders, glyphs, right-to-left Arabic labels, language selection, and file workflows. Owner sign-off received on November 26, 2026.

## Phase 4 — Color, formats, desktop integration, and performance

- [ ] **Color and metadata.** Add Color Profile to the Image menu with assignment and conversion for sRGB, Adobe RGB, Display P3, ProPhoto RGB, and imported ICC/ICM profiles. Preserve profiles, print resolution, and metadata in formats that support them. Cover PNG text, EXIF orientation, and CICP. Evaluate HDR-to-SDR conversion on import for relevant codecs.
- [ ] **Common formats.** Complete PNG options and the read/write matrix for JPEG, BMP/DIB/RLE, GIF, TGA, and TIFF. Test palettes, transparency, quality, compression, and metadata where supported. Define handling for animated formats, including GIF and APNG.
- [ ] **Additional formats.** DDS, WebP, AVIF, HEIC/HEIF, JPEG XL, and JPEG XR. Document read and write support, variants, available Linux dependencies, and round-trip criteria for each codec. [v5 file formats](https://paint.net/doc/latest/index.html)
- [ ] **Desktop and support.** Add printing and scanner or camera acquisition using Linux integrations, file associations and drag-and-drop, tool defaults and settings, themes, help, tutorials, About, and support and feedback links. Use the Linux distribution channel for updates.
- [ ] **Performance.** Extend phase 1 interactive benchmarks to compositing, clipboard operations, and large layer stacks (and later to effects in phase 5). Use observed bottlenecks to prioritize region updates, caching, and parallelism. Assess FP32 compositing, SIMD, and a portable GPU backend using precision comparisons and benchmarks. [Rendering, color, and extension changes in 5.2](https://paint.net/roadmap.html)

  **Per-tool baseline (November 27, 2026).** Development moved from WSL2 to native Ubuntu 26.04, so these numbers are **not comparable with the phase 1 table**. Compare later measurements only with results from this environment.

  Environment: `Linux ptlinux 7.0.0-44-generic #44-Ubuntu SMP PREEMPT_DYNAMIC Fri Nov  6 02:38:04 UTC 2026 x86_64 GNU/Linux`; `13th Gen Intel(R) Core(TM) i7-13700K`, `powersave` governor; `rustc 1.100.0 (0ef1bc386 2026-11-09)`; commit `f03fc24`; desktop session with other applications running.

  Reproduce with `cargo test --release --locked --bin openpaintr perf_baseline -- --ignored --nocapture`, which writes `target/perf/baseline.jsonl`. Keep the filter: other ignored tests rewrite versioned fixtures. Each case runs in its own process with 2 warm-ups and 10 timed runs, on the typical (`800 × 600`, 4 layers) and large (`2560 × 1440`, 8 layers) documents, with Analytical antialiasing. A frame is one headless UI update (UI, preview, compositing, and texture conversion), without tessellation or GPU upload.

  Phase 1 workloads, measured as in phase 1 (median and maximum of per-run means). The stroke now uses the default Analytical antialiasing.

  | Workload | Open to editable canvas | Stroke + frame | Zoom / scroll frame | PNG export | Peak `VmHWM` | Undo bytes per stroke |
  | --- | ---: | ---: | ---: | ---: | ---: | ---: |
  | Typical (`800 × 600`, 4 layers) | `10.17 ms` med / `11.74 ms` max | `0.15 ms` med / `0.31 ms` max | `0.11 ms` med / `0.19 ms` max | `4.48 ms` med / `5.28 ms` max | `38,676 kB` | `4,546` |
  | Large (`2560 × 1440`, 8 layers) | `124.35 ms` med / `125.39 ms` max | `0.18 ms` med / `0.19 ms` max | `0.13 ms` med / `0.13 ms` max | `56.41 ms` med / `57.57 ms` max | `250,408 kB` | `4,546` |

  Per-tool workloads (median and maximum over all timed frames; T = typical, L = large):

  | Workload | Variant | Phase | Samples | T frame ms (med / max) | L frame ms (med / max) | Peak `VmHWM` kB (T / L) | Undo bytes per entry (T / L) |
  | --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
  | `transform_layer` | nearest | preview | 100 | 6.19 / 7.15 | 72.26 / 76.33 | 43,976 / 275,916 | — |
  | `transform_layer` | nearest | commit | 10 | 7.96 / 9.00 | 111.20 / 112.16 | 43,976 / 275,916 | 1,920,128 / 14,745,728 |
  | `transform_floating` | bilinear-64x64 | preview | 100 | 10.08 / 11.08 | 100.20 / 113.15 | 40,632 / 250,540 | — |
  | `transform_floating` | bilinear-64x64 | commit | 10 | 13.10 / 13.45 | 141.51 / 144.57 | 40,632 / 250,540 | 1,920,128 / 14,745,728 |
  | `rotate_zoom` | nearest | preview | 100 | 6.35 / 7.44 | 72.64 / 77.85 | 45,588 / 275,992 | — |
  | `rotate_zoom` | nearest | commit | 10 | 8.31 / 8.98 | 115.22 / 118.73 | 45,588 / 275,992 | 1,920,128 / 14,745,728 |
  | `gradient` | linear | preview | 100 | 24.55 / 26.16 | 211.23 / 227.54 | 47,820 / 250,228 | — |
  | `gradient` | linear | commit | 10 | 28.09 / 28.72 | 279.31 / 289.76 | 47,820 / 250,228 | 7,680,064 / 58,982,464 |
  | `gradient` | conical | preview | 100 | 32.39 / 45.69 | 272.25 / 298.53 | 47,748 / 250,324 | — |
  | `gradient` | conical | commit | 10 | 35.70 / 36.93 | 336.74 / 342.60 | 47,748 / 250,324 | 7,680,064 / 58,982,464 |
  | `shape_star` | star5 | preview | 100 | 3.76 / 6.39 | 54.38 / 75.25 | 41,632 / 250,696 | — |
  | `shape_star` | star5 | commit | 10 | 9.15 / 9.90 | 124.53 / 125.97 | 41,632 / 250,696 | 1,797,536 / 13,509,632 |
  | `shape_freeform` | spiral | preview | 600 | 4.33 / 9.29 | 59.31 / 101.45 | 43,908 / 250,380 | — |
  | `shape_freeform` | spiral | commit | 10 | 11.42 / 12.42 | 151.64 / 158.63 | 43,908 / 250,380 | 2,452,528 / 18,414,576 |
  | `text` | 24px | typing | 140 | 3.29 / 3.77 | 44.92 / 46.88 | 43,172 / 250,960 | — |
  | `text` | 24px | commit | 10 | 5.92 / 6.12 | 86.97 / 87.46 | 43,172 / 250,960 | 20,752 / 21,056 |
  | `text` | 96px | typing | 140 | 11.19 / 17.54 | 53.21 / 70.08 | 55,464 / 250,664 | — |
  | `text` | 96px | commit | 10 | 19.65 / 20.23 | 100.30 / 101.83 | 55,464 / 250,664 | 203,792 / 204,944 |
  | `text` | 240px | typing | 140 | 44.05 / 55.52 | 96.73 / 148.99 | 117,096 / 250,712 | — |
  | `text` | 240px | commit | 10 | 55.40 / 55.86 | 190.56 / 191.64 | 117,096 / 250,712 | 524,832 / 1,156,192 |
  | `bucket` | contiguous | click | 10 | 20.86 / 21.89 | 173.32 / 174.82 | 41,600 / 250,540 | 1,920,128 / 14,745,728 |
  | `bucket` | contiguous | refresh | 10 | 4.92 / 5.05 | 86.13 / 86.32 | 41,600 / 250,540 | — |
  | `bucket` | global | click | 10 | 4.69 / 4.78 | 35.68 / 36.14 | 41,132 / 250,640 | 1,920,128 / 14,745,728 |
  | `bucket` | global | refresh | 10 | 4.62 / 4.93 | 86.15 / 87.44 | 41,132 / 250,640 | — |
  | `magic_wand` | default | click | 10 | 7.88 / 8.09 | 74.32 / 74.73 | 39,924 / 250,800 | 256 / 256 |
  | `magic_wand` | default | refresh | 10 | 0.51 / 0.53 | 0.96 / 0.99 | 39,924 / 250,800 | — |
  | `clone_stamp` | default | start | 10 | 0.19 / 0.24 | 1.04 / 1.11 | 44,880 / 276,292 | — |
  | `clone_stamp` | default | stroke | 100 | 0.14 / 0.22 | 0.27 / 0.39 | 44,880 / 276,292 | — |
  | `clone_stamp` | default | release | 10 | 0.15 / 0.23 | 0.33 / 0.35 | 44,880 / 276,292 | 80,128 / 237,152 |
  | `layer_opacity` | default | drag | 100 | 2.43 / 2.61 | 43.16 / 45.07 | 39,744 / 250,808 | 128 / 128 |
  | `thumbnails` | after-stroke | refresh | 100 | 2.12 / 2.38 | 40.40 / 43.55 | 39,560 / 250,300 | — |
  | `save_dialog` | pdn-1 | encode | 10 | 12.23 / 15.16 | 105.15 / 106.38 | 43,040 / 269,976 | — |
  | `save_dialog` | pdn-5 | encode | 10 | 13.58 / 14.52 | 117.01 / 118.76 | 42,236 / 270,060 | — |
  | `save_dialog` | pdn-9 | encode | 10 | 13.73 / 14.13 | 118.15 / 127.03 | 42,108 / 269,808 | — |
  | `save_dialog` | png-none | encode | 10 | 8.24 / 8.42 | 111.29 / 114.56 | 26,388 / 250,432 | — |
  | `save_dialog` | png-fast | encode | 10 | 5.13 / 5.46 | 82.69 / 88.00 | 26,476 / 250,552 | — |
  | `save_dialog` | png-balanced | encode | 10 | 7.27 / 7.45 | 97.61 / 112.08 | 26,596 / 250,816 | — |
  | `save_dialog` | png-high | encode | 10 | 7.90 / 8.44 | 96.88 / 101.91 | 26,788 / 250,800 | — |
  | `pdn_open` | default | open | 10 | 10.62 / 10.73 | 124.39 / 135.55 | 39,876 / 250,528 | — |
  | `pdn_save` | default | save | 10 | 8.69 / 9.28 | 89.07 / 96.72 | 41,320 / 258,540 | — |
  | `png_export` | default | export | 10 | 4.50 / 4.76 | 56.78 / 59.08 | 25,816 / 250,552 | — |

  Reading the table:

  - Layers are uniform, as in phase 1, so encoding times are favorable. `transform_layer`, `rotate_zoom`, and `clone_stamp` use a patterned top layer.
  - `layer_opacity` and `save_dialog` call the work their control triggers instead of locating the widget. Each `save_dialog` sample is one re-encoding with its preview.
  - Shape previews grow during the drag, so their maximum is well above the median.
  - `VmHWM` includes document setup. Undo bytes are the history's own estimate (plan item 3.2).

  **Zero-copy pixels (plan item 2.1, November 27, 2026).** Same environment and method, comparing commit `07e58e2` with `53c5107`; compare these columns only with each other. Large document unless noted, median milliseconds; output is byte-identical.

  | Measurement | Before | After |
  | --- | ---: | ---: |
  | Open to editable canvas | `125.69` | `111.37` |
  | PNG export | `56.95` | `49.44` |
  | Layer opacity drag frame | `43.90` | `36.69` |
  | Thumbnail refresh after a stroke | `41.21` | `34.02` |
  | Peak `VmHWM`, open case | `249,888 kB` | `250,312 kB` |
  | Importing and exporting an `8000 × 8000` RGBA PNG (CLI) | `870.4` | `578.9` |
  | Peak resident memory of that conversion | `755,232 kB` | `505,616 kB` |

  **Affine resampling (plan item 2.2, November 27, 2026).** Same environment, comparing commit `cfa709b` with `35d85e7`: the middle of the median frame times from three rounds that alternated both builds, in milliseconds (typical / large). Output is byte-identical.

  | Workload | Phase | Before | After |
  | --- | --- | ---: | ---: |
  | Move a whole layer (nearest) | preview | `8.45` / `84.94` | `5.18` / `59.39` |
  | Move a whole layer (nearest) | commit | `9.90` / `118.95` | `6.23` / `91.02` |
  | Move a `64 × 64` floating paste (bilinear) | preview | `14.72` / `133.02` | `2.61` / `39.37` |
  | Move a `64 × 64` floating paste (bilinear) | commit | `16.90` / `167.69` | `5.94` / `86.30` |
  | Rotate/Zoom a layer (nearest) | preview | `8.61` / `86.29` | `5.49` / `62.40` |
  | Rotate/Zoom a layer (nearest) | commit | `10.22` / `123.94` | `7.04` / `100.17` |

  **Gradient row buffer (plan item 2.3, November 27, 2026).** Same environment, comparing commit `a09c138` with `780da0c`: the middle of three alternating rounds of five timed runs, in milliseconds (typical / large). Output is byte-identical.

  | Gradient | Phase | Before | After |
  | --- | --- | ---: | ---: |
  | Linear | preview | `24.51` / `206.65` | `23.73` / `201.56` |
  | Linear | commit | `27.90` / `264.06` | `26.96` / `259.69` |
  | Conical | preview | `31.61` / `264.12` | `31.42` / `262.49` |
  | Conical | commit | `34.63` / `321.67` | `34.44` / `319.00` |

  **Vector coverage mask (plan item 2.4, November 27, 2026).** Same environment, comparing commit `70acebd` with `b461071`: the middle of three alternating rounds of five timed runs, in milliseconds (typical / large). Output is byte-identical.

  | Large shape drag | Phase | Before | After |
  | --- | --- | ---: | ---: |
  | Five-point star | preview | `3.27` / `46.62` | `2.48` / `39.73` |
  | Five-point star | commit | `8.17` / `109.88` | `5.31` / `85.16` |
  | Freeform spiral | preview | `3.89` / `51.14` | `2.60` / `40.64` |
  | Freeform spiral | commit | `10.21` / `142.88` | `5.88` / `89.45` |

  **Outline text (plan item 2.5, November 27, 2026).** Same environment, comparing commit `fa069ba` with both text styles at `e003098`: the middle of two alternating rounds of five timed runs, in milliseconds (typical / large), with Analytical antialiasing. Text edges now follow the antialiasing mode, so pixels differ.

  | Text size | Phase | Before | Sharp | Smooth |
  | --- | --- | ---: | ---: | ---: |
  | `24 px` | typing | `2.81` / `38.22` | `2.07` / `36.24` | `2.11` / `36.32` |
  | `24 px` | commit | `5.09` / `73.29` | `3.95` / `70.91` | `4.01` / `70.81` |
  | `96 px` | typing | `10.87` / `46.40` | `2.20` / `36.50` | `2.58` / `36.94` |
  | `96 px` | commit | `18.96` / `86.92` | `4.18` / `71.60` | `4.83` / `71.40` |
  | `240 px` | typing | `43.74` / `89.96` | `2.41` / `36.87` | `4.14` / `39.05` |
  | `240 px` | commit | `54.53` / `174.07` | `4.43` / `71.89` | `6.10` / `75.35` |

  At `240 px` on the typical document, peak `VmHWM` fell from `117,602 kB` to about `46,500 kB`.

  **Incremental thumbnails (plan item 2.6, November 27, 2026).** Same environment, comparing commit `d097a62` with `cbc77db`: the middle of three alternating rounds of five timed runs, in milliseconds (typical / large).

  | Measurement | Before | After |
  | --- | ---: | ---: |
  | Thumbnail refresh frame after a stroke | `1.84` / `33.58` | `0.12` / `0.14` |
  | Open to editable canvas | `9.54` / `109.22` | `7.81` / `75.54` |
  | Layer opacity drag frame | `1.94` / `35.03` | `1.96` / `34.97` |

**Acceptance criteria** — Publish the format and metadata support matrix, test file and clipboard integration on Wayland and X11, and record reproducible interactive benchmarks.

### Formats shown in the Paint.NET 5.2 Open dialog

The screenshot shows 13 format families and 24 extensions. **All Image Types** includes the extensions below. Verify write support and individual format variants separately from the Open dialog filters.

| Family | Extensions shown | OpenPaintR today |
| --- | --- | --- |
| Paint.NET | `.pdn` | Reads and writes layers |
| PNG | `.png` | Imports static PNG and exports the composite |
| JPEG | `.jpg`, `.jpeg`, `.jpe`, `.jfif`, `.exif` | Planned for phase 4 |
| JPEG XL | `.jxl` | Planned for phase 4 |
| AV1 (AVIF) | `.avif` | Planned for phase 4 |
| HEIC | `.heic`, `.heif`, `.hif` | Planned for phase 4 |
| WebP | `.webp` | Planned for phase 4 |
| DirectDraw Surface (DDS) | `.dds` | Planned for phase 4 |
| TIFF | `.tiff`, `.tif` | Planned for phase 4 |
| GIF | `.gif` | Planned for phase 4 |
| BMP | `.bmp`, `.dib`, `.rle` | Planned for phase 4 |
| TGA | `.tga` | Planned for phase 4 |
| JPEG XR | `.jxr`, `.wdp`, `.wmp` | Planned for phase 4 |

### Photoshop PSD

- [ ] **PSD import in phase 4.** Start with 8-bit-per-channel RGB. Preserve raster-layer pixels, transparency, position, order, names, visibility, and opacity. Map blend modes already represented by OpenPaintR. Validate compositing and properties against files generated in Photoshop. [Adobe PSD specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/)
- [ ] **Additional compatibility.** Warn when editable text, smart objects, masks, groups, adjustment layers, or other content cannot be preserved. Offer a flattened import when an embedded composite is available. Evaluate writing PSD, PSB support, and other bit depths in later compatibility work.

## Phase 5 — Plugin engine, adjustments, effects catalog, and extensibility

- [ ] **Shared infrastructure and plugin engine.** Implement the portable **`.rpdn-plugin`** package for Windows, macOS, and Linux on `x86_64` and `aarch64`. Provide a sandboxed hybrid runtime using **`wasmtime`** with `simd128`, optional **WGSL** compute shaders via `wgpu` and `naga`, and a deterministic CPU fallback. Implement the WIT contract `rustpdn:plugin@1.0.0`, the `egui` **`PluginUiHost`** for theme-aware declarative UI (`UiSchema`), and the minimal **`rustpdn-plugin-sdk`**. Pass the layer, the selection's `TMask` coverage, declarative parameters, and seed to operations. Provide cancelable asynchronous tile previews, one history entry per commit, reset to defaults, CLI verification (`plugin check/pack/test`), Repeat Last Effect (`Ctrl+F`), an installed plugin manager, actionable `.rpdn-plugin` load and execution errors, and access to the extension catalog.
- [ ] **5A — Adjustments.** Auto-Level, Black and White, Brightness / Contrast, Curves, Exposure, Highlights / Shadows, Hue / Saturation, Invert Alpha, Invert Colors, Levels, Posterize, Sepia, and Temperature / Tint. Share `PluginUiHost`, asynchronous previews, and a single history entry per edit.
- [ ] **5B — First effect plugins.** Ship official `.rpdn-plugin` packages using only the public API and `rustpdn-plugin-sdk`. Start with Gaussian Blur (two separable Wasm and WGSL passes), Square Blur, Sharpen, Pixelate, Add Noise (deterministic `Seed`), and Edge Detect. These cover kernels, halos, borders, antialiased selections, declarative UI, asynchronous previews, and CPU/GPU parity.
- [ ] **5C — Full effects catalog.** Deliver all 48 effects below as `.rpdn-plugin` packages loaded through the same registry as external plugins. This catalog includes the phase 5B effects.

| Category | Effects shown in Paint.NET 5.2 |
| --- | --- |
| Artistic (7) | Ink Sketch, Linocut, Mosaic, Oil Painting, Pencil Sketch, Pointillism, Stained Glass. |
| Blurs (10) | Bokeh Blur, Fragment Blur, Gaussian Blur, Median Blur, Motion Blur, Radial Blur, Sketch Blur, Square Blur, Surface Blur, Zoom Blur. |
| Color (2) | Ordered Dither, Quantize / Dither. |
| Distort (11) | Bulge, Crystalize, Dents, Frosted Glass, Morphology, Pixelate, Polar Inversion, Spherize, Tile Reflection, Twist, Waves. |
| Noise (2) | Add Noise, Reduce Noise. |
| Object (1) | Drop Shadow. |
| Photo (6) | Glow, Red Eye Removal, Sharpen, Soften Portrait, Straighten, Vignette. |
| Render (4) | Clouds, Julia Fractal, Mandelbrot Fractal, Turbulence. |
| Stylize (5) | Edge Detect, Emboss, Halftone, Outline, Relief. |

- [ ] **5D — Historical compatibility.** Evaluate Unfocus as a historical blur variant. Document the obsolete blur as a legacy Gaussian implementation and check whether classes such as Color Fill and Color Difference are exposed before adding menu commands. Rotate/Zoom is delivered in phase 2. [v3 effect source](https://github.com/nakijun/OpenPDN-1/tree/cca476b0df2a2f70996e6b9486ec45327631568c/src/Effects)
- [ ] **5E — Advanced extensibility (codecs and adjustments).** Extend the versioned `rustpdn:plugin@1.x` API and `rustpdn-plugin-sdk` from phase 5 to support third-party file codecs (`FileTypes`) and adjustments in the same `.rpdn-plugin` format. Evaluate Paint.NET 5.2 framework pixel formats and operations, including channel extraction and replacement, conversion, and interpolation. Support `RGBA16`, `RGBA32Float`, CMYK, and HDR at the import boundary as codecs require. Legacy .NET plugin binary compatibility remains a separate scope decision.

**Acceptance criteria** — Each adjustment and effect exposes parameters or a direct action, respects the selection's `TMask` coverage and alpha, and creates one undo entry. Keep reference images for representative settings and use controlled seeds for noise and procedural effects. Compare against Paint.NET 5.2 to establish operation-specific tolerances. Every effect must load from the same `.rpdn-plugin` file on Windows, macOS, and Linux, include a CPU Wasm reference path, and match WGSL within the documented numerical tolerance. Verify that a third-party plugin can be built from scratch with the SDK and guide. [Extensibility guide](extensibility.md)

## Post-Phase 5 — Usability study (SUS) and workspace redesign

Once phase 5 adjustments and `.rpdn-plugin` effects are integrated into the interface, run participant sessions in a controlled Linux environment (recording app version, fixtures, display scale, and input device, with counterbalanced ordering when comparing layouts and explicit start/end/time/help rules across the five core workflows plus adjustments and effects).

- [ ] **Usability.** Record unaided completion, time, and errors per task. Administer the 10-item SUS after the tasks for each version and before discussing the test. The redesign target is a **mean SUS above 80/100**. Interpret the mean with sample size, uncertainty, and task results. Define participants and sample size before the acceptance study. [Original SUS questionnaire](https://germanupa.de/sites/default/files/2021-12/brooke1996sus.pdf) · [Validated Brazilian Portuguese version](https://aquichan.unisabana.edu.co/index.php/aquichan/article/view/16497)
- [ ] **Redesign.** Use the problems that most affect task completion in the SUS sessions and baseline measurements to decide between targeted improvements and a broader workspace redesign. Keep the application's visual identity, preserve document, editing, and history behavior, and repeat comparable evaluation runs on updated builds.

## Future — MCP server for AI agents

Expose the editing engine to AI agents as a local [Model Context Protocol](https://modelcontextprotocol.io) server, alongside the CLI. The server runs as `openpaintr mcp` over stdio, behind an optional `mcp` feature, in its own `openpaintr-mcp` crate built on the official Rust SDK (`rmcp`). It depends on the separate core, engine, and CLI crates and on the CLI commands proposed in the [architecture improvement plan](architecture-improvement-plan.md) (items 5.6 and 5.7). Live control of a running editor window is out of scope.

- [ ] **Stateless tools.** Expose the CLI commands (`info`, `convert`, `flatten`, `resize`, `canvas-size`, `rotate`, `flip`, `export-layer`, and batch processing) as MCP tools with JSON Schema parameters. Define each command once and generate both the CLI arguments and the tool schema from that definition. Tools call the same engine functions as the GUI and the CLI. A `render` tool returns the composite or a single layer as a PNG image, downscaled to a configurable maximum size.
- [ ] **Stateful editing sessions.** Keep documents open between calls through session handles. Create, open, save, and close documents. Add, delete, reorder, and change layers. Select, fill, and erase. Draw gradients, lines and curves, shapes, and text from explicit parameters. Undo, redo, and inspect history. Each tool call creates at most one history entry, as in the GUI. This requires a non-interactive engine API for the tools that the GUI drives with pointer gestures, and editor state that is `Send` (architecture plan item 4.1).
- [ ] **Security.** Read and write only inside directories the user allows (`--allow-dir` or MCP roots). Keep the no-overwrite rule unless overwriting is explicitly allowed. Apply the core decode limits, cap the size of returned images and the memory of each session, and offer stdio only, with no network transport. Treat layer names, metadata, and other file content returned to the model as untrusted data.

**Acceptance criteria** — An MCP client, such as Claude Desktop or the MCP Inspector, lists the tools with their schemas and completes an end-to-end workflow through the server: open a PDN, change a layer's properties, draw, undo, save, and reopen. Results are byte-identical to the corresponding CLI command or GUI action and are checked against the same reference images as the GUI tests. Integration tests cover paths outside the allowed directories, oversized images, and invalid session handles.

## Evidence and scope decisions

- **Roadmap review.** The seven-reviewer synthesis records layout options, the evaluation plan, and decisions that depend on the baseline. [Review brief](../concepts_brief.md)
- **Paint.NET 5.2 screenshots.** The 16 screenshots cover the workspace, Edit, View, Image, Layers, Adjustments, all nine Effects submenus, and the File → Open format filter. Historical source and public documentation supplied additional File, Settings, and Help commands and full tool options. Verify submenus, parameters, and shapes that were closed in the screenshots during each phase.
- **Version 3.** OpenPDN-1 3.36.7 supplies the historical menus, actions, tools, codecs, and effects. Traceability examples include `CopyToClipboardAction`, `PasteInToNewLayerAction`, `CanvasSizeAction`, `CloneStampTool`, `RotateZoomEffect`, and `PdnFileType`. [Reviewed revision](https://github.com/nakijun/OpenPDN-1/tree/cca476b0df2a2f70996e6b9486ec45327631568c)
- **PDN validation.** Fixtures in `tests/fixtures/pdn` (`Untitled`, `Untitled3`, `oldPDN3510`, and `FlattenBlendTest`) are used by `tests/pdn_compat.rs` to verify exact compositing against `oldPDN3510-reference.png` and round-trip all 14 blend layers. Extend coverage in phase 1 against Paint.NET 5.2 references.
- **Scope.** The plan covers the editing features identified in the cited sources. Consider layer groups, text layers, masks, and adjustment layers after the proposed Paint.NET 5.2 parity. [Official roadmap](https://paint.net/roadmap.html)
