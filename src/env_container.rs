use std::collections::HashMap;
use std::env;
use std::process::Command;

use log::{debug, info};
use secrecy::{ExposeSecret, SecretString};

/// The `EnvironmentContainer` abstracts the process environment.
///
/// It maintains an internal map of variables intended for the child process,
/// allowing secure management (via `SecretString`) without modifying the global
/// `lemurs` process environment.
#[derive(Debug, Clone)]
pub struct EnvironmentContainer {
    vars: HashMap<String, SecretString>,
    working_dir: String,
}

impl EnvironmentContainer {
    /// Creates a new container initialized with the current system environment.
    pub fn new() -> Self {
        let vars = env::vars()
            .map(|(k, v)| (k, SecretString::new(v)))
            .collect();

        let working_dir = env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "/".to_string());

        Self { vars, working_dir }
    }

    /// Set an environment variable. Overwrites if exists.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();
        debug!("Setting environment variable '{}'", key);
        self.vars.insert(key, SecretString::new(value));
    }

    /// Set an environment variable only if it is NOT already set.
    pub fn set_or_preserve(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        if self.vars.contains_key(&key) {
            debug!("Skipping '{}': already set", key);
        } else {
            self.set(key, value);
        }
    }

    // Alias to match previous API name if needed, but `set_or_preserve` is clearer.
    pub fn set_or_own(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.set_or_preserve(key, value);
    }

    pub fn remove_var(&mut self, key: &str) {
        if self.vars.remove(key).is_some() {
            debug!("Removed environment variable '{}'", key);
        }
    }

    /// Sets the intended working directory for the session
    pub fn set_current_dir(&mut self, value: impl Into<String>) {
        self.working_dir = value.into();
    }

    /// Applies the contained environment to a `Command`.
    ///
    /// This clears the Command's default environment and replaces it strictly
    /// with the variables in this container.
    pub fn apply_to_command(&self, command: &mut Command) {
        command.env_clear();
        for (key, val) in &self.vars {
            command.env(key, val.expose_secret());
        }
        command.current_dir(&self.working_dir);
        info!("Applied environment to command (PWD: {})", self.working_dir);
    }
}
