use log::{error, info, warn};

use std::io;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::config::{Config, FocusBehaviour, SwitcherVisibility};
use crate::info_caching::{get_cached_information, set_cache};
use crate::post_login::PostLoginEnvironment;
use crate::{auth::try_auth, auth::AuthUserInfo, auth::AuthenticationError};
use status_message::StatusMessage;

use crossterm::cursor::MoveTo;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::{Frame, Terminal};

mod background;
mod chunks;
mod input_field;
mod key_menu;
mod panel;
mod status_message;
mod switcher;

use chunks::Chunks;
use input_field::{InputFieldDisplayType, InputFieldWidget};
use key_menu::KeyMenuWidget;
use status_message::{ErrorStatusMessage, InfoStatusMessage};
use switcher::{SwitcherItem, SwitcherWidget};

use self::background::BackgroundWidget;
use self::panel::PanelWidget;

#[derive(Clone)]
struct LoginFormInputMode(Arc<Mutex<InputMode>>);

impl LoginFormInputMode {
    fn new(mode: InputMode) -> Self {
        Self(Arc::new(Mutex::new(mode)))
    }

    fn get_guard(&self) -> MutexGuard<'_, InputMode> {
        let Self(mutex) = self;

        match mutex.lock() {
            Ok(guard) => guard,
            Err(err) => {
                error!("Lock failed. Reason: {}", err);
                std::process::exit(1);
            }
        }
    }

    fn get(&self) -> InputMode {
        *self.get_guard()
    }

    fn prev(&self, skip_switcher: bool) {
        self.get_guard().prev(skip_switcher)
    }
    fn next(&self, skip_switcher: bool) {
        self.get_guard().next(skip_switcher)
    }
    fn set(&self, mode: InputMode) {
        *self.get_guard() = mode;
    }
}

#[derive(Clone)]
struct LoginFormStatusMessage(Arc<Mutex<Option<StatusMessage>>>);

impl LoginFormStatusMessage {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }

    fn get_guard(&self) -> MutexGuard<'_, Option<StatusMessage>> {
        let Self(mutex) = self;

        match mutex.lock() {
            Ok(guard) => guard,
            Err(err) => {
                error!("Lock failed. Reason: {}", err);
                std::process::exit(1);
            }
        }
    }

    fn get(&self) -> Option<StatusMessage> {
        self.get_guard().clone()
    }

    fn clear(&self) {
        *self.get_guard() = None;
    }
    fn set(&self, msg: impl Into<StatusMessage>) {
        *self.get_guard() = Some(msg.into());
    }
}

/// All the different modes for input
#[derive(Clone, Copy)]
enum InputMode {
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
    fn next(&mut self, skip_switcher: bool) {
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
    fn prev(&mut self, skip_switcher: bool) {
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

pub enum LoginAction<'a> {
    None,
    Launch(AuthUserInfo<'a>, PostLoginEnvironment),
}

enum UIThreadRequest {
    Redraw,
    DisableTui,
    StopDrawing,
    // We use 'static here to send across channel, but we will transmute it back to 'a
    LoginSuccess(AuthUserInfo<'static>, PostLoginEnvironment),
}

#[derive(Clone)]
struct Widgets {
    background: BackgroundWidget,
    panel: PanelWidget,
    key_menu: KeyMenuWidget,
    environment: Arc<Mutex<SwitcherWidget<PostLoginEnvironment>>>,
    username: Arc<Mutex<InputFieldWidget>>,
    password: Arc<Mutex<InputFieldWidget>>,
}

impl Widgets {
    fn environment_guard(&self) -> MutexGuard<'_, SwitcherWidget<PostLoginEnvironment>> {
        match self.environment.lock() {
            Ok(guard) => guard,
            Err(err) => {
                error!("Lock failed. Reason: {}", err);
                std::process::exit(1);
            }
        }
    }
    fn username_guard(&self) -> MutexGuard<'_, InputFieldWidget> {
        match self.username.lock() {
            Ok(guard) => guard,
            Err(err) => {
                error!("Lock failed. Reason: {}", err);
                std::process::exit(1);
            }
        }
    }
    fn password_guard(&self) -> MutexGuard<'_, InputFieldWidget> {
        match self.password.lock() {
            Ok(guard) => guard,
            Err(err) => {
                error!("Lock failed. Reason: {}", err);
                std::process::exit(1);
            }
        }
    }

    fn get_environment(&self) -> Option<(String, PostLoginEnvironment)> {
        self.environment_guard()
            .selected()
            .map(|s| (s.title.clone(), s.content.clone()))
    }
    fn environment_try_select(&self, title: &str) {
        self.environment_guard().try_select(title);
    }
    fn get_username(&self) -> String {
        self.username_guard().get_content()
    }
    fn set_username(&self, content: &str) {
        self.username_guard().set_content(content)
    }
    fn get_password(&self) -> String {
        self.password_guard().get_content()
    }
    fn clear_password(&self) {
        self.password_guard().clear()
    }
}

/// App holds the state of the application
#[derive(Clone)]
pub struct LoginForm {
    /// Whether the application is running in preview mode
    preview: bool,

    widgets: Widgets,

    /// The configuration for the app
    config: Arc<Config>,
}

// Trait for backends that support enabling/disabling the UI (entering/leaving raw mode/alternate screen)
pub trait LoginBackend: ratatui::backend::Backend {
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

impl LoginForm {
    fn set_cache(&self) {
        let env_remember = self.config.environment_switcher.remember;
        let username_remember = self.config.username_field.remember;

        if !env_remember && !username_remember {
            info!("Nothing to cache.");
            return;
        }

        let selected_env = if self.config.environment_switcher.remember {
            self.widgets.get_environment().map(|(title, _)| title)
        } else {
            None
        };
        let username = self
            .config
            .username_field
            .remember
            .then_some(self.widgets.get_username());

        info!("Setting cached information");
        set_cache(selected_env.as_deref(), username.as_deref(), &self.config);
    }

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
}

// ... existing LoginForm struct ...

impl LoginForm {
    // ... existing methods ...

    pub fn run<'a, B: LoginBackend>(
        self,
        terminal: &mut Terminal<B>,
        pam_service: &'a str,
    ) -> io::Result<LoginAction<'a>> {
        terminal.backend_mut().enable_ui()?;
        self.load_cache();
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
        let panel_position = self.config.panel.position.clone();

        // Initial draw
        let draw_action = terminal.draw(|f| {
            let layout = Chunks::new(f, panel_position.clone());
            login_form_render(
                f,
                layout,
                &self.widgets,
                input_mode.get(),
                status_message.get(),
            );
        });

        if let Err(err) = draw_action {
            error!("Failed to draw. Reason: {}", err);
            std::process::exit(1);
        }

        let event_input_mode = input_mode.clone();
        let event_status_message = status_message.clone();

        let (req_send_channel, req_recv_channel) = channel();

        let widgets = self.widgets.clone();
        let config = self.config.clone();
        let preview = self.preview;
        let myself_clone = self.clone();
        // Transmute pam_service to static to allow it to be moved into the thread.
        // We know it actually lives as long as 'a (main), which outlives this function scope.
        let pam_service_static: &'static str = unsafe { std::mem::transmute(pam_service) };

        std::thread::spawn(move || {
            let mut switcher_hidden = widgets
                .environment
                .lock()
                .expect("Failed to grab environment lock")
                .hidden();
            let input_mode = event_input_mode;
            let status_message = event_status_message;

            let send_ui_request = |request: UIThreadRequest| match req_send_channel.send(request) {
                Ok(_) => {}
                Err(err) => warn!("Failed to send UI request. Reason: {}", err),
            };

            let pre_auth = || {
                widgets.clear_password();

                status_message.set(InfoStatusMessage::Authenticating);
                send_ui_request(UIThreadRequest::Redraw);
            };
            let pre_environment = || {
                // Remember username and environment for next time
                myself_clone.set_cache(); // Requires myself_clone

                status_message.set(InfoStatusMessage::LoggingIn);
                send_ui_request(UIThreadRequest::Redraw);

                // Disable the rendering of the login manager
                send_ui_request(UIThreadRequest::DisableTui);
            };

            // Hooks struct removed as we call them directly now

            loop {
                // NOTE: event::read() is blocking and uses Crossterm.
                // If we use KMS, we need to abstract event reading too.
                // But for now, let's assume TTY input works.
                if let Ok(Event::Key(key)) = event::read() {
                    match (key.code, input_mode.get(), key.modifiers) {
                        (KeyCode::Enter, InputMode::Password, _) => {
                            if preview {
                                // This is only for demonstration purposes
                                status_message.set(InfoStatusMessage::Authenticating);
                                send_ui_request(UIThreadRequest::Redraw);
                                std::thread::sleep(Duration::from_secs(2));

                                status_message.set(InfoStatusMessage::LoggingIn);
                                send_ui_request(UIThreadRequest::Redraw);
                                std::thread::sleep(Duration::from_secs(2));

                                status_message.clear();
                                send_ui_request(UIThreadRequest::Redraw);
                            } else {
                                let environment =
                                    widgets.get_environment().map(|(_, content)| content);
                                let username = widgets.get_username();
                                let password = widgets.get_password();
                                let _config = config.clone();

                                let Some(post_login_env) = environment else {
                                    status_message.set(ErrorStatusMessage::NoGraphicalEnvironment);
                                    send_ui_request(UIThreadRequest::Redraw);
                                    continue;
                                };

                                pre_auth(); // Call hook
                                match try_auth(&username, &password, pam_service_static) {
                                    Ok(auth_info) => {
                                        pre_environment(); // Call hook
                                                           // Transmute auth_info to 'static to send over channel
                                        let auth_info_static: AuthUserInfo<'static> =
                                            unsafe { std::mem::transmute(auth_info) };
                                        send_ui_request(UIThreadRequest::LoginSuccess(
                                            auth_info_static,
                                            post_login_env,
                                        ));
                                    }
                                    Err(AuthenticationError::PamService(err)) => {
                                        error!("PAM Service error: {}", err);
                                        status_message.set(
                                            ErrorStatusMessage::AuthenticationError(
                                                AuthenticationError::PamService(err),
                                            ),
                                        );
                                        send_ui_request(UIThreadRequest::Redraw);
                                    }
                                    Err(err) => {
                                        status_message
                                            .set(ErrorStatusMessage::AuthenticationError(err));
                                        send_ui_request(UIThreadRequest::Redraw);
                                    }
                                }
                            }
                        }
                        (KeyCode::Char('s'), InputMode::Normal, _) => myself_clone.set_cache(),

                        // On the TTY, it triggers the ALT key for some reason.
                        (KeyCode::Up | KeyCode::BackTab, _, _)
                        | (KeyCode::Tab, _, KeyModifiers::ALT | KeyModifiers::SHIFT)
                        | (KeyCode::Char('p'), _, KeyModifiers::CONTROL) => {
                            input_mode.prev(switcher_hidden);
                        }

                        (KeyCode::Enter | KeyCode::Down | KeyCode::Tab, _, _)
                        | (KeyCode::Char('n'), _, KeyModifiers::CONTROL) => {
                            input_mode.next(switcher_hidden);
                        }

                        // Esc is the overal key to get out of your input mode
                        (KeyCode::Esc, InputMode::Normal, _) => {
                            if preview {
                                info!("Pressed escape in preview mode to exit the application");
                                if let Err(e) = req_send_channel.send(UIThreadRequest::StopDrawing)
                                {
                                    warn!("Failed to send StopDrawing request: {:?}", e);
                                }
                            }
                        }

                        (KeyCode::Esc, _, _) => {
                            input_mode.set(InputMode::Normal);
                        }

                        (KeyCode::F(_), _, _) => {
                            widgets.key_menu.key_press(key.code);
                            widgets.environment_guard().key_press(key.code);

                            switcher_hidden = widgets
                                .environment
                                .lock()
                                .expect("Failed to grab lock")
                                .hidden();

                            if matches!(input_mode.get(), InputMode::Switcher) && switcher_hidden {
                                input_mode.next(true);
                            }
                        }

                        // For the different input modes the key should be passed to the corresponding
                        // widget.
                        (k, mode, modifiers) => {
                            let status_message_opt = match mode {
                                InputMode::Switcher => widgets.environment_guard().key_press(k),
                                InputMode::Username => {
                                    widgets.username_guard().key_press(k, modifiers)
                                }
                                InputMode::Password => {
                                    widgets.password_guard().key_press(k, modifiers)
                                }
                                _ => None,
                            };

                            // We don't wanna clear any existing error messages
                            if let Some(status_msg) = status_message_opt {
                                status_message.set(status_msg);
                            }
                        }
                    };
                }

                send_ui_request(UIThreadRequest::Redraw);
            }
        });

        // Start the UI thread. This actually draws to the screen.
        //
        // This blocks until we actually call StopDrawing
        while let Ok(request) = req_recv_channel.recv() {
            match request {
                UIThreadRequest::Redraw => {
                    let inputs_widgets = &self.widgets;
                    let draw_action = terminal.draw(|f| {
                        let layout = Chunks::new(f, panel_position.clone());
                        login_form_render(
                            f,
                            layout,
                            inputs_widgets,
                            input_mode.get(),
                            status_message.get(),
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
                    let info_a: AuthUserInfo<'a> = unsafe { std::mem::transmute(info) };
                    return Ok(LoginAction::Launch(info_a, env));
                }
                _ => break,
            }
        }

        Ok(LoginAction::None)
    }
}

fn login_form_render(
    frame: &mut Frame,
    chunks: Chunks,
    widgets: &Widgets,
    input_mode: InputMode,
    status_message: Option<StatusMessage>,
) {
    widgets.background.render(frame);
    widgets.panel.render(frame, chunks.panel_root);
    widgets.key_menu.render(frame, chunks.key_menu);
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
