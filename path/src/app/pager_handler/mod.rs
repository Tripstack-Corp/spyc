// src/app/pager_handler/mod.rs
//! Pager handler module.

use std::collections::HashMap;

use crate::pager_handler::image::Image;
use crate::pager_handler::modes::Modes;
use crate::render::overlays::Overlays;

pub struct PagerHandler {
    image: Image,
    modes: Modes,
    overlays: Overlays,
}

impl PagerHandler {
    pub fn new() -> Self {
        Self {
            image: Image::new(),
            modes: Modes::new(),
            overlays: Overlays::new(),
        }
    }

    pub fn render(&mut self, buffer: &str) -> String {
        let mut output = String::new();

        // Reserve a status line at the bottom of the view
        let status_line = self.overlays.render_status_line(buffer);
        output.push_str(&status_line);

        // Render the image
        let image = self.image.render(buffer);
        output.push_str(&image);

        // Render the modes
        let modes = self.modes.render(buffer);
        output.push_str(&modes);

        output
    }
}

pub fn create() -> PagerHandler {
    PagerHandler::new()
}