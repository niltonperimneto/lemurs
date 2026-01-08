pub struct UtmpxSession {
    #[cfg(target_env = "gnu")]
    session: libc::utmpx,
}

#[cfg(target_env = "gnu")]
pub fn add_utmpx_entry(username: &str, tty: u8, pid: u32) -> UtmpxSession {
    log::info!(
        "Adding UTMPX record for user '{}' on tty '{}'",
        username,
        tty
    );

    let entry = {
        // SAFETY: libc::utmpx is a C struct with no Rust destructors.
        // using zeroed() is standard for initializing C structs.
        let mut s: libc::utmpx = unsafe { std::mem::zeroed() };

        s.ut_type = libc::USER_PROCESS;
        s.ut_pid = pid as libc::pid_t;

        // Safely copy username (truncated to fits in ut_user)
        let user_bytes = username.as_bytes();
        let user_len = user_bytes.len().min(s.ut_user.len());
        for (i, b) in user_bytes.iter().take(user_len).enumerate() {
            s.ut_user[i] = *b as libc::c_char;
        }

        // Properly format TTY string (e.g., "tty1", "tty12")
        let tty_str = format!("tty{}", tty);
        let tty_bytes = tty_str.as_bytes();

        // ut_line (Device name, e.g., "tty1")
        let line_len = tty_bytes.len().min(s.ut_line.len());
        for (i, b) in tty_bytes.iter().take(line_len).enumerate() {
            s.ut_line[i] = *b as libc::c_char;
        }

        // ut_id (Terminal name suffix, e.g., "1", "12")
        // Usually just the number for TTYs
        let id_str = format!("{}", tty);
        let id_bytes = id_str.as_bytes();
        let id_len = id_bytes.len().min(s.ut_id.len());
        for (i, b) in id_bytes.iter().take(id_len).enumerate() {
            s.ut_id[i] = *b as libc::c_char;
        }

        // Set timestamp
        if let Ok(duration) =
            std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH)
        {
            s.ut_tv.tv_sec = duration.as_secs() as i32;
            s.ut_tv.tv_usec = duration.subsec_micros() as i32;
        } else {
            log::error!("System time is before UNIX EPOCH!");
        }

        // Set address (optional, but good practice to clear/set for local)
        // For local TTY, ut_addr_v6 is usually 0.

        s
    };

    unsafe {
        libc::setutxent();
        libc::pututxline(&entry as *const libc::utmpx);
        libc::endutxent(); // Always close properly
    };

    log::info!("Added UTMPX record");

    UtmpxSession { session: entry }
}

#[cfg(not(target_env = "gnu"))]
pub fn add_utmpx_entry(_username: &str, _tty: u8, _pid: u32) -> UtmpxSession {
    log::info!("Incompatible platform for UTMPX. Skipping...");
    UtmpxSession {}
}

#[cfg(target_env = "gnu")]
impl Drop for UtmpxSession {
    fn drop(&mut self) {
        log::info!("Removing UTMPX record");

        // Mark as dead process
        self.session.ut_type = libc::DEAD_PROCESS;

        // Zero out user and host fields for privacy/cleanup
        // We keep ut_line and ut_id so the system knows WHICH entry to update.
        // "pututxline() searches for ... a record with the same ut_type ... matching ut_id"
        // Actually, for DEAD_PROCESS, it matches by ut_id.

        unsafe {
            // libc::memset of ut_user to 0
            std::ptr::write_bytes(
                self.session.ut_user.as_mut_ptr(),
                0,
                self.session.ut_user.len(),
            );
            // Updates timestamp to now
            if let Ok(duration) =
                std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH)
            {
                self.session.ut_tv.tv_sec = duration.as_secs() as i32;
                self.session.ut_tv.tv_usec = duration.subsec_micros() as i32;
            }

            libc::setutxent();
            libc::pututxline(&self.session as *const libc::utmpx);
            libc::endutxent();
        }
    }
}
