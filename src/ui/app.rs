use log::{error, info, warn};
use ratatui::Terminal;
use std::io;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

use crate::config::{Config, FocusBehaviour, SwitcherVisibility};
use crate::info_caching::get_cached_information;

use super::background::BackgroundWidget;
use super::chunks::Chunks;
use super::input_field::{InputFieldDisplayType, InputFieldWidget};
use super::input_loop;
use super::key_menu::KeyMenuWidget;
use super::panel::PanelWidget;
use super::renderer;
use super::state::{InputMode, LoginFormInputMode, LoginFormStatusMessage};
use super::switcher::{SwitcherItem, SwitcherWidget};
use super::types::{LoginAction, LoginBackend, UIThreadRequest};
use super::widgets_collection::Widgets;

/// App holds the state of the application
#[derive(Clone)]
pub struct LoginForm {
    /// Whether the application is running in preview mode
    preview: bool,

    widgets: Widgets,

    /// The configuration for the app
    config: Arc<Config>,
}

impl LoginForm {
    // Note: set_cache logic logic is duplicated/inline in input_loop.rs to avoid circular issues
    // or passing "self" into the thread.
    // However, load_cache is called before the thread starts.

    fn load_cache(&self) {
        let env_remember = self.config.environment_switcher.remember;
        let username_remember = self.config.username_field.remember;

        let cached = get_cached_information(&self.config);

        if username_remember {
            if let Some(username) = cached.username() {
                info!("Loading username '{}' from cache", username);
                self.widgets.set_username(username);
            }
        }
        if env_remember {
            if let Some(env) = cached.environment() {
                info!("Loading environment '{}' from cache", env);
                self.widgets.environment_try_select(env);
            }
        }
    }

    pub fn new(config: Arc<Config>, preview: bool) -> LoginForm {
        LoginForm {
            preview,
            widgets: Widgets {
                background: BackgroundWidget::new(config.background.clone()),
                panel: PanelWidget::new(config.panel.clone()),
                key_menu: KeyMenuWidget::new(
                    config.power_controls.clone(),
                    config.environment_switcher.clone(),
                    config.system_shell.clone(),
                ),
                battery: Arc::new(Mutex::new(super::battery::BatteryWidget::new())),
                clock: super::clock::ClockWidget::new(config.design.show_clock),
                environment: Arc::new(Mutex::new(SwitcherWidget::new(
                    crate::post_login::get_envs(&config)
                        .into_iter()
                        .map(|(title, content)| SwitcherItem::new(title, content))
                        .collect(),
                    config.environment_switcher.clone(),
                ))),
                username: Arc::new(Mutex::new(InputFieldWidget::new(
                    InputFieldDisplayType::Echo,
                    config.username_field.style.clone(),
                    String::default(),
                ))),
                password: Arc::new(Mutex::new(InputFieldWidget::new(
                    InputFieldDisplayType::Replace(
                        config
                            .password_field
                            .content_replacement_character
                            .to_string(),
                    ),
                    config.password_field.style.clone(),
                    String::default(),
                ))),
            },
            config,
        }
    }

    pub fn run<B: LoginBackend + 'static>(
        self,
        terminal: &mut Terminal<B>,
        pam_service: String,
    ) -> io::Result<LoginAction> {
        terminal.backend_mut().enable_ui()?;
        self.load_cache();

        // Determine initial input mode
        let input_mode = LoginFormInputMode::new(match self.config.focus_behaviour {
            FocusBehaviour::FirstNonCached => match (
                self.config.username_field.remember && !self.widgets.get_username().is_empty(),
                self.config.environment_switcher.remember
                    && self
                        .widgets
                        .get_environment()
                        .map(|(title, _)| !title.is_empty())
                        .unwrap_or(false),
            ) {
                (true, true) => InputMode::Password,
                (true, _) => InputMode::Username,
                _ => {
                    if self.config.environment_switcher.switcher_visibility
                        == SwitcherVisibility::Visible
                    {
                        InputMode::Switcher
                    } else {
                        InputMode::Username
                    }
                }
            },
            FocusBehaviour::NoFocus => InputMode::Normal,
            FocusBehaviour::Environment => InputMode::Switcher,
            FocusBehaviour::Username => InputMode::Username,
            FocusBehaviour::Password => InputMode::Password,
        });

        let status_message = LoginFormStatusMessage::new();

        // Initial draw
        let draw_action = terminal.draw(|f| {
            let layout = Chunks::new(f, &self.config);
            renderer::render(
                f,
                layout,
                &self.widgets,
                input_mode.get(),
                status_message.get(),
                &self.config.design.layout,
            );
        });

        if let Err(err) = draw_action {
            error!("Failed to draw. Reason: {}", err);
            std::process::exit(1);
        }

        let (req_send_channel, req_recv_channel) = channel();

        // Clone state for the input thread
        let event_input_mode = input_mode.clone();
        let event_status_message = status_message.clone();
        let widgets = self.widgets.clone();
        let config = self.config.clone();
        let preview = self.preview;

        // Spawn Input Loop Thread
        std::thread::spawn(move || {
            input_loop::run(
                widgets,
                config,
                pam_service,
                preview,
                event_input_mode,
                event_status_message,
                req_send_channel,
            );
        });

        // Start the UI / Draw Loop (Main Thread)
        // This blocks until we actually call StopDrawing or receive action
        while let Ok(request) = req_recv_channel.recv() {
            match request {
                UIThreadRequest::Redraw => {
                    let inputs_widgets = &self.widgets;
                    let draw_action = terminal.draw(|f| {
                        let layout = Chunks::new(f, &self.config);
                        renderer::render(
                            f,
                            layout,
                            inputs_widgets,
                            input_mode.get(),
                            status_message.get(),
                            &self.config.design.layout,
                        );
                    });

                    if let Err(err) = draw_action {
                        warn!("Failed to draw to screen. Reason: {err}");
                    }
                }
                UIThreadRequest::DisableTui => {
                    terminal.backend_mut().disable_ui()?;
                }
                UIThreadRequest::LoginSuccess(info, env) => {
                    return Ok(LoginAction::Launch(info, env));
                }
                _ => break,
            }
        }

        Ok(LoginAction::None)
    }
}
