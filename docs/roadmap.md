# OpenPaintR Roadmap

**Status** — Approved on November 25, 2026. Last updated on November 26, 2026.

The first GitHub publication uses **OpenPaintR** as the project name and `openpaintr` as the executable. Layered editing, phase 2 selection and geometry, and phase 3 tools are implemented. Cross-version Paint.NET compatibility validation continues in phase 1. Plugin infrastructure, adjustments, and effects are next. [GitHub repository](https://github.com/pateixei/openpaintr)

The test suite has a known failure when dragging the bottom-right corner after entering canvas resize mode with Ctrl and a double-click. Validation of that gesture remains open.

The roadmap starts with a Linux baseline and a layered PDN editing and saving workflow. Baseline results will guide the redesign scope and timing. Later phases cover selection, tools, transforms, the 13 adjustments and 48 effects shown in Paint.NET 5.2, and broader format support, performance, and extensibility. Milestones specify dependencies and acceptance criteria.

OpenPaintR creates images, opens PNG and PDN, exports PNG, and supports brush and eraser strokes, zoom, layers, visibility, opacity, and undo and redo. PDN import preserves layer properties, including blend modes. Opening a layered file and viewing its layers was confirmed on one document. Compatibility by version and compositing equivalence still need measurement. [Current capabilities](../README.md)

**Interoperability.** Store editable documents in interchange formats. Validate PDN with Paint.NET and PSD with Adobe Photoshop. Record compatible versions and preserved features in the acceptance criteria.

**Antialiasing.** Lines, curves, strokes, fills, and selections always use **4×4 SSAA** in previews and in pixels committed to the layer.

**Plugins and effects.** Implement all **Effects** menu entries as portable, cross-platform `.rpdn-plugin` packages with WebAssembly, optional WGSL shaders, and a declarative UI. Use the public API available to third-party authors through the minimal `rustpdn-plugin-sdk`. [Plugin architecture](arquitetura-plugins.md) · [Extensibility guide](extensibilidade.md)

## Phase 1 — Documents, layers, and persistence

- [x] **Files and workspace.** Provide New, Open, Open Recent and Clear Recent Files, Save, Save As, save options with a preview, Export, Close, Close All, and Exit. Prompt for each document with unsaved changes. Open multiple documents in reorderable tabs with thumbnails, provide next and previous navigation and a document list, and keep history, selection, zoom, and position independent per document.
- [x] **Layers.** Add, delete, duplicate, merge down, toggle visibility, and import layers from a file. Navigate to the first, previous, next, or last layer. Move layers to the top or bottom, move them up or down, or reorder by dragging. Show thumbnails and expose name, visibility, opacity, and blend mode.
- [ ] **Compositing.** Validate Normal, Multiply, Additive, Color Burn, Color Dodge, Reflect, Glow, Overlay, Difference, Negation, Lighten, Darken, Screen, and Xor using partial alpha, hidden layers, and overlapping layers. Measure rounding differences between v3 and v5.2 to establish a reference for each version. [Blend modes](https://paint.net/doc/latest/BlendModes.html)
- [x] **History.** Undo and redo document, layer, and selection operations. Navigate to any state from the panel, group each gesture into one entry, and bound memory use. Evaluate temporary diff storage for large documents.
- [ ] **Editable persistence.** Use atomic replacement for Save and Save As. Reopening a PDN must preserve dimensions, RGBA pixels, layer order, names, visibility, opacity, and blend modes, and allow editing to continue. Track the last saved revision independently of undo history. Preserve the open document and last valid save if the operation fails or is canceled. Decide which session state to persist, including zoom, selection, and history.
- [ ] **Compatibility and export.** Test PDN 3.x, 4.x, and 5.x import with versioned fixtures and verify PNG compositing and alpha separately. Preserve the original when using Save As. Reopen generated files in OpenPaintR and in the Paint.NET versions declared compatible.
- [ ] **Workspace interface.** Define access to the operations delivered in this phase. Use the inventory of menus, panels, contextual toolbar, and status bar to plan the redesign. After the baseline, evaluate panel placement, visibility, and restoration with original icons and a distinct visual identity. Expose selection, adjustment, and effect commands as they become available.
- [ ] **Linux baseline.** Pin an editor build and verify the tasks before test sessions. Record usability and performance separately using the plan below.

**Acceptance criteria** — Open a PDN, reorder, duplicate, and merge layers, then undo the changes. Test Save, Close, and Reopen as a separate workflow and verify pixels and layer properties. Compare a PNG export with Paint.NET reference images. Record the Linux baseline and observed difficulties to guide the redesign scope and timing. [v3 menus and actions](https://github.com/nakijun/OpenPDN-1/tree/cca476b0df2a2f70996e6b9486ec45327631568c/src/Menus) · [v5 documentation](https://paint.net/doc/latest/index.html)

### First phase 1 delivery

The editor writes layered PDN files. Save and Save As write atomically and update the saved revision after success. History, active layer, and zoom are session state. Paint.NET 5.2 opened the 958 × 810 PDN produced from the four-layer recovery case. Pixels and layer properties were verified by reopening the file in OpenPaintR. Validation for other Paint.NET versions is ongoing.

The first Linux benchmark used the debug reference executable at `target/baseline/rustpdn-pre-persistencia` (SHA-256 `c5ab51a1d99e08c4d7b4eb91ebd2d07757172b62442ff2c6c3f19ceea4ab6d0c`). On WSL2 6.18.33.2 with a Core i7-13700K, the benchmark timed the historical command `rustpdn pdn entrada.pdn saída.png`, using one warm-up and ten runs per file. Results include opening, compositing, exporting, and process overhead.

| Fixture (800 × 600) | Layers | Median | Maximum |
| --- | ---: | ---: | ---: |
| Untitled | 1 | 175.6 ms | 182.8 ms |
| Untitled3 | 2 | 168.7 ms | 172.7 ms |
| oldPDN3510 | 2 | 249.7 ms | 340.7 ms |
| FlattenBlendTest | 14 | 297.4 ms | 317.5 ms |

The initial ImageMagick AE comparison between the `oldPDN3510` PNG and the local reference found 45 differing pixels due to rounding in `Pixel::over`. After routing `PDNB_NORMAL` through `bm_row_inplace` (`jflib-gfx` / `UserBlendOps`), the comparison found **zero differing pixels** (`tests/pdn_compat.rs`). Interactive benchmarks and usability sessions remain pending.

### Usability, performance, and redesign

Before participant sessions, record the app version, fixtures, Linux environment, display scale, and input device. Use equivalent documents with visible and hidden layers, transparency, and reference results. Define success, start and end conditions, time limits, and help rules for each task.

| Workflow | Expected result |
| --- | --- |
| Open and understand a project | Open a PDN, locate the requested layer, and identify the hidden layer. |
| Edit a layer | Change the correct layer's opacity, draw a stroke, and undo the stroke while retaining the opacity change. |
| Navigate and edit a detail | Reach a region with zoom and scrolling, edit it, and return to the overview. |
| Export to PNG | Write a file to the requested destination and verify visible-layer compositing and transparency. |
| Resume an editable project | Save, close, and reopen the project, verify the layers, and continue editing. Include this task when both versions support the workflow. |

- [ ] **Usability.** Record unaided completion, time, and errors per task. Compare layouts with equivalent features and fixtures. Counterbalance the order if each participant tests both versions. Administer the 10-item SUS after the tasks for each version and before discussing the test. The redesign target is a **mean SUS above 80/100**. Interpret the mean with sample size, uncertainty, and task results. Define participants and sample size before the acceptance study. [Original SUS questionnaire](https://germanupa.de/sites/default/files/2021-12/brooke1996sus.pdf) · [Validated Brazilian Portuguese version](https://aquichan.unisabana.edu.co/index.php/aquichan/article/view/16497)
- [ ] **Interactive performance.** Repeat measurements with typical and large projects on the same Linux hardware. Record time to an editable canvas, stroke-to-display latency, zoom and scroll frame times, export duration, and peak memory. Keep hardware, compositor, display scale, input device, and app version comparable. Use the baseline to set performance targets and locate bottlenecks.
- [ ] **Redesign.** Use the problems that most affect task completion to decide between targeted improvements and a broader redesign. Keep the application's visual identity and reuse document, editing, and history behavior in prototypes. Choose the phase after reviewing initial results and repeat tests on comparable versions.

## Phase 2 — Selection, clipboard, navigation, and geometry

- [x] **Selection.** Rectangle, ellipse, lasso, and Magic Wand. Support tolerance, layer or composite sampling, contiguous and global selection, Replace, Add, Subtract, Intersect, and XOR. Move or transform the mask separately from the pixels. Move, resize, and rotate selected pixels with handles, constrain proportions, and choose a resampler, including antialiased nearest neighbor.
- [x] **Selection feather.** Share the control across Rectangle Select, Ellipse Select, Lasso, and Magic Wand. At 0%, preserve the original SSAA selection. At 100%, fade coverage linearly to zero outside the selection over half the shorter side of its bounding box. Apply feather to each new gesture before combining it with an existing selection. Show partial coverage and preserve it when filling, erasing, copying, or moving the mask.
- [x] **Editing.** Cut, copy, copy flattened, paste, paste into a new layer or image, copy the selection mask, and paste masks with the applicable combination modes. Erase, fill, invert, select all, and deselect. Expose shortcuts and resolve the `Ctrl+Shift+Z` conflict between Redo and Rotate/Zoom.
- [x] **Viewing and navigation.** Zoom in and out, fit the window or selection, return to actual size, and support continuous zoom and a Zoom tool. Pan with the Hand tool or temporarily with Space. Provide a pixel grid and rulers, using document resolution to switch between pixels, inches, and centimeters.
- [x] **Image operations.** Crop to selection, resize with resampling and optional aspect lock, change canvas size with an anchor, flip horizontally or vertically, rotate 90° clockwise or counterclockwise or 180°, and flatten. Color Profile is planned for phase 5.
- [x] **Layer geometry.** Flip horizontally or vertically, rotate 180°, and provide Rotate/Zoom with preview, translation, scale, and rotation. Validate interpolation, edges, transparency, and the distinction between layer and document operations.

**Validation** — In Linux/X11, a UI test selected an irregular region with the lasso, filled it from the keyboard, copied the composite, and opened it in another tab. A separate X11 client received the clipboard PNG. The flow also verified moving the mask without touching pixels, previewing and applying a pixel transform, undo, rulers, and zoom to selection. Automated tests cover combination modes, Magic Wand, DPI, rotation, anchors, aspect ratio, transparency during resampling and cropping, and independent tab history. Native Wayland clipboard integration remains part of phase 5 platform validation.

**Acceptance criteria** — Select an irregular region, copy the composite into another image, move the mask without changing pixels, transform the pixels separately, and undo each step. Test selection, DPI, anchor, and dimensions at boundaries. [Paint.NET Image menu](https://paint.net/doc/latest/ImageMenu.html) · [v5 tools](https://paint.net/doc/latest/ToolsWindow.html) · [v3 source code](https://github.com/nakijun/OpenPDN-1/tree/cca476b0df2a2f70996e6b9486ec45327631568c/src)

## Phase 3 — Tools and colors

- [x] **Strokes.** Brush, Pencil (default `1 px`), and Eraser with widths from `1..=96 px`. Where applicable, support hardness (`0–100%`), spacing (`1–200%`), fill patterns (solid, stripes, diagonals, checkerboard, dots, and grid), always-on 4×4 SSAA, and Blend or Overwrite. Use primary and secondary mouse buttons for the two colors and offer temporary brush sampling with `Ctrl+Click` in Paint.NET or `Alt+Click` in Adobe.
- [x] **Contiguous Paint Bucket.** Fill edge-connected pixels on the active layer with the same RGBA value as the clicked pixel. Respect selection SSAA coverage and record one history entry. Provide a tool icon and profile shortcuts. The next item covers tolerance and global fill.
- [x] **Fill and sampling.** Paint Bucket supports contiguous or global scope (`Shift+Click` for global in Paint.NET), fill patterns, blend modes, and layer or composite sampling. Gradient provides linear, reflected, diamond, radial, conical, and clockwise and counterclockwise spiral modes, with repetition, color or transparency channels, dithering, 4×4 SSAA, and draggable handles. Eyedropper samples the layer or composite (`Ctrl` or `Alt`) and can remain active or restore the previous tool. Clone Stamp sets the source with `Ctrl+Click` in Paint.NET or `Alt+Click` in Adobe, displays a source marker, and snapshots the source at the start of each stroke. Recolor preserves shading.
- [x] **Color tolerance.** Magic Wand, Paint Bucket, and Recolor share a `0–100%` toolbar control and premultiplied or straight alpha matching. At 0%, require an exact match under the selected alpha mode. Magic Wand and Paint Bucket compare with the clicked sample. Recolor compares pixels under the stroke with the sampled target color (`Ctrl` or `Alt`) or the secondary color. Preserve clean separation across antialiased strokes. [Paint.NET toolbar](https://paint.net/doc/latest/Toolbar.html) · [Paint.NET Recolor](https://paint.net/doc/latest/RecolorTool.html)
- [x] **Geometric drawing.** Line/Curve offers `CubicBezier` and `Spline` with four draggable control points, width, dashes, and end caps including arrows. Shapes offer outline, fill, or both, plus patterns and blending. The 26 presets span basic shapes (including rounded rectangles with an adjustable radius), polygons and stars, arrows, symbols, and callouts. Paint.NET v3-style Freeform is also supported. Shapes use 4×4 SSAA with preview and resize and rotate handles. Commit with `Enter` or click, or cancel with `Esc`.
- [x] **Antialiasing in existing tools.** Brush, Eraser, selections, and elliptical fill or erase always use **4×4 SSAA** and preserve partial alpha in history and PDN saves.
- [x] **Always-on SSAA.** The editor applies 4×4 SSAA to outlines and fills during previews and when committing pixels. SSAA is the only antialiasing mode.
- [x] **Text.** Click to position and drag the anchor. Type directly on the canvas or use the contextual toolbar field. Press `Shift+Enter` for a line break, `Enter` to commit, and `Esc` to cancel. Choose a detected system or bundled font, size (`6..=240 px`), bold, italic, underline, strikethrough, and left, center, or right alignment. Rasterize with 4×4 SSAA and record one history entry.
- [x] **Colors.** Primary and secondary colors, swap (`X`), reset (`D`), HSVA wheel, brightness and opacity controls, and a compact or expanded panel with synchronized RGB, HSV, hex, and alpha fields. Provide 32 editable swatches with `+` to add or right-click to remove, Default, Pastel, and Grayscale presets, Paint.NET-compatible `.txt` import and export (`AARRGGBB`), and automatic saving to `paleta.txt`.

**Acceptance criteria** — Each tool follows its active-layer and selection rules, maintains a preview during the gesture, and produces one reversible edit on confirmation. Test sampling with hidden layers and strokes containing semitransparent pixels. [Tools and settings](https://paint.net/doc/latest/ToolsWindow.html) · [Paint.NET 5.2 changelog](https://paint.net/roadmap.html)

### Configurable keyboard and mouse shortcuts

- [x] **Command registry.** Centralize shortcuts and assign a stable identifier to each tool and command. Register configurable bindings in the JSON schema whenever a tool is added.
- [x] **Profiles.** Select, create, and edit profiles in **Settings → Keyboard and Mouse Shortcuts**. The default **Paint.NET** profile follows Paint.NET, and the **Adobe** profile follows Photoshop for the equivalent actions implemented here. Show binding conflicts. [Paint.NET](https://paint.net/doc/latest/KeyboardMouseCommands.html) · [Adobe Photoshop](https://helpx.adobe.com/content/dam/help/en/photoshop/using/default-keyboard-shortcuts/photoshop-keyboard-shortcuts.pdf)
- [x] **Local persistence.** Store profiles, bindings, and the active profile in `atalhos.json` next to the executable. Use the [versioned JSON schema](atalhos.schema.json), preserve custom bindings when tools are added, and report read or write failures in Settings.

- [x] **Mouse gestures.** Share the active profile and `atalhos.json` between keyboard and mouse. The **Mouse** tab remaps strokes, selection gestures, the color wheel, navigation, transforms, and layer commands. The schema distinguishes gesture identifiers from keyboard commands.
- [x] **Stroke width via the wheel.** In the Paint.NET and Adobe profiles, `Ctrl+Shift+Wheel` changes width in 1 px steps over the canvas. Offer the configurable gesture for all tools with pixel-width strokes and update the preview of a line, curve, or shape being edited.
- [x] **Reference gesture parity.** The equivalent Paint.NET and Photoshop gestures are configurable in `atalhos.json` and documented in `atalhos.schema.json`. They include marquee zoom (`zoom.marquee` in Paint.NET), scrubby zoom (`zoom.scrubby` in Adobe), repositioning a selection while drawing (`selection.relocate`, right button in Paint.NET or `Space+Left` in Adobe), Eyedropper, temporary Brush sampling, global Paint Bucket, Clone Stamp source, Recolor target, and handles for gradients, curves, shapes, and text. [Paint.NET](https://paint.net/doc/latest/KeyboardMouseCommands.html) · [Adobe Photoshop](https://helpx.adobe.com/photoshop/desktop/get-started/learn-the-basics/zoom-images.html)

**Shortcut acceptance criteria** — Switch between Paint.NET and Adobe, create a custom profile, change a binding, restart, and verify that the active profile is restored. Add a new action to the schema and verify that existing custom bindings still work.

Switching to Adobe, creating a custom profile, and restoring its binding after a restart were verified in the Linux/X11 interface.

## Phase 4 — Plugin engine, adjustments, and effects catalog

- [ ] **Shared infrastructure and plugin engine.** Implement the portable **`.rpdn-plugin`** package for Windows, macOS, and Linux on `x86_64` and `aarch64`. Provide a sandboxed hybrid runtime using **`wasmtime`** with `simd128`, optional **WGSL** compute shaders via `wgpu` and `naga`, and a deterministic CPU fallback. Implement the WIT contract `rustpdn:plugin@1.0.0`, the `egui` **`PluginUiHost`** for theme-aware declarative UI (`UiSchema`), and the minimal **`rustpdn-plugin-sdk`**. Pass the layer, 4×4 SSAA `TMask`, declarative parameters, and seed to operations. Provide cancelable asynchronous tile previews, one history entry per commit, reset to defaults, CLI verification (`plugin check/pack/test`), and Repeat Last Effect (`Ctrl+F`).
- [ ] **4A — Adjustments.** Auto-Level, Black and White, Brightness / Contrast, Curves, Exposure, Highlights / Shadows, Hue / Saturation, Invert Alpha, Invert Colors, Levels, Posterize, Sepia, and Temperature / Tint. Share `PluginUiHost`, asynchronous previews, and a single history entry per edit.
- [ ] **4B — First effect plugins.** Ship official `.rpdn-plugin` packages using only the public API and `rustpdn-plugin-sdk`. Start with Gaussian Blur (two separable Wasm and WGSL passes), Square Blur, Sharpen, Pixelate, Add Noise (deterministic `Seed`), and Edge Detect. These cover kernels, halos, borders, SSAA selections, declarative UI, asynchronous previews, and CPU/GPU parity.
- [ ] **4C — Full effects catalog.** Deliver all 48 effects below as `.rpdn-plugin` packages loaded through the same registry as external plugins. This catalog includes the phase 4B effects.

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

- [ ] **4D — Historical compatibility.** Evaluate Unfocus as a historical blur variant. Document the obsolete blur as a legacy Gaussian implementation and check whether classes such as Color Fill and Color Difference are exposed before adding menu commands. Rotate/Zoom is delivered in phase 2. [v3 effect source](https://github.com/nakijun/OpenPDN-1/tree/cca476b0df2a2f70996e6b9486ec45327631568c/src/Effects)

**Acceptance criteria** — Each adjustment and effect exposes parameters or a direct action, respects the 4×4 SSAA `TMask` selection and alpha, and creates one undo entry. Keep reference images for representative settings and use controlled seeds for noise and procedural effects. Compare against Paint.NET 5.2 to establish operation-specific tolerances. Every effect must load from the same `.rpdn-plugin` file on Windows, macOS, and Linux, include a CPU Wasm reference path, and match WGSL within the documented numerical tolerance. Verify that a third-party plugin can be built from scratch with the SDK and guide. [Extensibility guide](extensibilidade.md)

## Phase 5 — Color, formats, integration, and extensibility

- [ ] **Color and metadata.** Add Color Profile to the Image menu with assignment and conversion for sRGB, Adobe RGB, Display P3, ProPhoto RGB, and imported ICC/ICM profiles. Preserve profiles, print resolution, and metadata in formats that support them. Cover PNG text, EXIF orientation, and CICP. Evaluate HDR-to-SDR conversion on import for relevant codecs.
- [ ] **Common formats.** Complete PNG options and the read/write matrix for JPEG, BMP/DIB/RLE, GIF, TGA, and TIFF. Test palettes, transparency, quality, compression, and metadata where supported. Define handling for animated formats, including GIF and APNG.
- [ ] **Additional formats.** DDS, WebP, AVIF, HEIC/HEIF, JPEG XL, and JPEG XR. Document read and write support, variants, available Linux dependencies, and round-trip criteria for each codec. [v5 file formats](https://paint.net/doc/latest/index.html)
- [ ] **Desktop and support.** Add printing and scanner or camera acquisition using Linux integrations, file associations and drag-and-drop, tool defaults and settings, languages and themes, help, tutorials, About, and support and feedback links. Use the Linux distribution channel for updates. Provide an installed plugin manager, actionable `.rpdn-plugin` load and execution errors, and access to the extension catalog.
- [ ] **Advanced extensibility (codecs and adjustments).** Extend the versioned `rustpdn:plugin@1.x` API and `rustpdn-plugin-sdk` from phase 4 to support third-party file codecs (`FileTypes`) and adjustments in the same `.rpdn-plugin` format. Evaluate Paint.NET 5.2 framework pixel formats and operations, including channel extraction and replacement, conversion, and interpolation. Support `RGBA16`, `RGBA32Float`, CMYK, and HDR at the import boundary as codecs require. Legacy .NET plugin binary compatibility remains a separate scope decision.
- [ ] **Performance.** Extend phase 1 interactive benchmarks to compositing, clipboard operations, effects, and large layer stacks. Use observed bottlenecks to prioritize region updates, caching, and parallelism. Assess FP32 compositing, SIMD, and a portable GPU backend using precision comparisons and benchmarks. [Rendering, color, and extension changes in 5.2](https://paint.net/roadmap.html)

**Acceptance criteria** — Publish the format and metadata support matrix, test file and clipboard integration on Wayland and X11, and record reproducible interactive benchmarks.

### Formats shown in the Paint.NET 5.2 Open dialog

The screenshot shows 13 format families and 24 extensions. **All Image Types** includes the extensions below. Verify write support and individual format variants separately from the Open dialog filters.

| Family | Extensions shown | OpenPaintR today |
| --- | --- | --- |
| Paint.NET | `.pdn` | Reads and writes layers |
| PNG | `.png` | Imports static PNG and exports the composite |
| JPEG | `.jpg`, `.jpeg`, `.jpe`, `.jfif`, `.exif` | Planned for phase 5 |
| JPEG XL | `.jxl` | Planned for phase 5 |
| AV1 (AVIF) | `.avif` | Planned for phase 5 |
| HEIC | `.heic`, `.heif`, `.hif` | Planned for phase 5 |
| WebP | `.webp` | Planned for phase 5 |
| DirectDraw Surface (DDS) | `.dds` | Planned for phase 5 |
| TIFF | `.tiff`, `.tif` | Planned for phase 5 |
| GIF | `.gif` | Planned for phase 5 |
| BMP | `.bmp`, `.dib`, `.rle` | Planned for phase 5 |
| TGA | `.tga` | Planned for phase 5 |
| JPEG XR | `.jxr`, `.wdp`, `.wmp` | Planned for phase 5 |

### Photoshop PSD

- [ ] **PSD import in phase 5.** Start with 8-bit-per-channel RGB. Preserve raster-layer pixels, transparency, position, order, names, visibility, and opacity. Map blend modes already represented by OpenPaintR. Validate compositing and properties against files generated in Photoshop. [Adobe PSD specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/)
- [ ] **Additional compatibility.** Warn when editable text, smart objects, masks, groups, adjustment layers, or other content cannot be preserved. Offer a flattened import when an embedded composite is available. Evaluate writing PSD, PSB support, and other bit depths in later compatibility work.

## Evidence and scope decisions

- **Roadmap review.** The seven-reviewer synthesis records layout options, the evaluation plan, and decisions that depend on the baseline. [Review brief](../concepts_brief.md)
- **Paint.NET 5.2 screenshots.** The 16 screenshots cover the workspace, Edit, View, Image, Layers, Adjustments, all nine Effects submenus, and the File → Open format filter. Historical source and public documentation supplied additional File, Settings, and Help commands and full tool options. Verify submenus, parameters, and shapes that were closed in the screenshots during each phase.
- **Version 3.** OpenPDN-1 3.36.7 supplies the historical menus, actions, tools, codecs, and effects. Traceability examples include `CopyToClipboardAction`, `PasteInToNewLayerAction`, `CanvasSizeAction`, `CloneStampTool`, `RotateZoomEffect`, and `PdnFileType`. [Reviewed revision](https://github.com/nakijun/OpenPDN-1/tree/cca476b0df2a2f70996e6b9486ec45327631568c)
- **PDN validation.** Fixtures in `tests/fixtures/pdn` (`Untitled`, `Untitled3`, `oldPDN3510`, and `FlattenBlendTest`) are used by `tests/pdn_compat.rs` to verify exact compositing against `oldPDN3510-reference.png` and round-trip all 14 blend layers. Extend coverage in phase 1 as more versions are tested.
- **Scope.** The plan covers the editing features identified in the cited sources. Consider layer groups, text layers, masks, and adjustment layers after the proposed Paint.NET 5.2 parity. [Official roadmap](https://paint.net/roadmap.html)
