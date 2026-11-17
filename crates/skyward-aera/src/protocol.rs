//! AERA's WPE browser protocol, as the recovery side reads it.
//!
//! The worker and the recovery share a fixed 2072-byte message on a
//! `SOCK_SEQPACKET` channel, plus two sealed pixel slots in a memfd. No
//! pointers, file paths or recovery operations cross this ABI.

pub const MAGIC: u32 = 0x4152_5032; // "ARP2"
pub const WIDTH: usize = 1080;
pub const HEIGHT: usize = 2100;
pub const VIEW_WIDTH: i32 = 360;
pub const VIEW_HEIGHT: i32 = 700;
pub const DEVICE_SCALE: f64 = 3.0;
pub const FRAME_SLOTS: usize = 2;
pub const FRAME_BYTES: usize = WIDTH * HEIGHT * 4;
pub const SHARED_BYTES: usize = FRAME_BYTES * FRAME_SLOTS;

/// The kinds a controller sends, and those the worker sends back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Kind {
    Open = 1,
    Back,
    Forward,
    Reload,
    Stop,
    TouchDown,
    TouchMove,
    TouchUp,
    Key,
    Ack,
    Close,
    SetZoom,
    DownloadCancel,
    SetCookiePolicy,
    ClearBrowsingData,
    Frame = 32,
    Status,
    Error,
    KeyboardShow,
    KeyboardHide,
    DownloadStarted,
    DownloadProgress,
    DownloadFinished,
    DownloadFailed,
    DownloadCancelled,
    BrowsingDataCleared,
    BrowsingDataFailed,
}

impl Kind {
    pub fn from_u32(value: u32) -> Option<Self> {
        Some(match value {
            1 => Self::Open,
            2 => Self::Back,
            3 => Self::Forward,
            4 => Self::Reload,
            5 => Self::Stop,
            6 => Self::TouchDown,
            7 => Self::TouchMove,
            8 => Self::TouchUp,
            9 => Self::Key,
            10 => Self::Ack,
            11 => Self::Close,
            12 => Self::SetZoom,
            13 => Self::DownloadCancel,
            14 => Self::SetCookiePolicy,
            15 => Self::ClearBrowsingData,
            32 => Self::Frame,
            33 => Self::Status,
            34 => Self::Error,
            35 => Self::KeyboardShow,
            36 => Self::KeyboardHide,
            37 => Self::DownloadStarted,
            38 => Self::DownloadProgress,
            39 => Self::DownloadFinished,
            40 => Self::DownloadFailed,
            41 => Self::DownloadCancelled,
            42 => Self::BrowsingDataCleared,
            43 => Self::BrowsingDataFailed,
            _ => return None,
        })
    }
}

/// The exact AERA wire message. Its layout is part of the worker ABI.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Message {
    pub magic: u32,
    pub kind: u32,
    pub sequence: u32,
    pub x: i32,
    pub y: i32,
    pub value: u32,
    pub text: [u8; 2048],
}

const _: () = assert!(std::mem::size_of::<Message>() == 2072);
const _: () = assert!(std::mem::align_of::<Message>() == 4);

impl Default for Message {
    fn default() -> Self {
        Self { magic: MAGIC, kind: Kind::Status as u32, sequence: 0, x: 0, y: 0, value: 0, text: [0; 2048] }
    }
}

impl Message {
    pub fn new(kind: Kind, sequence: u32) -> Self {
        Self { kind: kind as u32, sequence, ..Self::default() }
    }

    pub fn bytes(&self) -> &[u8] {
        // SAFETY: Message is repr(C), every field is plain data, and the
        // returned slice lasts as long as the message.
        unsafe { std::slice::from_raw_parts((self as *const Self).cast::<u8>(), std::mem::size_of::<Self>()) }
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != std::mem::size_of::<Self>() {
            return None;
        }
        // SAFETY: the message is plain data with no invalid bit patterns.
        Some(unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<Self>()) })
    }

    pub fn text(&self) -> String {
        let end = self.text.iter().position(|&b| b == 0).unwrap_or(self.text.len());
        String::from_utf8_lossy(&self.text[..end]).into_owned()
    }

    pub fn set_text(&mut self, text: &str) -> bool {
        let bytes = text.as_bytes();
        if bytes.len() >= self.text.len() {
            return false;
        }
        self.text.fill(0);
        self.text[..bytes.len()].copy_from_slice(bytes);
        true
    }

    pub fn kind(&self) -> Option<Kind> {
        Kind::from_u32(self.kind)
    }
}

/// What the worker may send, as AERA's protocol checks it.
pub fn valid_from_worker(message: &Message) -> bool {
    if message.magic != MAGIC || !message.text.contains(&0) {
        return false;
    }
    let Some(kind) = message.kind() else { return false };
    match kind {
        Kind::Frame => message.value as usize == FRAME_BYTES
            && message.x == WIDTH as i32
            && message.y == HEIGHT as i32,
        Kind::Status => message.value <= 100
            && (0..=1).contains(&message.x)
            && (0..=1).contains(&message.y),
        Kind::Error | Kind::KeyboardHide | Kind::BrowsingDataCleared | Kind::BrowsingDataFailed => true,
        Kind::KeyboardShow => message.value <= 10,
        Kind::DownloadStarted
        | Kind::DownloadProgress
        | Kind::DownloadFinished
        | Kind::DownloadFailed
        | Kind::DownloadCancelled => {
            message.sequence > 0
                && (0..=100).contains(&message.x)
                && message.y >= 0
        }
        _ => false,
    }
}

/// What the controller may send.
pub fn valid_from_controller(message: &Message) -> bool {
    if message.magic != MAGIC || !message.text.contains(&0) {
        return false;
    }
    match message.kind() {
        Some(Kind::TouchDown | Kind::TouchMove | Kind::TouchUp) => {
            (0..VIEW_WIDTH).contains(&message.x)
                && (0..VIEW_HEIGHT).contains(&message.y)
                && message.value < 2
        }
        Some(Kind::SetZoom) => (50..=300).contains(&message.value),
        Some(Kind::DownloadCancel) => message.sequence > 0,
        Some(Kind::SetCookiePolicy) => message.value <= 2,
        Some(
            Kind::Open
            | Kind::Back
            | Kind::Forward
            | Kind::Reload
            | Kind::Stop
            | Kind::Key
            | Kind::Ack
            | Kind::Close
            | Kind::ClearBrowsingData,
        ) => true,
        _ => false,
    }
}

/// Turn what a person typed into the address the worker accepts. There is
/// deliberately no implicit search and no file, javascript or data scheme.
pub fn address(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 2040 {
        return None;
    }
    if value == "aera://start" || value == "aera://test" {
        return Some(value.to_string());
    }
    if value.bytes().any(|c| c <= 32 || c == 127 || c == b'\\') {
        return None;
    }
    let value = if value.contains("://") {
        value.to_string()
    } else {
        if value.contains(':') {
            return None;
        }
        format!("https://{value}")
    };
    let prefix = if value.starts_with("https://") {
        8
    } else if value.starts_with("http://") {
        7
    } else {
        return None;
    };
    if value.len() >= 2048 {
        return None;
    }
    let end = value[prefix..].find(['/', '?', '#']).map_or(value.len(), |i| prefix + i);
    let authority = &value[prefix..end];
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_is_the_workers_wire_size() {
        assert_eq!(std::mem::size_of::<Message>(), 2072);
        let message = Message::new(Kind::Open, 0);
        assert_eq!(message.bytes().len(), 2072);
        assert_eq!(Message::from_bytes(message.bytes()).unwrap().kind(), Some(Kind::Open));
    }

    #[test]
    fn the_whole_command_and_event_range_is_named() {
        for (value, kind) in [
            (1, Kind::Open),
            (2, Kind::Back),
            (15, Kind::ClearBrowsingData),
            (32, Kind::Frame),
            (43, Kind::BrowsingDataFailed),
        ] {
            assert_eq!(Kind::from_u32(value), Some(kind));
        }
    }

    #[test]
    fn addresses_are_https_or_http_only() {
        assert_eq!(address("example.com").as_deref(), Some("https://example.com"));
        assert_eq!(address("https://example.com/a").as_deref(), Some("https://example.com/a"));
        assert_eq!(address("http://example.com").as_deref(), Some("http://example.com"));
        assert_eq!(address("aera://start").as_deref(), Some("aera://start"));
        for bad in ["file:///etc/passwd", "javascript:alert(1)", "https://user@example.com", "example.com:8080"] {
            assert_eq!(address(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_frame_must_be_the_fixed_size_and_slot() {
        let mut message = Message::new(Kind::Frame, 1);
        message.x = WIDTH as i32;
        message.y = HEIGHT as i32;
        message.value = FRAME_BYTES as u32;
        assert!(valid_from_worker(&message));
        message.value -= 1;
        assert!(!valid_from_worker(&message));
    }
}
