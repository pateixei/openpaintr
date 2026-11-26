# Guia de Extensibilidade do OpenPaintR

Este guia mostra como criar, testar e empacotar um plugin portátil **`.rpdn-plugin`** para o OpenPaintR. O mesmo arquivo gerado roda sem alteração em **Linux, Windows e macOS** (`x86_64` e `aarch64`).

Antes de começar, consulte as definições formais em [Arquitetura de plugins](arquitetura-plugins.md).

---

## 1. Requisitos e estrutura do projeto

Instale o alvo WebAssembly da toolchain Rust (uma única vez):

```sh
rustup target add wasm32-unknown-unknown
```

Um projeto mínimo de plugin tem a estrutura:

```text
meu-efeito/
├── Cargo.toml
├── manifest.json
├── shaders/            # Opcional: apenas para plugins híbridos Wasm + WGSL
│   ├── pass_h.wgsl
│   └── pass_v.wgsl
└── src/
    └── lib.rs
```

### `Cargo.toml` mínimo

```toml
[package]
name = "openpaintr-efeito-ruido"
version = "1.0.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
rustpdn-plugin-sdk = "1.0"
bytemuck = { version = "1", features = ["derive"] } # Necessário apenas se enviar uniforms WGSL
```

Sugestão de `.cargo/config.toml` para habilitar SIMD 128-bit no WebAssembly:

```toml
[target.wasm32-unknown-unknown]
rustflags = ["-C", "target-feature=+simd128"]
```

---

## 2. Exemplo 1 — Plugin de efeito em WebAssembly (CPU)

Este exemplo implementa um efeito determinístico de **Adicionar Ruído** com três controles na UI declarativa: slider de **Intensidade**, checkbox **Monocromático** e **Semente** com botão de re-sorteio automático no diálogo do OpenPaintR.

### `manifest.json`

```json
{
  "schema_version": 1,
  "id": "com.exemplo.openpaintr.ruido",
  "name": "Adicionar Ruído",
  "version": "1.0.0",
  "api_version": "1.0",
  "kind": "effect",
  "author": "Seu Nome",
  "license": "MIT",
  "menu": {
    "category": "Ruído",
    "label": "Adicionar Ruído...",
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
        UiBuilder::new("Adicionar Ruído")
            .slider_f32("intensity", "Intensidade", 0.0, 100.0, 25.0, Some("%"))
            .checkbox("mono", "Monocromático", false)
            .seed("seed", "Semente aleatória", 42)
            .build()
    }

    fn plan_render(
        &self,
        _ctx: &EffectContext,
        _params: &Params,
    ) -> Result<RenderPlan, String> {
        // Halo 0 porque cada pixel depende apenas de si mesmo e da coordenada (x, y)
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

Pontos importantes neste código:
- A coordenada global `(dst.origin_x() + x, dst.origin_y() + y)` garante que não apareçam emendas nas fronteiras dos tiles `256 × 256 px`.
- Você não precisa checar a seleção ativa nem aplicar SSAA manualmente: o host aplica a máscara `TMask` (SSAA 4×4) ao gravar o resultado do tile na camada.

---

## 3. Exemplo 2 — Plugin híbrido (`Wasm` + shaders `WGSL`)

Para efeitos pesados de vizinhança (ex.: **Desfoque Gaussiano separável**), `plugin.wasm` descreve a UI e monta o plano com **dois passes `WGSL`**, mantendo `render_tile` como *fallback* determinístico em CPU.

### `shaders/pass_h.wgsl` (passe horizontal — análogo ao `pass_v.wgsl`)

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
        // Acumula em RGBA pré-multiplicado para evitar escurecimento em bordas transparentes
        acc += vec4<f32>(px.rgb * px.a, px.a) * w;
        w_sum += w;
    }

    let out_pre = acc / max(w_sum, 1e-6);
    let out_rgb = select(vec3<f32>(0.0), out_pre.rgb / out_pre.a, out_pre.a > 1e-6);
    textureStore(dst_tex, vec2<i32>(gid.xy), vec4<f32>(out_rgb, out_pre.a));
}
```

### Planejamento em `src/lib.rs`

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
        UiBuilder::new("Desfoque Gaussiano")
            .slider_i32("radius", "Raio", 0, 200, 6, Some("px"))
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
        // Implementação CPU equivalente usando src.rgba32f_clamped(x + dx, y + dy) ...
        let _ = (radius, src, dst);
        Ok(())
    }
}

export_effect!(GaussianBlurPlugin);
```

---

## 4. Referência rápida da UI declarativa

| Controle (`UiBuilder`) | Tipo salvo em `Params` | Widget montado em `egui` pelo OpenPaintR |
| --- | --- | --- |
| `slider_f32(key, label, min, max, default, unit)` | `f32` | Slider contínuo com campo numérico e sufixo (ex.: `%`, `px`). |
| `slider_i32(key, label, min, max, default, unit)` | `i32` | Slider inteiro com ajuste fino. |
| `angle(key, label, default_deg)` | `f32` | Mostrador circular de ângulo + caixa em graus (`-180°` a `180°`). |
| `center_offset(key, label, default_x, default_y)` | `(f32, f32)` | Pad 2D normalizado (`-1.0..=1.0`) para centro de distorções/iluminação. |
| `color(key, label, default_rgba, allow_alpha)` | `[u8; 4]` | Botão de cor integrado ao tema + alfa opcional. |
| `checkbox(key, label, default)` | `bool` | Caixa de seleção padrão. |
| `select(key, label, default, options)` | `u32` | ComboBox suspenso estilo Windows. |
| `radio(key, label, default, options)` | `u32` | Grupo de opções exclusivas. |
| `seed(key, label, default)` | `u64` | Campo numérico + botão **Re-sortear**. |
| `section(title, ...)` / `visible_if(key, value)` | — | Cartão recolhível `styled_card` e exibição condicional dinâmica. |

---

## 5. Boas práticas de processamento

1. **Calcule o `halo_px` exato em `plan_render`:** nunca leia além de `[-halo, width + halo)` em `TileView`. Se o usuário escolher raio `12`, retorne `halo = 12`.
2. **Trate transparência sem halos escuros:** em convoluções, pondere o RGB pelo alfa (`rgb * a`) antes de acumular e divida pelo alfa resultante ao final, usando `PixelFormat::Rgba32Float`.
3. **Respeite o determinismo por tile:** efeitos aleatórios devem combinar `seed` com `(origin_x + x, origin_y + y)` (hash espacial), nunca um gerador sequencial dependente da ordem dos tiles.
4. **Mantenha paridade entre WGSL e Wasm:** o resultado de `render_tile` em CPU é a referência oficial para testes automatizados e para sistemas sem suporte `wgpu`.
5. **Preserve o contrato da UI:** não codifique cores fixas de texto/fundo nos textos de ajuda; o host aplica as cores dos temas **Moderno Pastel** e **Escuro Elegante** automaticamente.

---

## 6. Empacotamento, instalação e verificação

Compile o módulo WebAssembly em modo *release*:

```sh
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/openpaintr_efeito_ruido.wasm plugin.wasm
```

Empacote e valide com a CLI do OpenPaintR (prevista na entrega da Fase 4):

```sh
# Verifica manifesto, exports WIT e shaders WGSL via naga
openpaintr plugin check .

# Gera o pacote portátil meu-efeito.rpdn-plugin
openpaintr plugin pack . -o meu-efeito.rpdn-plugin

# Executa bateria headless (prévia, seleção SSAA, bordas alfa e paridade CPU/GPU)
openpaintr plugin test meu-efeito.rpdn-plugin
```

### Onde instalar pacotes `.rpdn-plugin`

Para testar no editor em qualquer sistema operacional, basta copiar o arquivo `.rpdn-plugin` para a pasta do usuário correspondente:

- **Linux:** `~/.local/share/openpaintr/plugins/`
- **Windows:** `%APPDATA%\OpenPaintR\plugins\`
- **macOS:** `~/Library/Application Support/OpenPaintR/plugins/`
