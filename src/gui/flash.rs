//! A short flash that draws attention to a new error in the status bar.

use std::time::Duration;

use eframe::egui;

/// Duration of each inverted or normal step.
const STEP: f64 = 0.15;
/// Inverted, normal, inverted: the message flashes twice.
const STEPS: u32 = 3;

#[derive(Default)]
pub struct Flash {
    drawn: bool,
    since: Option<f64>,
}

impl Flash {
    /// Whether to draw the message inverted in this frame, or `None` once the
    /// flash is over.
    pub fn inverted(&mut self, ctx: &egui::Context) -> Option<bool> {
        // The frame that raised the error may have waited on a file dialog, so
        // its time is stale: the countdown starts with the following frame.
        if !self.drawn {
            self.drawn = true;
            ctx.request_repaint();
            return Some(true);
        }
        let now = ctx.input(|input| input.time);
        let elapsed = now - *self.since.get_or_insert(now);
        let step = (elapsed / STEP) as u32;
        if step >= STEPS {
            return None;
        }
        let next = f64::from(step + 1) * STEP - elapsed;
        ctx.request_repaint_after(Duration::from_secs_f64(next));
        Some(step.is_multiple_of(2))
    }
}
