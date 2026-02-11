use ratatui::Frame;

use super::chunks::Chunks;
use super::state::InputMode;
use super::status_message::StatusMessage;
use super::widgets_collection::Widgets;
use ratatui::widgets::Widget;

use crate::config::LayoutStyle;

pub fn render(
    frame: &mut Frame,
    chunks: Chunks,
    widgets: &Widgets,
    input_mode: InputMode,
    status_message: Option<StatusMessage>,
    layout_style: &LayoutStyle,
) {
    widgets.background.render(frame);

    // Only render panel box in Boxed mode
    if matches!(layout_style, LayoutStyle::Boxed) {
        widgets.panel.render(frame, chunks.panel_root);
    }

    widgets.key_menu.render(frame, chunks.key_menu);
    if let Ok(battery) = widgets.battery.lock() {
        battery.render(frame.area(), frame.buffer_mut());
    }

    // Render Clock
    widgets.clock.render(chunks.clock, frame.buffer_mut());

    widgets.environment_guard().render(
        frame,
        chunks.switcher,
        matches!(input_mode, InputMode::Switcher),
    );
    widgets.username_guard().render(
        frame,
        chunks.username_field,
        matches!(input_mode, InputMode::Username),
    );
    widgets.password_guard().render(
        frame,
        chunks.password_field,
        matches!(input_mode, InputMode::Password),
    );

    // Display Status Message
    StatusMessage::render(status_message, frame, chunks.status_message);
}
