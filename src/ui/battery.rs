use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub enum BatteryStatus {
    Charging,
    Discharging,
    Full,
    Unknown,
}

#[derive(Clone)]
pub struct BatteryWidget {
    pub capacity: u8,
    pub status: BatteryStatus,
    pub exists: bool,
}

impl BatteryWidget {
    pub fn new() -> Self {
        let mut widget = Self {
            capacity: 0,
            status: BatteryStatus::Unknown,
            exists: false,
        };
        widget.update();
        widget
    }

    pub fn update(&mut self) {
        // Simple heuristic: find first BAT entry
        let power_supply_dir = Path::new("/sys/class/power_supply");
        if let Ok(entries) = fs::read_dir(power_supply_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                if name.starts_with("BAT") || name.starts_with("cw2015-bat") {
                    self.exists = true;

                    // Read Capacity
                    if let Ok(cap_str) = fs::read_to_string(path.join("capacity")) {
                        self.capacity = cap_str.trim().parse().unwrap_or(0).min(100);
                    }

                    // Read Status
                    if let Ok(stat_str) = fs::read_to_string(path.join("status")) {
                        self.status = match stat_str.trim() {
                            "Charging" => BatteryStatus::Charging,
                            "Discharging" => BatteryStatus::Discharging,
                            "Full" => BatteryStatus::Full,
                            _ => BatteryStatus::Unknown,
                        };
                    }
                    return; // Found a battery, stop looking
                }
            }
        }
        self.exists = false;
    }
}

impl Widget for &BatteryWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.exists {
            return;
        }

        // Symbols (Nerd Font / FontAwesome)
        // Charging:  (Bolt)
        // Discharging:     

        let symbol = match self.status {
            BatteryStatus::Charging => " ",
            _ => match self.capacity {
                90..=100 => " ",
                60..=89 => " ",
                40..=59 => " ",
                10..=39 => " ",
                0..=9 => " ",
                _ => " ",
            },
        };

        let color = match (self.status, self.capacity) {
            (BatteryStatus::Charging, _) => Color::Yellow,
            (_, 0..=20) => Color::Red,
            (_, _) => Color::White,
        };

        let text = format!("{}{}%", symbol, self.capacity);

        // Render top-right of the area provided.
        // Assuming area is the full screen or top bar.
        // We'll just render at 0,0 of the provided area, expecting the caller to position the area correctly.
        // BUT, renderer.rs usually passes the full screen or specific chunks.
        // Let's assume we render 'text'

        // Ideally we want right-aligned.
        // width of text
        let text_len = text.chars().count() as u16;
        if area.width < text_len {
            return;
        }

        let x = area.x + area.width - text_len;
        let y = area.y;

        buf.set_string(x, y, text, Style::default().fg(color));
    }
}
