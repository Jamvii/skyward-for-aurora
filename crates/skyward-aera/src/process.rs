//! Starting one isolated AERA worker and handing its two fixed descriptors
//! to a [`Session`].

use crate::protocol::SHARED_BYTES;
use crate::session::Session;
use anyhow::{Context, Result, bail};
use std::ffi::CString;
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::Path;

/// A running AERA supervisor and the session connected to it.
pub struct Process {
    pid: libc::pid_t,
    session: Option<Session>,
    frame: Option<OwnedFd>,
    control: Option<OwnedFd>,
}

impl Process {
    /// Create the sealed pixel buffer and the seqpacket channel, then run
    /// `/system/bin/aera-browser-jail --browser <runtime>` with them at fd 3
    /// and fd 4, exactly as AERA's launcher does.
    ///
    /// The jail is a separate fixed binary; this function never runs web
    /// content itself and never inherits recovery descriptors into it.
    pub fn start(runtime: &Path) -> Result<Self> {
        if !runtime.is_dir() {
            bail!("the AERA runtime {} is not a directory", runtime.display());
        }
        let runtime = CString::new(runtime.as_os_str().as_encoded_bytes())
            .context("the AERA runtime path contains a NUL")?;
        // SAFETY: memfd_create takes a NUL-terminated name and flags.
        let frame = unsafe {
            libc::memfd_create(c"aera-browser-pixels".as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING)
        };
        if frame < 0 {
            return Err(std::io::Error::last_os_error()).context("make the browser pixels");
        }
        // SAFETY: frame is ours and exclusively held here.
        let frame = unsafe { OwnedFd::from_raw_fd(frame) };
        if unsafe { libc::ftruncate(std::os::fd::AsRawFd::as_raw_fd(&frame), SHARED_BYTES as libc::off_t) } != 0 {
            return Err(std::io::Error::last_os_error()).context("size the browser pixels");
        }
        if unsafe {
            libc::fcntl(
                std::os::fd::AsRawFd::as_raw_fd(&frame),
                libc::F_ADD_SEALS,
                libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_SEAL,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error()).context("seal the browser pixels");
        }
        let mut channels = [-1; 2];
        // SAFETY: channels has room for two descriptors.
        if unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, 0, channels.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error()).context("make the browser control channel");
        }
        // SAFETY: both descriptors were just created.
        let (parent_control, child_control) = unsafe { (OwnedFd::from_raw_fd(channels[0]), OwnedFd::from_raw_fd(channels[1])) };
        // Move the child's copies above the fixed ABI before dup2, so one
        // dup2 can never overwrite the other source.
        // SAFETY: the descriptors are valid.
        let child_frame = unsafe { libc::fcntl(std::os::fd::AsRawFd::as_raw_fd(&frame), libc::F_DUPFD_CLOEXEC, 10) };
        let child_socket = unsafe { libc::fcntl(std::os::fd::AsRawFd::as_raw_fd(&child_control), libc::F_DUPFD_CLOEXEC, 10) };
        if child_frame < 0 || child_socket < 0 {
            return Err(std::io::Error::last_os_error()).context("protect the browser descriptors");
        }
        // Resolve this before fork: the child must not take Rust's
        // environment lock between fork and exec.
        let jail_path = std::env::var_os("AERA_JAIL").unwrap_or_else(|| "/system/bin/aera-browser-jail".into());
        let jail = CString::new(jail_path.as_encoded_bytes()).unwrap_or_else(|_| c"/system/bin/aera-browser-jail".into());
        // SAFETY: fork is called before any thread in this process touches
        // shared state after the descriptors are ready; the child only calls
        // async-signal-safe functions and execs.
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(std::io::Error::last_os_error()).context("start the AERA supervisor");
        }
        if pid == 0 {
            // SAFETY: this branch is the child; the descriptors are valid.
            unsafe {
                let _ = libc::setpgid(0, 0);
                if libc::dup2(child_frame, 3) < 0 || libc::dup2(child_socket, 4) < 0 {
                    libc::_exit(78);
                }
                libc::close(child_frame);
                libc::close(child_socket);
                let flag = c"--browser";
                let arguments = [jail.as_ptr(), flag.as_ptr(), runtime.as_ptr(), std::ptr::null()];
                libc::execv(jail.as_ptr(), arguments.as_ptr());
                libc::_exit(78);
            }
        }
        // SAFETY: the parent's copies are no longer needed.
        unsafe {
            libc::close(child_frame);
            libc::close(child_socket);
        }
        drop(child_control);
        let session = Session::adopt(
            // SAFETY: adopt takes ownership of the duplicated descriptor.
            unsafe { libc::dup(std::os::fd::AsRawFd::as_raw_fd(&frame)) },
            // SAFETY: adopt takes ownership of this descriptor.
            unsafe { libc::dup(std::os::fd::AsRawFd::as_raw_fd(&parent_control)) },
        )?;
        Ok(Self { pid, session: Some(session), frame: Some(frame), control: Some(parent_control) })
    }

    pub fn session(&mut self) -> &mut Session {
        self.session.as_mut().expect("a running process has a session")
    }

    pub fn running(&mut self) -> bool {
        if self.pid <= 0 {
            return false;
        }
        let mut status = 0;
        // SAFETY: status is a valid int and pid is our child.
        let done = unsafe { libc::waitpid(self.pid, &raw mut status, libc::WNOHANG) };
        if done == self.pid || (done < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD)) {
            self.pid = -1;
            return false;
        }
        true
    }

    pub fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        if self.pid > 0 {
            // SAFETY: pid is our child; SIGTERM is delivered to its group.
            unsafe {
                libc::kill(-self.pid, libc::SIGTERM);
                libc::kill(self.pid, libc::SIGTERM);
            }
            for _ in 0..25 {
                let mut status = 0;
                // SAFETY: status is valid; WNOHANG does not block.
                let done = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
                if done == self.pid {
                    self.pid = -1;
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            // SAFETY: the child is ours and still running.
            unsafe {
                libc::kill(-self.pid, libc::SIGKILL);
                libc::kill(self.pid, libc::SIGKILL);
                libc::waitpid(self.pid, std::ptr::null_mut(), 0);
            }
            self.pid = -1;
        }
        self.session.take();
        self.frame.take();
        self.control.take();
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.halt();
    }
}
