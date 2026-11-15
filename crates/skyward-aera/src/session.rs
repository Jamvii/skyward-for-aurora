//! The recovery side of one AERA browser session: the sealed pixel memfd
//! and the `SOCK_SEQPACKET` control channel.

use crate::protocol::{
    valid_from_worker, Kind, Message, FRAME_BYTES, FRAME_SLOTS, SHARED_BYTES,
};
use anyhow::{Context, Result, bail};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

/// What the page asks the recovery's keyboard to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Keyboard {
    #[default]
    None,
    Show(u32),
    Hide,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadEvent {
    Started,
    Progress,
    Finished,
    Failed(String),
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadMessage {
    pub id: u64,
    pub event: DownloadEvent,
    pub name: String,
    pub done: u64,
    pub total: Option<u64>,
}

pub struct Session {
    shared: *mut u8,
    shared_len: usize,
    control: OwnedFd,
    sequence: u32,
    frame_pending: bool,
    status: String,
    progress: u32,
    can_back: bool,
    can_forward: bool,
    zoom: u32,
    keyboard: Keyboard,
    failed: Option<String>,
    downloads: Vec<DownloadMessage>,
}

impl Session {
    /// Take the two descriptors the launcher created. They are checked
    /// exactly as AERA checks them: a sealed memfd of the full two-slot
    /// frame and a `SOCK_SEQPACKET` socket.
    pub fn adopt(frame_fd: RawFd, control_fd: RawFd) -> Result<Self> {
        if frame_fd < 0 || control_fd < 0 || frame_fd == control_fd {
            bail!("the browser channels are not two descriptors");
        }
        let mut info = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: `info` is a valid stat buffer and the descriptor is ours
        // until the OwnedFd below takes it.
        if unsafe { libc::fstat(frame_fd, info.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error()).context("stat the browser pixels");
        }
        // SAFETY: fstat succeeded.
        let info = unsafe { info.assume_init() };
        if info.st_size as usize != SHARED_BYTES {
            bail!("the browser pixels are {} bytes, not {SHARED_BYTES}", info.st_size);
        }
        let seals = unsafe { libc::fcntl(frame_fd, libc::F_GET_SEALS) };
        if seals < 0 || seals & (libc::F_SEAL_SHRINK | libc::F_SEAL_GROW) != (libc::F_SEAL_SHRINK | libc::F_SEAL_GROW) {
            bail!("the browser pixels are not sealed");
        }
        let mut kind: libc::c_int = 0;
        let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        // SAFETY: kind and len are valid for the call.
        if unsafe {
            libc::getsockopt(control_fd, libc::SOL_SOCKET, libc::SO_TYPE, (&raw mut kind).cast(), &raw mut len)
        } != 0
            || kind != libc::SOCK_SEQPACKET
        {
            bail!("the browser control channel is not SOCK_SEQPACKET");
        }
        // SAFETY: the descriptor is valid and the length is the mapped size.
        let shared = unsafe {
            libc::mmap(std::ptr::null_mut(), SHARED_BYTES, libc::PROT_READ, libc::MAP_SHARED, frame_fd, 0)
        };
        if shared == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error()).context("map the browser pixels");
        }
        // SAFETY: the descriptors are adopted exactly once.
        let frame = unsafe { OwnedFd::from_raw_fd(frame_fd) };
        let control = unsafe { OwnedFd::from_raw_fd(control_fd) };
        drop(frame);
        // SAFETY: the descriptor is a socket and is non-blocking from here on.
        unsafe { libc::fcntl(control.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) };
        unsafe { libc::fcntl(control.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
        Ok(Self {
            shared: shared.cast(),
            shared_len: SHARED_BYTES,
            control,
            sequence: 0,
            frame_pending: false,
            status: String::new(),
            progress: 0,
            can_back: false,
            can_forward: false,
            zoom: 100,
            keyboard: Keyboard::None,
            failed: None,
            downloads: Vec::new(),
        })
    }

    pub fn connected(&self) -> bool {
        self.failed.is_none()
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn progress(&self) -> u32 {
        self.progress
    }

    pub fn can_back(&self) -> bool {
        self.can_back
    }

    pub fn can_forward(&self) -> bool {
        self.can_forward
    }

    pub fn zoom(&self) -> u32 {
        self.zoom
    }

    pub fn keyboard(&self) -> Keyboard {
        self.keyboard
    }

    pub fn take_keyboard(&mut self) -> Keyboard {
        std::mem::take(&mut self.keyboard)
    }

    /// Download events the worker has reported since the last call.
    pub fn take_downloads(&mut self) -> Vec<DownloadMessage> {
        std::mem::take(&mut self.downloads)
    }

    /// Cancel one worker download by the id it announced.
    pub fn cancel_download(&mut self, id: u64) -> bool {
        let mut message = Message::new(Kind::DownloadCancel, id as u32);
        message.sequence = id as u32;
        self.write(&message)
    }

    pub fn frame(&self) -> Option<&[u8]> {
        if self.sequence == 0 || self.shared.is_null() {
            return None;
        }
        let slot = self.sequence as usize % FRAME_SLOTS;
        // SAFETY: the mapped region holds FRAME_SLOTS slots and the slot was
        // published by the worker before its message was read.
        Some(unsafe { std::slice::from_raw_parts(self.shared.add(slot * FRAME_BYTES), FRAME_BYTES) })
    }

    pub fn has_frame(&self) -> bool {
        self.sequence != 0
    }

    pub fn sequence(&self) -> u32 {
        self.sequence
    }

    pub fn send(&mut self, kind: Kind, x: i32, y: i32, value: u32, text: &str) -> bool {
        let mut message = Message::new(kind, self.sequence);
        message.x = x;
        message.y = y;
        message.value = value;
        if !message.set_text(text) {
            return false;
        }
        self.write(&message)
    }

    pub fn open(&mut self, address: &str) -> bool {
        self.send(Kind::Open, 0, 0, 0, address)
    }

    pub fn back(&mut self) -> bool {
        self.send(Kind::Back, 0, 0, 0, "")
    }

    pub fn forward(&mut self) -> bool {
        self.send(Kind::Forward, 0, 0, 0, "")
    }

    pub fn reload(&mut self) -> bool {
        self.send(Kind::Reload, 0, 0, 0, "")
    }

    pub fn stop(&mut self) -> bool {
        self.send(Kind::Stop, 0, 0, 0, "")
    }

    pub fn touch(&mut self, kind: Kind, x: i32, y: i32) -> bool {
        self.send(kind, x, y, 0, "")
    }

    pub fn key(&mut self, key: &str) -> bool {
        self.send(Kind::Key, 0, 0, 0, key)
    }

    pub fn set_zoom(&mut self, percent: u32) -> bool {
        let percent = percent.clamp(50, 300);
        if !self.send(Kind::SetZoom, 0, 0, percent, "") {
            return false;
        }
        self.zoom = percent;
        true
    }

    /// Acknowledge the last published frame. The worker will not publish
    /// the next one until this arrives.
    pub fn acknowledge(&mut self) -> bool {
        if !self.frame_pending {
            return self.connected();
        }
        let message = Message::new(Kind::Ack, self.sequence);
        if !self.write(&message) {
            return false;
        }
        self.frame_pending = false;
        true
    }

    /// Read at most eight messages. `true` when the visible frame changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        for _ in 0..8 {
            let mut message = std::mem::MaybeUninit::<Message>::uninit();
            // SAFETY: the buffer is exactly one Message and the socket is
            // a seqpacket channel.
            let count = unsafe {
                libc::recv(
                    self.control.as_raw_fd(),
                    message.as_mut_ptr().cast(),
                    std::mem::size_of::<Message>(),
                    libc::MSG_DONTWAIT | libc::MSG_TRUNC,
                )
            };
            if count < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    break;
                }
                if error.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                self.fail("browser input channel stopped responding");
                break;
            }
            if count as usize != std::mem::size_of::<Message>() {
                self.fail("browser closed or sent a short message");
                break;
            }
            // SAFETY: recv filled a whole Message.
            let message = unsafe { message.assume_init() };
            if !valid_from_worker(&message) {
                self.fail("browser sent an invalid message");
                break;
            }
            match message.kind() {
                Some(Kind::Frame) => {
                    if self.frame_pending || message.sequence != self.sequence.wrapping_add(1) {
                        self.fail("browser frame sequence mismatch");
                        break;
                    }
                    self.sequence = message.sequence;
                    self.frame_pending = true;
                    changed = true;
                }
                Some(Kind::Status) => {
                    self.progress = message.value;
                    self.can_back = message.x != 0;
                    self.can_forward = message.y != 0;
                    self.status = message.text();
                }
                Some(Kind::Error) => {
                    self.status = message.text();
                    self.fail(&self.status.clone());
                    break;
                }
                Some(Kind::KeyboardShow) => self.keyboard = Keyboard::Show(message.value),
                Some(Kind::KeyboardHide) => self.keyboard = Keyboard::Hide,
                Some(kind @ (Kind::DownloadStarted | Kind::DownloadProgress | Kind::DownloadFinished
                    | Kind::DownloadFailed | Kind::DownloadCancelled)) => {
                    let total = (message.y > 0).then_some(message.y as u64 * 1024);
                    let done = message.value as u64 * 1024;
                    let event = match kind {
                        Kind::DownloadStarted => DownloadEvent::Started,
                        Kind::DownloadProgress => DownloadEvent::Progress,
                        Kind::DownloadFinished => DownloadEvent::Finished,
                        Kind::DownloadFailed => DownloadEvent::Failed(message.text()),
                        Kind::DownloadCancelled => DownloadEvent::Cancelled,
                        _ => unreachable!(),
                    };
                    self.downloads.push(DownloadMessage {
                        id: message.sequence as u64,
                        event,
                        name: message.text(),
                        done,
                        total,
                    });
                }
                Some(_) => {}
                None => {}
            }
        }
        changed
    }

    fn write(&mut self, message: &Message) -> bool {
        if !self.connected() {
            return false;
        }
        // SAFETY: message is a full Message and the descriptor is our
        // seqpacket socket.
        let count = unsafe {
            libc::send(
                self.control.as_raw_fd(),
                message.bytes().as_ptr().cast(),
                message.bytes().len(),
                libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
            )
        };
        if count as usize != message.bytes().len() {
            self.fail("browser input channel stopped responding");
            return false;
        }
        true
    }

    fn fail(&mut self, why: &str) {
        self.failed.get_or_insert_with(|| why.to_string());
        self.status = why.to_string();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.shared.is_null() {
            // SAFETY: the mapping was made in adopt with this length.
            unsafe { libc::munmap(self.shared.cast(), self.shared_len) };
            self.shared = std::ptr::null_mut();
        }
    }
}
