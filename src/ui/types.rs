use std::io;

use crossterm::cursor::MoveTo;
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::{Backend, CrosstermBackend};

use crate::auth::AuthUserInfo;
use crate::post_login::PostLoginEnvironment;

/// Trait for backends that support enabling/disabling the UI (entering/leaving raw mode/alternate screen)
pub trait LoginBackend: Backend {
    fn enable_ui(&mut self) -> io::Result<()>;
    fn disable_ui(&mut self) -> io::Result<()>;
}

impl<W: io::Write> LoginBackend for CrosstermBackend<W> {
    fn enable_ui(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        execute!(self, EnterAlternateScreen, crossterm::cursor::Hide)?;
        Ok(())
    }

    fn disable_ui(&mut self) -> io::Result<()> {
        disable_raw_mode()?;
        execute!(
            self,
            LeaveAlternateScreen,
            Clear(ClearType::All),
            MoveTo(0, 0),
            crossterm::cursor::Show
        )?;
        Ok(())
    }
}

pub enum LoginAction {
    None,
    Launch(Box<AuthUserInfo>, PostLoginEnvironment),
}

pub enum UIThreadRequest {
    Redraw,
    DisableTui,
    StopDrawing,
    LoginSuccess(Box<AuthUserInfo>, PostLoginEnvironment),
}
