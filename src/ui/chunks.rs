use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    Frame,
};
use Constraint::{Length, Min};

use crate::config::{Config, LayoutStyle, PanelPosition};

pub struct Chunks {
    pub key_menu: Rect,
    pub panel_root: Rect,
    pub switcher: Rect,
    pub username_field: Rect,
    pub password_field: Rect,
    pub status_message: Rect,
    pub clock: Rect,
}

impl Chunks {
    pub fn new(frame: &Frame, config: &Config) -> Self {
        let area = frame.area();

        // Main Vertical Layout: KeyMenu at top or bottom? Default logic.
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Length(1), // Key Menu
                Min(0),    // Middle Content
                Length(1), // Status Message
            ])
            .horizontal_margin(2)
            .vertical_margin(1)
            .split(area);

        let key_menu = main_chunks[0];
        let middle_content = main_chunks[1];
        let status_message = main_chunks[2];

        // Layout Logic Branch
        match config.design.layout {
            LayoutStyle::Boxed => Self::boxed(middle_content, config, key_menu, status_message),
            LayoutStyle::Minimal => Self::minimal(middle_content, config, key_menu, status_message),
        }
    }

    fn boxed(area: Rect, config: &Config, key_menu: Rect, status_message: Rect) -> Self {
        // Panel dimensions
        let panel_width = 50;
        let panel_height = 13;
        let position = &config.panel.position;

        // Alignment Logic (Same as before)
        let (v_constraints, h_constraints) = match position {
            PanelPosition::Center => (
                [Min(0), Length(panel_height), Min(0)],
                [Min(0), Length(panel_width), Min(0)],
            ),
            PanelPosition::TopLeft => (
                [Length(panel_height), Min(0), Length(0)],
                [Length(panel_width), Min(0), Length(0)],
            ),
            PanelPosition::TopCenter => (
                [Length(panel_height), Min(0), Length(0)],
                [Min(0), Length(panel_width), Min(0)],
            ),
            PanelPosition::TopRight => (
                [Length(panel_height), Min(0), Length(0)],
                [Length(0), Min(0), Length(panel_width)],
            ),
            PanelPosition::CenterLeft => (
                [Min(0), Length(panel_height), Min(0)],
                [Length(panel_width), Min(0), Length(0)],
            ),
            PanelPosition::CenterRight => (
                [Min(0), Length(panel_height), Min(0)],
                [Length(0), Min(0), Length(panel_width)],
            ),
            PanelPosition::BottomLeft => (
                [Length(0), Min(0), Length(panel_height)],
                [Length(panel_width), Min(0), Length(0)],
            ),
            PanelPosition::BottomCenter => (
                [Length(0), Min(0), Length(panel_height)],
                [Min(0), Length(panel_width), Min(0)],
            ),
            PanelPosition::BottomRight => (
                [Length(0), Min(0), Length(panel_height)],
                [Length(0), Min(0), Length(panel_width)],
            ),
        };

        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(v_constraints)
            .split(area);

        let v_idx = match position {
            PanelPosition::Center | PanelPosition::CenterLeft | PanelPosition::CenterRight => 1,
            PanelPosition::TopLeft | PanelPosition::TopCenter | PanelPosition::TopRight => 0,
            PanelPosition::BottomLeft
            | PanelPosition::BottomCenter
            | PanelPosition::BottomRight => 2,
        };

        let row_for_panel = vertical_chunks[v_idx];

        let horizontal_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(h_constraints)
            .split(row_for_panel);

        let h_idx = match position {
            PanelPosition::Center | PanelPosition::TopCenter | PanelPosition::BottomCenter => 1,
            PanelPosition::TopLeft | PanelPosition::CenterLeft | PanelPosition::BottomLeft => 0,
            PanelPosition::TopRight | PanelPosition::CenterRight | PanelPosition::BottomRight => 2,
        };

        let panel_root = horizontal_chunks[h_idx];

        // Panel Layout: Inside the box
        let panel_chunks = Layout::default()
            .direction(Direction::Vertical)
            .horizontal_margin(1)
            .vertical_margin(1)
            .constraints([
                Length(3), // Switcher
                Length(1), // Spacer
                Length(3), // Username
                Length(1), // Spacer
                Length(3), // Password
                Min(0),
            ])
            .split(panel_root);

        // Calculate Clock Position (e.g. above panel or top center of screen area?)
        // For Boxed, let's put it above the panel if center, or just top center of screen.
        // Let's reserve a space above the panel logic for consistency?
        // Or just float it at Top-Center of the whole screen area.
        let clock_area = Rect::new(area.x, area.y + 2, area.width, 5); // Rough top placement

        Self {
            key_menu,
            status_message,
            panel_root,
            switcher: panel_chunks[0],
            username_field: panel_chunks[2],
            password_field: panel_chunks[4],
            clock: clock_area,
        }
    }

    fn minimal(area: Rect, _config: &Config, key_menu: Rect, status_message: Rect) -> Self {
        // Minimal Layout:
        // Top 1/3: Clock
        // Middle: Gap
        // Bottom 1/3 (or lower middle): Inputs

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Min(10),    // Top space (Clock)
                Length(13), // Input Block (Height of current panel logic ~13)
                Min(10),    // Bottom space
            ])
            .split(area);

        let clock = chunks[0];
        let input_area = chunks[1];

        // Center the input block horizontally
        let center_h = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Min(0),
                Length(50), // Standard width
                Min(0),
            ])
            .split(input_area);

        let panel_root = center_h[1];

        // Same internal constraints as boxed, but "panel_root" is invisible (no border logic handled here, handled in renderer/panel)
        let panel_chunks = Layout::default()
            .direction(Direction::Vertical)
            // No margins if we want it super clean, or keep them for spacing
            .constraints([
                Length(3), // Switcher
                Length(1), // Spacer
                Length(3), // Username
                Length(1), // Spacer
                Length(3), // Password
                Min(0),
            ])
            .split(panel_root);

        Self {
            key_menu,
            status_message,
            panel_root,
            switcher: panel_chunks[0],
            username_field: panel_chunks[2],
            password_field: panel_chunks[4],
            clock,
        }
    }
}
