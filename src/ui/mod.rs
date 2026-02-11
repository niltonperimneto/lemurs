// Modules
pub mod background;
pub mod battery;
pub mod chunks;
pub mod clock;
pub mod input_field;
pub mod key_menu;
pub mod panel;
pub mod status_message;
pub mod switcher;

// New modularized structure
pub mod app;
pub mod input_loop;
pub mod renderer;

pub mod state;
pub mod types;
pub mod widgets_collection;

// Expose key types that Main uses
pub use app::LoginForm;
pub use types::{LoginAction, LoginBackend};
