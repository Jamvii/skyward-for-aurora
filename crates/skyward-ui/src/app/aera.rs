//! The AERA WPE browser's page state.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AeraPanel {
    History,
    Bookmarks,
    Downloads,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Aera {
    /// What was typed, and what the worker last loaded.
    pub address: String,
    pub loaded: String,
    pub status: String,
    pub can_back: bool,
    pub can_forward: bool,
    /// The last ARGB frame, 1080x2100, little-endian B,G,R,A.
    pub frame: Option<Vec<u8>>,
    /// The runtime has been started for this session.
    pub started: bool,
    pub failed: Option<String>,
    /// The web textbox asked for the keyboard: its input purpose.
    pub keyboard: Option<u32>,
    /// The address is being edited in a bottom sheet over the page.
    pub address_open: bool,
    /// A history, bookmark or download sheet.
    pub panel: Option<AeraPanel>,
    /// Pages opened in this session, newest last.
    pub history: Vec<String>,
    pub bookmarks: Vec<String>,
    /// The display backend composites the frame below a transparent page.
    pub gpu: bool,
}
