use crate::gui::kms::KmsBackend;
use crate::ui::LoginBackend;
use ratatui::backend::Backend;
use ratatui::buffer::Cell;

use std::io;
use std::fs;
use rusttype::{Font, Scale, Point, PositionedGlyph};
use crossterm::terminal::{enable_raw_mode, disable_raw_mode};

pub struct KmsRatatuiBackend {
    kms: KmsBackend,
    cursor_pos: Option<(u16, u16)>,
    font: Font<'static>,
    scale: Scale,
    char_width: u32,
    char_height: u32,
}

impl KmsRatatuiBackend {
    pub fn new(kms: KmsBackend, config: &crate::config::Config) -> Self {
        // Load font with fallback strategy
        let mut font_data = Vec::new();
        let mut loaded_path = String::new();

        // 1. Try configured path
        if let Ok(data) = fs::read(&config.font_path) {
            font_data = data;
            loaded_path = config.font_path.clone();
        } else {
            eprintln!("Warning: Failed to load configured font at '{}'", config.font_path);
            
            // 2. Try common system fonts
            let fallbacks = [
                "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
                "/usr/share/fonts/truetype/freefont/FreeMono.ttf",
                "/usr/share/fonts/liberation/LiberationMono-Regular.ttf",
                "/usr/share/fonts/gnu-free/FreeMono.ttf",
                "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
            ];

            for path in fallbacks {
                if let Ok(data) = fs::read(path) {
                    font_data = data;
                    loaded_path = path.to_string();
                    eprintln!("Fallback: Loaded font from '{}'", path);
                    break;
                }
            }
        }

        let font = if !font_data.is_empty() {
             Font::try_from_vec(font_data).expect("Error parsing font data")
        } else {
            eprintln!("CRITICAL: No usable font found! Please install DejaVu Sans Mono or configure a valid font in config.toml.");
            panic!("No font found."); 
        };
        
        // Define font size from config
        let scale = Scale::uniform(config.font_size as f32);
        
        // Calculate metrics for a utility character to determine cell size
        let v_metrics = font.v_metrics(scale);
        let glyph = font.glyph('M').scaled(scale);
        let h_metrics = glyph.h_metrics();
        
        let char_width = h_metrics.advance_width.ceil() as u32;
        let char_height = (v_metrics.ascent - v_metrics.descent + v_metrics.line_gap).ceil() as u32;

        Self {
            kms,
            cursor_pos: None,
            font,
            scale,
            char_width,
            char_height,
        }
    }

    fn color_to_rgb(color: ratatui::style::Color) -> u32 {
        match color {
            ratatui::style::Color::Reset => 0x00000000,
            ratatui::style::Color::Black => 0x00000000,
            ratatui::style::Color::Red => 0x00FF0000,
            ratatui::style::Color::Green => 0x0000FF00,
            ratatui::style::Color::Yellow => 0x00FFFF00,
            ratatui::style::Color::Blue => 0x000000FF,
            ratatui::style::Color::Magenta => 0x00FF00FF,
            ratatui::style::Color::Cyan => 0x0000FFFF,
            ratatui::style::Color::Gray => 0x00808080,
            ratatui::style::Color::DarkGray => 0x00404040,
            ratatui::style::Color::LightRed => 0x00FF8080,
            ratatui::style::Color::LightGreen => 0x0080FF80,
            ratatui::style::Color::LightYellow => 0x00FFFF80,
            ratatui::style::Color::LightBlue => 0x008080FF,
            ratatui::style::Color::LightMagenta => 0x00FF80FF,
            ratatui::style::Color::LightCyan => 0x0080FFFF,
            ratatui::style::Color::White => 0x00FFFFFF,
            ratatui::style::Color::Rgb(r, g, b) => ((r as u32) << 16) | ((g as u32) << 8) | (b as u32),
            ratatui::style::Color::Indexed(_) => 0x00FFFFFF, // Fallback
        }
    }

    fn set_cursor_state(&mut self, x: u16, y: u16) {
        self.cursor_pos = Some((x, y));
        let char_width = self.char_width;
        let char_height = self.char_height;
        let px = x as i32 * char_width as i32;
        let py = y as i32 * char_height as i32;
        
        // Draw a simple cursor block (white) at the bottom
        for dy in (char_height - 4)..char_height {
             for dx in 0..char_width {
                 self.kms.set_pixel((px + dx as i32) as u32, (py + dy as i32) as u32, 0x00FFFFFF);
             }
        }
    }
}

impl Backend for KmsRatatuiBackend {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            // Calculate pixel position
            let px = x as i32 * self.char_width as i32;
            let py = y as i32 * self.char_height as i32;
            
            // 1. Draw Background
            let bg_color = Self::color_to_rgb(cell.bg);
            for dy in 0..self.char_height {
                for dx in 0..self.char_width {
                    self.kms.set_pixel((px + dx as i32) as u32, (py + dy as i32) as u32, bg_color);
                }
            }

            // 2. Draw Foreground (Text) with RustType
            let fg_color = Self::color_to_rgb(cell.fg);
            let content_str = cell.symbol();
            
            if content_str.is_empty() || content_str == " " {
                continue;
            }

            // Simple blending: alpha composition against bg_color
            let v_metrics = self.font.v_metrics(self.scale);
            let offset = point(px as f32, py as f32 + v_metrics.ascent); // Baseline

            for glyph in self.font.layout(content_str, self.scale, offset) {
                if let Some(bb) = glyph.pixel_bounding_box() {
                    glyph.draw(|gx, gy, v| {
                         let screen_x = gx as i32 + bb.min.x;
                         let screen_y = gy as i32 + bb.min.y;
                         
                         let alpha = v;
                         if alpha < 0.1 { return; } // Optimization

                         // Decompose colors
                         let bg_r = (bg_color >> 16) & 0xFF;
                         let bg_g = (bg_color >> 8) & 0xFF;
                         let bg_b = bg_color & 0xFF;

                         let fg_r = (fg_color >> 16) & 0xFF;
                         let fg_g = (fg_color >> 8) & 0xFF;
                         let fg_b = fg_color & 0xFF;

                         let out_r = ((fg_r as f32 * alpha) + (bg_r as f32 * (1.0 - alpha))) as u32;
                         let out_g = ((fg_g as f32 * alpha) + (bg_g as f32 * (1.0 - alpha))) as u32;
                         let out_b = ((fg_b as f32 * alpha) + (bg_b as f32 * (1.0 - alpha))) as u32;

                         let out_color = (out_r << 16) | (out_g << 8) | out_b;
                         
                         self.kms.set_pixel(screen_x as u32, screen_y as u32, out_color);
                    });
                }
            }
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.cursor_pos = None;
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn get_cursor(&mut self) -> io::Result<(u16, u16)> {
        Ok(self.cursor_pos.unwrap_or((0, 0)))
    }

    fn set_cursor(&mut self, x: u16, y: u16) -> io::Result<()> {
        self.set_cursor_state(x, y);
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<ratatui::layout::Position, io::Error> {
        let (x, y) = self.cursor_pos.unwrap_or((0, 0));
        Ok(ratatui::layout::Position { x, y })
    }

    fn set_cursor_position<P: Into<ratatui::layout::Position>>(&mut self, position: P) -> Result<(), io::Error> {
        let p = position.into();
        self.set_cursor_state(p.x, p.y);
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        self.kms.fill_screen(0x00000000); // Black
        Ok(())
    }

    fn size(&self) -> io::Result<ratatui::layout::Size> {
        let cols = self.kms.width() / self.char_width;
        let rows = self.kms.height() / self.char_height;
        Ok(ratatui::layout::Size { width: cols as u16, height: rows as u16 })
    }

    fn window_size(&mut self) -> Result<ratatui::backend::WindowSize, io::Error> {
        let cols = self.kms.width() / self.char_width;
        let rows = self.kms.height() / self.char_height;
        let s = ratatui::layout::Size { width: cols as u16, height: rows as u16 };
        
        Ok(ratatui::backend::WindowSize {
            columns_rows: s,
            pixels: ratatui::layout::Size {
                width: self.kms.width() as u16,
                height: self.kms.height() as u16,
            },
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.kms.flush();
        Ok(())
    }
}

impl LoginBackend for KmsRatatuiBackend {
    fn enable_ui(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        Ok(())
    }
    fn disable_ui(&mut self) -> io::Result<()> {
        disable_raw_mode()?;
        self.kms.fill_screen(0);
        Ok(())
    }
}

// Helper for rusttype point
fn point(x: f32, y: f32) -> Point<f32> {
    Point { x, y }
}
