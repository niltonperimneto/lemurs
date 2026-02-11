use log::error;
use std::sync::{Arc, Mutex, MutexGuard};

use super::status_message::StatusMessage;

/// All the different modes for input
#[derive(Clone, Copy)]
pub enum InputMode {
    /// Using the env switcher widget
    Switcher,

    /// Typing within the Username input field
    Username,

    /// Typing within the Password input field
    Password,

    /// Nothing selected
    Normal,
}

impl InputMode {
    /// Move to the next mode
    pub fn next(&mut self, skip_switcher: bool) {
        use InputMode::*;

        *self = match self {
            Normal => {
                if skip_switcher {
                    Username
                } else {
                    Switcher
                }
            }
            Switcher => Username,
            Username => Password,
            Password => Password,
        }
    }

    /// Move to the previous mode
    pub fn prev(&mut self, skip_switcher: bool) {
        use InputMode::*;

        *self = match self {
            Normal => Normal,
            Switcher => Normal,
            Username => {
                if skip_switcher {
                    Normal
                } else {
                    Switcher
                }
            }
            Password => Username,
        }
    }
}

#[derive(Clone)]
pub struct LoginFormInputMode(Arc<Mutex<InputMode>>);

impl LoginFormInputMode {
    pub fn new(mode: InputMode) -> Self {
        Self(Arc::new(Mutex::new(mode)))
    }

    pub fn get_guard(&self) -> MutexGuard<'_, InputMode> {
        let Self(mutex) = self;

        match mutex.lock() {
            Ok(guard) => guard,
            Err(err) => {
                error!("Lock failed. Reason: {}", err);
                std::process::exit(1);
            }
        }
    }

    pub fn get(&self) -> InputMode {
        *self.get_guard()
    }

    pub fn prev(&self, skip_switcher: bool) {
        self.get_guard().prev(skip_switcher)
    }
    pub fn next(&self, skip_switcher: bool) {
        self.get_guard().next(skip_switcher)
    }
    pub fn set(&self, mode: InputMode) {
        *self.get_guard() = mode;
    }
}

#[derive(Clone)]
pub struct LoginFormStatusMessage(Arc<Mutex<Option<StatusMessage>>>);

impl LoginFormStatusMessage {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }

    pub fn get_guard(&self) -> MutexGuard<'_, Option<StatusMessage>> {
        let Self(mutex) = self;

        match mutex.lock() {
            Ok(guard) => guard,
            Err(err) => {
                error!("Lock failed. Reason: {}", err);
                std::process::exit(1);
            }
        }
    }

    pub fn get(&self) -> Option<StatusMessage> {
        self.get_guard().clone()
    }

    pub fn clear(&self) {
        *self.get_guard() = None;
    }
    pub fn set(&self, msg: impl Into<StatusMessage>) {
        *self.get_guard() = Some(msg.into());
    }
}
