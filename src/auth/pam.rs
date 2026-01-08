use pam_sys::*;
use secrecy::{ExposeSecret, SecretString};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::fmt;
use std::ptr;
use std::sync::Mutex;
use uzers::os::unix::UserExt;

use crate::auth::AuthUserInfo;

/// All the different errors that can occur during PAM opening an authenticated session
#[derive(Clone, Debug)]
pub enum AuthenticationError {
    PamService(String),
    AccountValidation,
    HomeDirInvalidUtf8,
    ShellInvalidUtf8,
    UsernameNotFound,
    SessionOpen,
    CredUnavailable,
    CredUninitialized,
    CredExpired,
    Other(i32),
}

impl fmt::Display for AuthenticationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PamService(service) => write!(f, "Failed to create authenticator with PAM service '{service}'"),
            Self::AccountValidation => f.write_str("Invalid login credentials"),
            Self::HomeDirInvalidUtf8 => f.write_str("User home directory path contains invalid UTF-8"),
            Self::ShellInvalidUtf8 => f.write_str("User shell path contains invalid UTF-8"),
            Self::UsernameNotFound => f.write_str("Login creditionals are valid, but username is not found. This should not be possible :("),
            Self::SessionOpen => f.write_str("Failed to open a PAM session"),
            Self::CredUnavailable => f.write_str("Failed to set credentials: Unavailable"),
            Self::CredUninitialized => f.write_str("Failed to set credentials: Uninitialized"),
            Self::CredExpired => f.write_str("Failed to set credentials: Expired"),
            Self::Other(code) => write!(f, "PAM error code: {}", code),
        }
    }
}

// Data passed to the conversation function.
// Wrapped in Mutex for thread safety (though PAM usually calls on same thread, Send requirement necessitates it).
struct ConvData {
    password: Mutex<Option<SecretString>>,
}

pub struct PamAuthenticator {
    handle: *mut pam_handle_t,
    last_status: i32,
    #[allow(dead_code)] // Kept alive for the lifetime of the handle
    conv_data: Box<ConvData>,
}

unsafe impl Send for PamAuthenticator {}

impl Drop for PamAuthenticator {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                pam_end(self.handle, self.last_status);
            }
        }
    }
}

extern "C" fn conversation(
    num_msg: i32,
    msg: *mut *const pam_message,
    resp: *mut *mut pam_response,
    appdata_ptr: *mut libc::c_void,
) -> i32 {
    unsafe {
        let appdata = &*(appdata_ptr as *const ConvData);
        let msgs = std::slice::from_raw_parts(msg, num_msg as usize);

        // Allocate space for responses
        let responses = libc::calloc(num_msg as usize, std::mem::size_of::<pam_response>())
            as *mut pam_response;

        if responses.is_null() {
            return PAM_BUF_ERR;
        }

        let resp_slice = std::slice::from_raw_parts_mut(responses, num_msg as usize);

        for (i, m_ptr) in msgs.iter().enumerate() {
            let m = **m_ptr;
            match m.msg_style {
                PAM_PROMPT_ECHO_OFF | PAM_PROMPT_ECHO_ON => {
                    // Provide password
                    if let Ok(guard) = appdata.password.lock() {
                        if let Some(ref secret) = *guard {
                            let p = CString::new(secret.expose_secret().clone()).unwrap();
                            resp_slice[i].resp = libc::strdup(p.as_ptr());
                            resp_slice[i].resp_retcode = 0;
                        } else {
                            // Password already cleared or not provided?
                            // This might happen during account management if they ask again.
                            resp_slice[i].resp = ptr::null_mut();
                            resp_slice[i].resp_retcode = 0;
                        }
                    } else {
                        // Mutex poisoned
                        libc::free(responses as *mut libc::c_void);
                        return PAM_CONV_ERR;
                    }
                }
                PAM_ERROR_MSG | PAM_TEXT_INFO => {
                    // Ignore info/error messages for now, or log them
                    resp_slice[i].resp = ptr::null_mut();
                    resp_slice[i].resp_retcode = 0;
                }
                _ => {
                    // Unknown message style
                    // Clean up
                    libc::free(responses as *mut libc::c_void);
                    return PAM_CONV_ERR;
                }
            }
        }

        *resp = responses;
        PAM_SUCCESS
    }
}

/// Open a PAM authenticated session
pub fn open_session(
    username: &str,
    password: &SecretString,
    pam_service: &str,
) -> Result<AuthUserInfo, AuthenticationError> {
    log::info!("Started opening session via PAM-SYS");

    let c_user = CString::new(username).map_err(|_| AuthenticationError::UsernameNotFound)?;
    let c_service = CString::new(pam_service)
        .map_err(|_| AuthenticationError::PamService(pam_service.to_string()))?;

    // Create ConvData on heap
    let conv_data = Box::new(ConvData {
        password: Mutex::new(Some(password.clone())),
    });

    // We pass a raw pointer to PAM, but we keep ownership in PamAuthenticator
    let conv_ptr = &*conv_data as *const ConvData as *mut libc::c_void;

    let conv = pam_conv {
        conv: Some(conversation),
        appdata_ptr: conv_ptr,
    };

    let mut handle: *mut pam_handle_t = ptr::null_mut();

    let ret = unsafe { pam_start(c_service.as_ptr(), c_user.as_ptr(), &conv, &mut handle) };

    let mut auth = PamAuthenticator {
        handle,
        last_status: ret,
        conv_data, // Ownership moved here. It will be dropped when `auth` is dropped.
    };

    if ret != PAM_SUCCESS {
        return Err(AuthenticationError::PamService(pam_service.to_string()));
    }

    // 1. Authenticate
    auth.last_status = unsafe { pam_authenticate(handle, 0) };
    if auth.last_status != PAM_SUCCESS {
        return Err(AuthenticationError::AccountValidation);
    }

    // Securely clear the password from memory now that authentication is done.
    // We keep the ConvData struct alive, but empty the Option inside the Mutex.
    // This assumes subsequent PAM calls won't need the password again.
    // If they do (e.g. some complex re-auth), this would fail, which is secure-by-default.
    if let Ok(mut guard) = auth.conv_data.password.lock() {
        *guard = None;
    }

    // 2. Account Management
    auth.last_status = unsafe { pam_acct_mgmt(handle, 0) };
    if auth.last_status != PAM_SUCCESS {
        return Err(AuthenticationError::AccountValidation);
    }

    // 3. Set Credentials (Initialize Keyrings!)
    auth.last_status = unsafe { pam_setcred(handle, PAM_ESTABLISH_CRED as i32) };
    if auth.last_status != PAM_SUCCESS {
        match auth.last_status {
            PAM_CRED_UNAVAIL => return Err(AuthenticationError::CredUnavailable),
            PAM_CRED_EXPIRED => return Err(AuthenticationError::CredExpired),
            _ => return Err(AuthenticationError::Other(auth.last_status)),
        }
    }

    // 4. Open Session
    auth.last_status = unsafe { pam_open_session(handle, 0) };
    if auth.last_status != PAM_SUCCESS {
        return Err(AuthenticationError::SessionOpen);
    }

    log::info!("PAM Session Opened Successfully");

    // 5. Get Environment (SSH_AUTH_SOCK, etc.)
    let mut pam_env = HashMap::new();
    unsafe {
        let env_list = pam_getenvlist(handle);
        if !env_list.is_null() {
            let mut curr = env_list;
            while !(*curr).is_null() {
                let env_str = CStr::from_ptr(*curr).to_string_lossy();
                if let Some((key, val)) = env_str.split_once('=') {
                    pam_env.insert(key.to_string(), val.to_string());
                }

                // Free the individual string
                libc::free(*curr as *mut libc::c_void);

                curr = curr.add(1);
            }
            // Free the array itself
            libc::free(env_list as *mut libc::c_void);
        }
    }

    // Fetch User Info (via uzers)
    let user = uzers::get_user_by_name(username).ok_or(AuthenticationError::UsernameNotFound)?;

    let uid = user.uid();
    let primary_gid = user.primary_group_id();
    let all_gids = user.groups().map_or_else(Vec::default, |v| {
        v.into_iter().map(|group| group.gid()).collect()
    });
    let home_dir = user
        .home_dir()
        .to_str()
        .ok_or(AuthenticationError::HomeDirInvalidUtf8)?
        .to_string();
    let shell = user
        .shell()
        .to_str()
        .ok_or(AuthenticationError::ShellInvalidUtf8)?
        .to_string();

    Ok(AuthUserInfo {
        authenticator: auth,
        username: username.to_string(),
        uid,
        primary_gid,
        all_gids,
        home_dir,
        shell,
        pam_env,
    })
}
