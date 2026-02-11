use super::state::{InputMode, LoginFormInputMode, LoginFormStatusMessage};
use super::status_message::{ErrorStatusMessage, InfoStatusMessage};
use super::types::UIThreadRequest;
use super::widgets_collection::Widgets;
use crate::auth::{try_auth, AuthenticationError};
use crate::config::Config;
use crate::info_caching::set_cache;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use log::{error, info, warn};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

pub fn run(
    widgets: Widgets,
    config: Arc<Config>,
    pam_service: String,
    preview: bool,
    input_mode: LoginFormInputMode,
    status_message: LoginFormStatusMessage,
    req_send_channel: Sender<UIThreadRequest>,
) {
    let mut switcher_hidden = widgets
        .environment
        .lock()
        .expect("Failed to grab environment lock")
        .hidden();

    let send_ui_request = |request: UIThreadRequest| match req_send_channel.send(request) {
        Ok(_) => {}
        Err(err) => warn!("Failed to send UI request. Reason: {}", err),
    };

    // We need another clone for the loop usage if we want to call it multiple times
    // Actually, do_cache is a closure that moves widgets_clone. We can call it if it's Fn.
    // But widgets_clone is captured.
    // To cleanly call it in multiple places, let's just use the logic inline or defining it as a function that takes refs.
    // Redefining it to be simpler:
    let perform_caching = |w: &Widgets, c: &Config| {
        let env_remember = c.environment_switcher.remember;
        let username_remember = c.username_field.remember;

        if env_remember || username_remember {
            let selected_env = if env_remember {
                w.get_environment().map(|(title, _)| title)
            } else {
                None
            };
            let username = username_remember.then_some(w.get_username());
            info!("Setting cached information");
            set_cache(selected_env.as_deref(), username.as_deref(), c);
        }
    };

    // Need a way to call performing_caching from inside the loop
    // Since we have 'widgets' and 'config' available in the loop, we can just call the logic.

    let pre_auth = || {
        widgets.clear_password();
        status_message.set(InfoStatusMessage::Authenticating);
        send_ui_request(UIThreadRequest::Redraw);
    };

    // This closure needs to capture the caching logic
    let pre_environment = || {
        // Remember username and environment for next time
        perform_caching(&widgets, &config);

        status_message.set(InfoStatusMessage::LoggingIn);
        send_ui_request(UIThreadRequest::Redraw);

        // Disable the rendering of the login manager
        send_ui_request(UIThreadRequest::DisableTui);
    };

    // Last time we updated the battery
    let mut last_tick = std::time::Instant::now();
    let tick_rate = Duration::from_millis(1000);

    loop {
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout).unwrap_or(false) {
            // NOTE: event::read() is blocking and uses Crossterm.
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
                            let environment = widgets.get_environment().map(|(_, content)| content);
                            let username = widgets.get_username();
                            let password = widgets.get_password();
                            let tty_nr = config.tty;

                            let Some(post_login_env) = environment else {
                                status_message.set(ErrorStatusMessage::NoGraphicalEnvironment);
                                send_ui_request(UIThreadRequest::Redraw);
                                continue;
                            };

                            pre_auth(); // Call hook
                            match try_auth(
                                &username,
                                &password,
                                &pam_service,
                                &format!("/dev/tty{}", tty_nr),
                            ) {
                                Ok(auth_info) => {
                                    pre_environment(); // Call hook
                                    send_ui_request(UIThreadRequest::LoginSuccess(
                                        Box::new(auth_info),
                                        post_login_env,
                                    ));
                                }
                                Err(AuthenticationError::PamService(err)) => {
                                    error!("PAM Service error: {}", err);
                                    status_message.set(ErrorStatusMessage::AuthenticationError(
                                        AuthenticationError::PamService(err),
                                    ));
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
                    (KeyCode::Char('s'), InputMode::Normal, _) => {
                        perform_caching(&widgets, &config)
                    }

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
                            if let Err(e) = req_send_channel.send(UIThreadRequest::StopDrawing) {
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
                            InputMode::Username => widgets.username_guard().key_press(k, modifiers),
                            InputMode::Password => widgets.password_guard().key_press(k, modifiers),
                            _ => None,
                        };

                        // We don't wanna clear any existing error messages
                        if let Some(status_msg) = status_message_opt {
                            status_message.set(status_msg);
                        }
                    }
                };
            }
        }

        if last_tick.elapsed() >= tick_rate {
            // Update Battery
            if let Ok(mut battery) = widgets.battery.lock() {
                battery.update();
            }

            // Always redraw on tick to ensure clock/battery updates are shown
            send_ui_request(UIThreadRequest::Redraw);
            last_tick = std::time::Instant::now();
        } else {
            // If we didn't tick but we processed an event, we likely want to redraw immediately
            // to show typing feedback.
            send_ui_request(UIThreadRequest::Redraw);
        }
    }
}
