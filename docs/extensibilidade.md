# OpenPaintR Extensibility Guide

This guide describes the planned workflow for building, testing, and packaging a portable OpenPaintR **`.rpdn-plugin`**. A package is designed to run on **Linux, Windows, and macOS** (`x86_64` and `aarch64`).

See the [plugin architecture](arquitetura-plugins.md) for the format and API specification. The runtime, SDK, and packaging commands are planned for phase 4.

---

## 1. Requirements and project layout

Install the Rust toolchain's WebAssembly target once.

```sh
rustup target add wasm32-unknown-unknown
```

A minimal plugin project has this layout.

```text
my-effect/
├── Cargo.toml
├── manifest.json
├── shaders/            # Optional, for hybrid Wasm and WGSL plugins
│   ├── pass_h.wgsl
│   └── pass_v.wgsl
└── src/
    └── lib.rs
```

### Minimal `Cargo.toml`

```toml
[package]
name = "openpaintr-noise-effect"
version = "1.0.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
rustpdn-plugin-sdk = "1.0"
bytemuck = { version = "1", features = ["derive"] } # Required only for WGSL uniforms
```

Enable 128-bit WebAssembly SIMD in `.cargo/config.toml`.

```toml
[target.wasm32-unknown-unknown]
rustflags = ["-C", "target-feature=+simd128"]
```

---

## 2. Example 1 — CPU WebAssembly effect

This example implements deterministic **Add Noise** with three declarative controls. OpenPaintR displays an **Intensity** slider, a **Monochrome** checkbox, and a **Seed** control with a randomize button.

### `manifest.json`

```json
{
  "schema_version": 1,
  "id": "com.example.openpaintr.noise",
  "name": "Add Noise",
  "version": "1.0.0",
  "api_version": "1.0",
  "kind": "effect",
  "author": "Your Name",
  "license": "MIT",
  "menu": {
    "category": "Noise",
    "label": "Add Noise...",
    "order": 10
  },
  "capabilities": {
    "cpu_wasm": true,
    "gpu_wgsl": false,
    "pixel_format": "rgba8unorm",
    "max_halo_px": 0,
    "needs_seed": true,
    "supports_repeat": true
  },
  "entrypoints": {
    "wasm": "plugin.wasm"
  }
}
```

### `src/lib.rs`

```rust
use rustpdn_plugin_sdk::{
    BorderMode, EffectContext, EffectPlugin, Params, PixelFormat, RenderPlan, TileMut, TileView,
    UiBuilder, UiSchema, export_effect,
};

pub struct AddNoisePlugin;

impl EffectPlugin for AddNoisePlugin {
    fn describe_ui(&self) -> UiSchema {
        UiBuilder::new("Add Noise")
            .slider_f32("intensity", "Intensity", 0.0, 100.0, 25.0, Some("%"))
            .checkbox("mono", "Monochrome", false)
            .seed("seed", "Random seed", 42)
            .build()
    }

    fn plan_render(
        &self,
        _ctx: &EffectContext,
        _params: &Params,
    ) -> Result<RenderPlan, String> {
        // Each pixel depends only on itself and its (x, y) position, so the halo is zero.
        Ok(RenderPlan::cpu_tiles(0, BorderMode::Clamp, PixelFormat::Rgba8Unorm))
    }

    fn render_tile(
        &self,
        ctx: &EffectContext,
        params: &Params,
        src: &TileView,
        dst: &mut TileMut,
    ) -> Result<(), String> {
        let strength = (params.f32_or("intensity", 25.0) / 100.0 * 127.0) as i32;
        let mono = params.bool_or("mono", false);
        let seed = params.seed_or("seed", ctx.seed);

        for y in 0..dst.height() {
            for x in 0..dst.width() {
                let gx = dst.origin_x() + x;
                let gy = dst.origin_y() + y;
                let [r, g, b, a] = src.rgba8(x, y);

                if a == 0 || strength == 0 {
                    dst.set_rgba8(x, y, [r, g, b, a]);
                    continue;
                }

                let h = hash_2d(gx, gy, seed);
                let delta = |shift: u32| -> i32 {
                    let v = ((h >> shift) & 0xff) as i32;
                    ((v - 128) * strength) / 128
                };

                let (dr, dg, db) = if mono {
                    let d = delta(0);
                    (d, d, d)
                } else {
                    (delta(0), delta(8), delta(16))
                };

                dst.set_rgba8(
                    x,
                    y,
                    [
                        (r as i32 + dr).clamp(0, 255) as u8,
                        (g as i32 + dg).clamp(0, 255) as u8,
                        (b as i32 + db).clamp(0, 255) as u8,
                        a,
                    ],
                );
            }
        }
        Ok(())
    }
}

fn hash_2d(x: u32, y: u32, seed: u64) -> u64 {
    let mut z = seed ^ (u64::from(x) << 32) ^ u64::from(y);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

export_effect!(AddNoisePlugin);
```

- Using global coordinates `(dst.origin_x() + x, dst.origin_y() + y)` avoids seams at `256 × 256 px` tile boundaries.
- The host applies the selection's `TMask` coverage when committing a tile to the layer.

---

## 3. Example 2 — Hybrid Wasm and WGSL plugin

For costly neighborhood effects such as separable **Gaussian Blur**, `plugin.wasm` describes the UI and plans **two WGSL passes**. `render_tile` provides the deterministic CPU fallback.

### Horizontal pass in `shaders/pass_h.wgsl` (the vertical pass uses the same structure)

```wgsl
struct HostUniforms {
    width: u32,
    height: u32,
    seed_lo: u32,
    seed_hi: u32,
};

struct BlurUniforms {
    radius: u32,
    sigma: f32,
    _pad0: u32,
    _pad1: u32,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var dst_tex: texture_storage_2d<rgba32float, write>;
@group(0) @binding(2) var src_sampler: sampler;
@group(0) @binding(3) var<uniform> host: HostUniforms;
@group(0) @binding(4) var<uniform> params: BlurUniforms;

@compute @workgroup_size(16, 16, 1)
fn cs_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= host.width || gid.y >= host.height) {
        return;
    }

    let r = i32(params.radius);
    let two_sigma_sq = max(2.0 * params.sigma * params.sigma, 0.0001);
    var acc = vec4<f32>(0.0);
    var w_sum = 0.0;

    for (var dx = -r; dx <= r; dx = dx + 1) {
        let sx = clamp(i32(gid.x) + dx, 0, i32(host.width) - 1);
        let px = textureLoad(src_tex, vec2<i32>(sx, i32(gid.y)), 0);
        let w = exp(-f32(dx * dx) / two_sigma_sq);
        // Accumulate premultiplied RGBA to avoid darkening transparent edges.
        acc += vec4<f32>(px.rgb * px.a, px.a) * w;
        w_sum += w;
    }

    let out_pre = acc / max(w_sum, 1e-6);
    let out_rgb = select(vec3<f32>(0.0), out_pre.rgb / out_pre.a, out_pre.a > 1e-6);
    textureStore(dst_tex, vec2<i32>(gid.xy), vec4<f32>(out_rgb, out_pre.a));
}
```

### Render planning in `src/lib.rs`

```rust
use bytemuck::{Pod, Zeroable};
use rustpdn_plugin_sdk::{
    BorderMode, EffectContext, EffectPlugin, GpuPlanBuilder, Params, PixelFormat, RenderPlan,
    TileMut, TileView, UiBuilder, UiSchema, export_effect,
};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlurUniforms {
    radius: u32,
    sigma: f32,
    _pad: [u32; 2],
}

pub struct GaussianBlurPlugin;

impl EffectPlugin for GaussianBlurPlugin {
    fn describe_ui(&self) -> UiSchema {
        UiBuilder::new("Gaussian Blur")
            .slider_i32("radius", "Radius", 0, 200, 6, Some("px"))
            .build()
    }

    fn plan_render(&self, _ctx: &EffectContext, params: &Params) -> Result<RenderPlan, String> {
        let radius = params.i32_or("radius", 6).clamp(0, 200) as u32;
        if radius == 0 {
            return Ok(RenderPlan::cpu_tiles(0, BorderMode::Clamp, PixelFormat::Rgba32Float));
        }
        let uniforms = BlurUniforms {
            radius,
            sigma: (radius as f32 * 0.5).max(0.5),
            _pad: [0; 2],
        };
        Ok(GpuPlanBuilder::new(radius, BorderMode::Clamp, PixelFormat::Rgba32Float)
            .add_pass("blur_h", (16, 16, 1), bytemuck::bytes_of(&uniforms), false)
            .add_pass("blur_v", (16, 16, 1), bytemuck::bytes_of(&uniforms), true)
            .build())
    }

    fn render_tile(
        &self,
        _ctx: &EffectContext,
        params: &Params,
        src: &TileView,
        dst: &mut TileMut,
    ) -> Result<(), String> {
        let radius = params.i32_or("radius", 6).clamp(0, 200) as i32;
        // Implement the equivalent CPU pass with src.rgba32f_clamped(x + dx, y + dy).
        let _ = (radius, src, dst);
        Ok(())
    }
}

export_effect!(GaussianBlurPlugin);
```

---

## 4. Declarative UI quick reference

| `UiBuilder` control | Type stored in `Params` | OpenPaintR `egui` widget |
| --- | --- | --- |
| `slider_f32(key, label, min, max, default, unit)` | `f32` | Continuous slider with a numeric field and a unit suffix such as `%` or `px`. |
| `slider_i32(key, label, min, max, default, unit)` | `i32` | Integer slider with fine adjustment. |
| `angle(key, label, default_deg)` | `f32` | Circular angle dial and a degree field (`-180°` to `180°`). |
| `center_offset(key, label, default_x, default_y)` | `(f32, f32)` | Normalized 2D pad (`-1.0..=1.0`) for a distortion or light center. |
| `color(key, label, default_rgba, allow_alpha)` | `[u8; 4]` | Themed color button with optional alpha. |
| `checkbox(key, label, default)` | `bool` | Standard checkbox. |
| `select(key, label, default, options)` | `u32` | Windows-style drop-down combo box. |
| `radio(key, label, default, options)` | `u32` | Group of mutually exclusive options. |
| `seed(key, label, default)` | `u64` | Numeric field and **Randomize** button. |
| `section(title, ...)` / `visible_if(key, value)` | — | Collapsible `styled_card` and dynamic conditional visibility. |

---

## 5. Processing guidelines

1. Return the exact `halo_px` from `plan_render` and read only within `[-halo, width + halo)` in `TileView`. For a radius of 12, return `halo = 12`.
2. For convolution, multiply RGB by alpha before accumulating and divide by the resulting alpha at the end. Use `PixelFormat::Rgba32Float` to avoid dark halos around transparent areas.
3. Keep results deterministic across tiles. Hash `seed` with `(origin_x + x, origin_y + y)` so random output is independent of tile order.
4. Maintain parity between WGSL and Wasm. CPU `render_tile` provides the reference for automated tests and systems without `wgpu` support.
5. Let the host apply the active light pastel or dark theme to help text. Leave text and background colors unstyled.

---

## 6. Packaging, installation, and validation

Build the WebAssembly module in release mode.

```sh
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/openpaintr_noise_effect.wasm plugin.wasm
```

Package and validate with the OpenPaintR CLI planned for phase 4.

```sh
# Validate the manifest, WIT exports, and WGSL shaders through naga
openpaintr plugin check .

# Build the portable my-effect.rpdn-plugin package
openpaintr plugin pack . -o my-effect.rpdn-plugin

# Run headless checks for preview, antialiased selection, alpha edges, and CPU/GPU parity
openpaintr plugin test my-effect.rpdn-plugin
```

### Where to install `.rpdn-plugin` packages

To test a package in the editor, copy the `.rpdn-plugin` file to the user directory for your operating system.

- **Linux** — `~/.local/share/openpaintr/plugins/`
- **Windows** — `%APPDATA%\OpenPaintR\plugins\`
- **macOS** — `~/Library/Application Support/OpenPaintR/plugins/`
