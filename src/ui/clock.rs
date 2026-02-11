use chrono::Local;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

#[derive(Clone)]
pub struct ClockWidget {
    pub show: bool,
}

impl ClockWidget {
    pub fn new(show: bool) -> Self {
        Self { show }
    }
}

// Simple 5x3 font for digits 0-9 and :
// Using full blocks █
const DIGITS: [&[&str]; 11] = [
    // 0
    &["███", "█ █", "█ █", "█ █", "███"],
    // 1
    &["  █", "  █", "  █", "  █", "  █"],
    // 2
    &["███", "  █", "███", "█  ", "███"],
    // 3
    &["███", "  █", "███", "  █", "███"],
    // 4
    &["█ █", "█ █", "███", "  █", "  █"],
    // 5
    &["███", "█  ", "███", "  █", "███"],
    // 6
    &["███", "█  ", "███", "█ █", "███"],
    // 7
    &["███", "  █", "  █", "  █", "  █"],
    // 8
    &["███", "█ █", "███", "█ █", "███"],
    // 9
    &["███", "█ █", "███", "  █", "███"],
    // :
    &["   ", " █ ", "   ", " █ ", "   "],
];

impl Widget for &ClockWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if !self.show {
            return;
        }

        let now = Local::now();
        let time_str = now.format("%H:%M").to_string();

        let char_height = 5;
        let char_width = 3;
        let spacing = 1;

        let total_width = (char_width + spacing) * time_str.len() as u16;

        // Center horizontally in the given area
        if area.width < total_width || area.height < char_height {
            return;
        }

        let start_x = area.x + (area.width - total_width) / 2;
        let start_y = area.y + (area.height - char_height) / 2;

        let style = Style::default().fg(Color::White); // TODO: Configurable color

        for (i, c) in time_str.chars().enumerate() {
            let digit_idx = match c {
                '0'..='9' => c.to_digit(10).unwrap() as usize,
                ':' => 10,
                _ => continue,
            };

            let x_offset = start_x + (i as u16 * (char_width + spacing));

            for (dy, line) in DIGITS[digit_idx].iter().enumerate() {
                let y = start_y + dy as u16;

                for (dx, pixel_char) in line.chars().enumerate() {
                    if pixel_char != ' ' {
                        if let Some(cell) = buf.cell_mut((x_offset + dx as u16, y)) {
                            cell.set_symbol("█");
                            cell.set_style(style);
                        }
                    }
                }
            }
        }
    }
}
