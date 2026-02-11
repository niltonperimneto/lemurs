use log::error;
use secrecy::{ExposeSecret, SecretString};
use std::sync::{Arc, Mutex, MutexGuard};

use super::background::BackgroundWidget;
use super::input_field::InputFieldWidget;
use super::key_menu::KeyMenuWidget;
use super::panel::PanelWidget;
use super::switcher::SwitcherWidget;
use crate::post_login::PostLoginEnvironment;

#[derive(Clone)]
pub struct Widgets {
    pub background: BackgroundWidget,
    pub panel: PanelWidget,
    pub key_menu: KeyMenuWidget,
    pub battery: std::sync::Arc<std::sync::Mutex<super::battery::BatteryWidget>>,
    pub clock: super::clock::ClockWidget,
    pub environment: Arc<Mutex<SwitcherWidget<PostLoginEnvironment>>>,
    pub username: Arc<Mutex<InputFieldWidget>>,
    pub password: Arc<Mutex<InputFieldWidget>>,
}

impl Widgets {
    pub fn environment_guard(&self) -> MutexGuard<'_, SwitcherWidget<PostLoginEnvironment>> {
        match self.environment.lock() {
            Ok(guard) => guard,
            Err(err) => {
                error!("Lock failed. Reason: {}", err);
                std::process::exit(1);
            }
        }
    }
    pub fn username_guard(&self) -> MutexGuard<'_, InputFieldWidget> {
        match self.username.lock() {
            Ok(guard) => guard,
            Err(err) => {
                error!("Lock failed. Reason: {}", err);
                std::process::exit(1);
            }
        }
    }
    pub fn password_guard(&self) -> MutexGuard<'_, InputFieldWidget> {
        match self.password.lock() {
            Ok(guard) => guard,
            Err(err) => {
                error!("Lock failed. Reason: {}", err);
                std::process::exit(1);
            }
        }
    }

    pub fn get_environment(&self) -> Option<(String, PostLoginEnvironment)> {
        self.environment_guard()
            .selected()
            .map(|s| (s.title.clone(), s.content.clone()))
    }
    pub fn environment_try_select(&self, title: &str) {
        self.environment_guard().try_select(title);
    }
    pub fn get_username(&self) -> String {
        self.username_guard().get_content().expose_secret().clone()
    }
    pub fn set_username(&self, content: &str) {
        self.username_guard().set_content(content)
    }
    pub fn get_password(&self) -> SecretString {
        self.password_guard().get_content()
    }
    pub fn clear_password(&self) {
        self.password_guard().clear()
    }
}
