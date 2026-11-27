//! The tab and layer thumbnails of a document, redrawn only where it changed.

use eframe::egui::{self, TextureHandle, TextureOptions};
use openpaintr::{Document, Pixel, ThumbnailGrid};

use super::{editor::LayerChanges, widgets::pixels_to_color_image};

const TAB_SIDE: u32 = 24;
const LAYER_SIDE: u32 = 28;
/// Shortest interval between redraws while the pointer or a session is busy.
const BUSY_INTERVAL: f64 = 0.25;

#[derive(Default)]
pub struct Thumbnails {
    pub tab: Option<TextureHandle>,
    pub layers: Vec<Option<TextureHandle>>,
    /// The tab thumbnail's pixels, taken from the composites on display.
    samples: Vec<Pixel>,
    grid: Option<ThumbnailGrid>,
    tab_changed: bool,
    changed_layers: LayerChanges,
    drawn_at: f64,
}

impl Thumbnails {
    /// Takes the tab thumbnail's samples from a composite of the region from
    /// `min` to `max`, both included, of a `size` canvas.
    pub fn composited(
        &mut self,
        size: (u32, u32),
        min: (u32, u32),
        max: (u32, u32),
        pixels: &[Pixel],
    ) {
        let grid = ThumbnailGrid::new(size.0, size.1, TAB_SIDE);
        if self.grid != Some(grid) {
            self.grid = Some(grid);
            self.samples = vec![Pixel::default(); (grid.width * grid.height) as usize];
        }
        let stride = (max.0 - min.0 + 1) as usize;
        for (sample, (x, y)) in self.samples.iter_mut().zip(grid.positions()) {
            if (min.0..=max.0).contains(&x) && (min.1..=max.1).contains(&y) {
                *sample = pixels[(y - min.1) as usize * stride + (x - min.0) as usize];
            }
        }
        self.tab_changed = true;
    }

    #[cfg(test)]
    pub fn tab_pixels(&self) -> &[Pixel] {
        &self.samples
    }

    pub fn layers_changed(&mut self, changes: LayerChanges) {
        self.changed_layers.merge(changes);
    }

    /// Redraws what changed, immediately when idle and at intervals when busy.
    pub fn refresh(&mut self, ctx: &egui::Context, document: &Document, busy: bool) {
        if !self.tab_changed && self.changed_layers.is_empty() {
            return;
        }
        let now = ctx.input(|input| input.time);
        let wait = self.drawn_at + BUSY_INTERVAL - now;
        if busy && wait > 0.0 {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
            return;
        }
        self.drawn_at = now;
        if std::mem::take(&mut self.tab_changed)
            && let Some(grid) = self.grid
        {
            let size = (grid.width, grid.height);
            update(ctx, &mut self.tab, "tab_thumb", size, &self.samples);
        }
        let changes = std::mem::take(&mut self.changed_layers);
        let count = document.layer_count();
        self.layers.resize_with(count, || None);
        for (index, texture) in self.layers.iter_mut().enumerate() {
            if (changes.contains(index) || texture.is_none())
                && let Some((width, height, pixels)) = document.layer_thumbnail(index, LAYER_SIDE)
            {
                let name = format!("layer_thumb_{index}");
                update(ctx, texture, &name, (width, height), &pixels);
            }
        }
        // Panels drawn earlier in this frame show the new thumbnails next time.
        ctx.request_repaint();
    }
}

fn update(
    ctx: &egui::Context,
    texture: &mut Option<TextureHandle>,
    name: &str,
    (width, height): (u32, u32),
    pixels: &[Pixel],
) {
    let image = pixels_to_color_image(width as usize, height as usize, pixels);
    match texture {
        Some(texture) => texture.set(image, TextureOptions::LINEAR),
        None => *texture = Some(ctx.load_texture(name, image, TextureOptions::LINEAR)),
    }
}
