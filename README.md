# OpenPaintR

OpenPaintR is a raster image editor written in Rust for Linux. It has its own interface and workflows inspired by Paint.NET. The graphical editor and command-line interface share the same document and layer engine.

## Graphical editor

Run the editor on Linux with Wayland or X11 and OpenGL.

```sh
cargo run --locked
```

Use **Claro** (Light) or **Escuro** (Dark) in the menu bar to switch themes. The horizontal **Color Ribbon** sits below the contextual toolbar. Click the custom, black, or white swatch to select the primary color, then adjust its opacity on the slider beside the hue spectrum. Each swatch keeps its own opacity as you switch colors. Right-click a swatch in the Paint.NET profile or Alt-click it in the Adobe profile to assign it to the secondary color. The chevron opens the floating **Cores** (Colors) panel with an HSVA wheel, brightness and opacity controls, and expandable RGB, HSV, hex, and alpha inputs. It also offers 32 editable swatches, presets, and Paint.NET-compatible palette import and export in `AARRGGBB` text format. The active palette is stored in `palette.txt` in the [configuration directory](#user-files).

Tools are on the left, document tabs and the canvas are in the center, and the **Camadas** (Layers) and **Histórico** (History) panels are on the right. The bottom bar shows image dimensions, coordinates, and zoom. Use **Novo** (New) or the `+` button to create an image, or **Abrir** (Open) to open a Paint.NET document (`.pdn`) or import a PNG. **Arquivo → Abrir recentes** (File → Open Recent) lists recent files. Drag tabs to reorder them or switch with `Ctrl+Tab` and `Ctrl+Shift+Tab`.

The Layers panel lets you add, delete, duplicate, merge down, reorder by dragging, and import layers from files. Edit each layer's name, visibility, opacity, and blend mode. The History panel records strokes, selections, and layer changes. Click a history entry to return to that state, use **Ctrl+Z** to undo, or use **Ctrl+Y** (Paint.NET profile) or **Ctrl+Shift+Z** (Adobe profile) to redo.

After clicking the Layers panel, press **Insert** to create a transparent layer directly above the selected layer or **Delete** to remove the selected layer. The document keeps at least one layer. With focus on the canvas, **Delete** erases the selection.

Use **Salvar** (`Ctrl+S`) and **Salvar como** (`Ctrl+Shift+S`) to save layered PDN files atomically. PDN retains RGBA pixels, layer order, names, visibility, opacity, and blend modes. **Exportar PNG** (Export PNG) writes the visible composition to a new file and offers a preview, bit depth, and compression options. An asterisk marks documents with unsaved changes. Closing a tab or the application prompts for confirmation before discarding changes. File dialogs use the Linux desktop portal.

### Painting and selection

The toolbox includes Brush, Pencil, Eraser, Eyedropper, Clone Stamp, Recolor, Paint Bucket, Gradient, Line/Curve, Shapes (26 presets and Freeform), Text, Rectangle and Ellipse Select, Lasso, Magic Wand, Hand, Zoom, and Move/Transform Pixels. The contextual toolbar exposes the options for the active tool, including hardness, spacing, fill patterns, and blend or overwrite modes where applicable.

Rectangle Select, Ellipse Select, Lasso, and Magic Wand support Replace, Add, Subtract, Intersect, and XOR, plus **Feather (0–100%)**. Hold `Shift` to constrain a rectangle or ellipse to a square or circle, and use `Ctrl` or `Alt` to combine selections. While drawing a rectangle or ellipse, hold the right mouse button in the Paint.NET profile or `Space+Click` in the Adobe profile to move the outline. The **Seleção** (Selection) menu offers Select All (`Ctrl+A`), Deselect (`Ctrl+D` or `Enter`), Invert (`Ctrl+I`), Fill with the current color (`Backspace`), and Erase (`Delete`). Click outside the selection or press **Esc** to clear it.

Magic Wand, Paint Bucket, and Recolor share **Color Tolerance (0–100%)** and premultiplied or straight alpha matching. The Paint Bucket fills contiguous or global matches (`Shift+Click` for global fill in the Paint.NET profile), samples the active layer or the composite, and recomposes supported antialiased edge transitions. It respects the selection's coverage. Set the Clone Stamp source with `Ctrl+Click` (Paint.NET) or `Alt+Click` (Adobe). Gradient supports seven shapes, including linear, reflected, diamond, radial, conical, and two spiral directions, plus repetition, color or transparency modes, and dithering. Text supports direct entry, font, size, style, and alignment, with one history entry per confirmed edit.

Over the canvas, **Ctrl+Shift+wheel up** increases stroke width and **Ctrl+Shift+wheel down** decreases it by 1 px per notch. The gesture applies to Brush, Pencil, Eraser, Clone Stamp, Recolor, Line/Curve, and Shapes. Lines, curves, and shapes update in the preview. The Pencil starts at 1 px and has its own width setting. The editor applies **4×4 SSAA** to strokes, selections, and fill edges in previews and when committing pixels to a layer.

### Editing and navigation

In **Editar** (Edit), copy the active layer or the flattened composition. Paste to the active layer, a new layer, or a new image. A paste into the active layer can be positioned or transformed with handles before you commit it. Pasting into a new image opens a separate tab and preserves the source. You can copy and combine selection masks within the application.

Use **Mover/transformar máscara** (Move/Transform Mask) or **Mover/transformar pixels** (Move/Transform Pixels) to transform them separately. The handles support copying or resizing from the center with `Ctrl` or `Alt`, and aspect-ratio or 15° angle constraints with `Shift`. When transforming pixels or a mask, or editing a gradient, line, curve, shape, or text, drag the handles or nudge with the arrow keys. Press **Enter** to commit or **Esc** to cancel and restore the layer.

The **Exibir** (View) menu controls zoom, the pixel grid, and rulers. Zoom with `Ctrl+wheel` in the Paint.NET profile or `Alt+wheel` in the Adobe profile. The Zoom tool (`Z`) supports drag-to-region zoom in the Paint.NET profile and scrubby zoom in the Adobe profile. Pan with the Hand tool, middle mouse button, or `Space` while dragging. In the Adobe profile, double-click the Hand icon to fit the window or the Zoom icon to return to actual size. Rulers and coordinates can show pixels, inches, or centimeters. Documents without a valid physical resolution use 96 DPI.

The **Imagem** (Image) menu crops to a selection, resizes images or the canvas, flips, rotates, and flattens. Canvas Size supports an anchor. You can also enter canvas resizing with `Ctrl+double-click` at the bottom-right corner, drag the boundary, and press `Enter` or `Ctrl` to confirm or `Esc` to cancel. The **Camadas** (Layers) menu includes **Girar/Zoom** (Rotate/Zoom) with a preview. In the Paint.NET profile, `Ctrl+Shift+Z` opens Rotate/Zoom and `Ctrl+Y` redoes an edit. In the Adobe profile, `Ctrl+Shift+Z` redoes an edit and `Ctrl+T` opens Free Transform.

### Shortcut profiles

Open **Configurações → Atalhos de teclado e mouse** (Settings → Keyboard and Mouse Shortcuts). Select **Paint.NET** (the default) or **Adobe**, or duplicate a profile to customize keyboard and mouse bindings. Enter combinations such as `Ctrl+Shift+B` or `Ctrl+Wheel`, separate alternatives with `;`, and leave a field empty to remove its binding. Select **Salvar perfis** (Save Profiles) to apply changes. Profiles and the active profile are stored in `shortcuts.json` in the [configuration directory](#user-files). If it can't be written, the application reports the save error. [JSON Schema](docs/shortcuts.schema.json)

### Text rendering

Text follows the antialiasing mode chosen under **Settings → Antialiasing**. **Settings → Text rendering** selects one of two styles. **Sharp** (the default) draws heavier edges, rounds baselines to pixel rows up to 48 px, and uses the font's bold file when it exists (DejaVu and Liberation, among others). **Smooth** draws lighter edges at the exact position and simulates bold. Fonts without a separate bold file, such as the built-in ones, have simulated bold in both styles.

### Interface language

Choose a language under **Settings → Language**. The change takes effect immediately and is saved in `language.json` in the [configuration directory](#user-files). The first launch uses a supported system language or falls back to English. The bundled catalogs cover English, French, Spanish, German, Brazilian Portuguese, Italian, Russian, Arabic, Simplified Chinese, and Japanese. Arabic labels read right to left within the existing editor layout. System file-picker controls use the desktop's language.

At startup, OpenPaintR adds to or overrides its bundled catalogs from these locations, each one taking precedence over the previous ones:

1. `openpaintr/translations/` in each directory of `$XDG_DATA_DIRS` (defaults to `/usr/local/share` and `/usr/share`; the first listed directory wins);
2. `translations/` beside the executable;
3. `$XDG_DATA_HOME/openpaintr/translations/` (defaults to `~/.local/share/openpaintr/translations/`). In portable mode, or with `OPENPAINTR_DATA_DIR`, `translations/` in the data directory replaces this one.

The working directory is not searched. Only regular files of up to 1 MiB are loaded. To add a language, save a flat JSON object under a lowercase locale filename such as `nl-nl.json` and restart. Use `language-name` to set its name in the language menu. Use [`en-us.json`](translations/en-us.json) as the key reference and keep numbered placeholders such as `{0}` intact. Missing messages in an additional language fall back to English.

### User files

| Files | Location |
| --- | --- |
| `settings.json`, `language.json`, `shortcuts.json`, `palette.txt` | `$XDG_CONFIG_HOME/openpaintr/` (defaults to `~/.config/openpaintr/`) |
| `recent.json` | `$XDG_STATE_HOME/openpaintr/` (defaults to `~/.local/state/openpaintr/`) |

**Portable mode.** When a file named `portable` exists beside the executable, all of these files and your translations are kept in a `data/` directory beside it. If that directory can't be written, saving reports an error; nothing is written elsewhere. For development and tests, set `OPENPAINTR_DATA_DIR` to an absolute path to use that directory in any mode.

**Upgrading.** Earlier versions kept `atalhos.json`, `paleta.txt`, and `recentes.json` beside the executable. When the corresponding new file doesn't exist yet, OpenPaintR copies the old one to its new name and location, and leaves the old file untouched.

### Window controls

The menu bar includes light and dark theme buttons and controls to minimize, maximize or restore, and close the window. Drag the empty part of the menu bar to move the window, double-click it to maximize or restore, and drag an edge or corner to resize. Closing the window prompts you about unsaved documents.

## Core and command line

The core provides an 8-bit-per-channel RGBA canvas, canvas-sized layers, pixel editing, and bottom-to-top compositing across all 14 Paint.NET blend modes, accelerated by SIMD kernels from `jflib-gfx`. Each layer has a name, visibility, opacity, and a blend mode. The demo generates a 320 × 240 image with two overlapping shapes on a transparent background.

Use Rust 1.98 or later and Cargo. After downloading dependencies with `cargo fetch --locked`, you can run the commands offline.

```sh
cargo test --locked --offline
cargo run --locked --offline --no-default-features -- demo openpaintr-demo.pam
cargo run --locked --offline --no-default-features -- demo-png openpaintr-demo.png
cargo run --locked --offline --no-default-features -- png input.png output.png
cargo run --locked --offline --no-default-features -- pdn input.pdn output.png
```

Use a new output path for each run. The CLI preserves existing files.

The `png` crate reads and writes PNGs. Static PNG import creates one 8-bit RGBA raster layer. PNG export flattens the visible layers into a single image and preserves transparency. Animated PNG (APNG) is pending.

The `jflib-img-pdn` crate reads and writes layered Paint.NET documents. OpenPaintR preserves layer order, names, visibility, opacity, blend modes, and the ICC profile, resolution, and document and layer metadata supported by the codec. PSD support is planned, with Adobe Photoshop as the compatibility reference.

## Project documentation

- [Roadmap and acceptance criteria](docs/roadmap.md)
- [Technical references and compatibility experiments](docs/references.md)
- [Plugin architecture](docs/plugin-architecture.md) and [extensibility guide](docs/extensibility.md) for the planned cross-platform `.rpdn-plugin` format. The plan requires all effects to use WebAssembly, optional WGSL shaders, and a declarative UI.
