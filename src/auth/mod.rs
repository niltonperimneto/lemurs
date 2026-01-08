mod pam;
pub mod utmpx;

use log::info;
use std::collections::HashMap;

pub use crate::auth::pam::AuthenticationError;
use crate::auth::pam::{open_session, PamAuthenticator};

use secrecy::SecretString;

pub struct AuthUserInfo {
    // This is used to keep the user session. If the struct is dropped then the user session is
    // also automatically dropped.
    #[allow(dead_code)]
    pub authenticator: PamAuthenticator,

    #[allow(dead_code)]
    pub username: String,

    pub uid: libc::uid_t,
    pub primary_gid: libc::gid_t,
    pub all_gids: Vec<libc::gid_t>,
    pub home_dir: String,
    pub shell: String,
    /// Environment variables provided by PAM modules (e.g. SSH_AUTH_SOCK, XDG_RUNTIME_DIR)
    pub pam_env: HashMap<String, String>,
}

impl AuthUserInfo {
    pub fn get_env(&self) -> HashMap<String, String> {
        self.pam_env.clone()
    }
}

pub fn try_auth(
    username: &str,
    password: &SecretString,
    pam_service: &str,
) -> Result<AuthUserInfo, AuthenticationError> {
    info!("Login attempt for '{username}'");

    open_session(username, password, pam_service).inspect_err(|err| {
        info!(
            "Authentication failed for '{}'. Reason: {}",
            username,
            err.to_string()
        );
    })
}
