use std::path::Path;

use image::{ImageBuffer, Rgba};
use log::warn;

use crate::ui::events::{Rectangle, RectangleExt};
use crate::ui::renderer::{DrawColor, Renderer};

pub type Icon = ImageBuffer<Rgba<u8>, Vec<u8>>;

/// Footprint of an icon button — one chess cell (1072 / 8).
pub const ICON_BUTTON_SIZE: u16 = 134;
/// Default size the icon glyph is drawn at, centred inside the footprint.
const ICON_GLYPH_SIZE: u16 = 100;

/// Load a PNG icon from the assets directory, returning `None` (and logging) on
/// failure so a missing asset degrades gracefully instead of crashing.
pub fn load_icon(name: &str) -> Option<Icon> {
    let path = format!("{}{}", env!("ASSETS_DIR"), name);
    match image::open(Path::new(&path)) {
        Ok(img) => Some(img.to_rgba8()),
        Err(e) => {
            warn!("Failed to load icon {}: {}", path, e);
            None
        }
    }
}

/// A borderless, label-less button: just an icon glyph, with a chess-cell-sized
/// touch footprint. Used for the back-nav and settings buttons in the sidebars.
pub struct IconButton {
    pub rect: Rectangle,
    icon: Option<Icon>,
    glyph_size: u16,
}

impl IconButton {
    pub fn new(x: i16, y: i16, asset_name: &str) -> Self {
        Self {
            rect: Rectangle::new(x, y, ICON_BUTTON_SIZE, ICON_BUTTON_SIZE),
            icon: load_icon(asset_name),
            glyph_size: ICON_GLYPH_SIZE,
        }
    }

    /// Builder override for the drawn glyph size (the touch footprint stays a
    /// full cell). Used to render the settings icon slightly smaller than the
    /// back-nav icon.
    pub fn with_glyph_size(mut self, size: u16) -> Self {
        self.glyph_size = size;
        self
    }

    pub fn contains(&self, px: i16, py: i16) -> bool {
        self.rect.contains(px, py)
    }

    /// Draw just the centred icon — no outline, no fill, no label.
    pub fn draw(&self, renderer: &mut Renderer) -> Result<(), Box<dyn std::error::Error>> {
        let Some(icon) = self.icon.as_ref() else {
            return Ok(());
        };
        let offset = (ICON_BUTTON_SIZE as i16 - self.glyph_size as i16) / 2;
        renderer.draw_image_alpha(
            self.rect.x + offset,
            self.rect.y + offset,
            self.glyph_size,
            self.glyph_size,
            icon,
            DrawColor::White,
        )?;
        Ok(())
    }
}
