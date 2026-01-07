mod pam;
pub mod utmpx;

use std::collections::HashMap;

use ::pam::{Client, PasswordConv};
use log::info;

use crate::auth::pam::open_session;
pub use crate::auth::pam::AuthenticationError;

pub struct AuthUserInfo<'a> {
    // This is used to keep the user session. If the struct is dropped then the user session is
    // also automatically dropped.
    #[allow(dead_code)]
    client: Client<'a, PasswordConv>,

    #[allow(dead_code)]
    pub username: String,

    pub uid: libc::uid_t,
    pub primary_gid: libc::gid_t,
    pub all_gids: Vec<libc::gid_t>,
    pub home_dir: String,
    pub shell: String,
}

impl<'a> AuthUserInfo<'a> {
    pub fn get_env(&self) -> HashMap<String, String> {
        // TODO: PAM 0.8.0 client does not expose environment variables via methods like `env` or `getenv`.
        // We return an empty map for now. Propagating PAM environment variables requires a different approach
        // or a different crate (e.g. pam-client or unsafe FFI).
        HashMap::new()
    }
}

pub fn try_auth<'a>(
    username: &str,
    password: &str,
    pam_service: &'a str,
) -> Result<AuthUserInfo<'a>, AuthenticationError> {
    info!("Login attempt for '{username}'");

    open_session(username, password, pam_service).inspect_err(|err| {
        info!(
            "Authentication failed for '{}'. Reason: {}",
            username,
            err.to_string()
        );
    })
}
