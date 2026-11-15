//! The AERA browser's control plane: protocol, session and the launcher's
//! fixed descriptors. Rendering is WPE's; this crate never draws.

pub mod process;
pub mod protocol;
pub mod session;

pub use protocol::{address, Kind, Message, FRAME_BYTES, HEIGHT, SHARED_BYTES, VIEW_HEIGHT, VIEW_WIDTH, WIDTH};
pub use process::Process;
pub use session::{Keyboard, Session};
