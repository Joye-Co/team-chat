//! WebSocket frames. Every frame is a JSON object tagged on `t`.

mod client;
mod server;

pub use client::*;
pub use server::*;

use crate::errors::{ErrorCode, FrameError};
use crate::limits::MAX_FRAME_BYTES;

fn invalid(message: impl Into<String>) -> FrameError {
    FrameError {
        code: ErrorCode::Invalid,
        message: message.into(),
    }
}

fn check_size(text: &str) -> Result<(), FrameError> {
    if text.len() > MAX_FRAME_BYTES {
        Err(FrameError {
            code: ErrorCode::TooLarge,
            message: "frame exceeds 1 MiB".into(),
        })
    } else {
        Ok(())
    }
}

/// Parses one WebSocket text frame from the server. Checks the frame size first.
pub fn parse_server_frame(text: &str) -> Result<ServerFrame, FrameError> {
    check_size(text)?;
    serde_json::from_str(text).map_err(|e| invalid(e.to_string()))
}

/// Parses one WebSocket text frame from a client and checks its limits.
pub fn parse_client_frame(text: &str) -> Result<ClientFrame, FrameError> {
    check_size(text)?;
    let frame: ClientFrame = serde_json::from_str(text).map_err(|e| invalid(e.to_string()))?;
    frame.validate().map_err(|e| invalid(e.to_string()))?;
    Ok(frame)
}

/// Checks the limits of a client frame and encodes it as JSON text.
pub fn encode_client_frame(frame: &ClientFrame) -> Result<String, FrameError> {
    frame.validate().map_err(|e| invalid(e.to_string()))?;
    let text = serde_json::to_string(frame).map_err(|e| invalid(e.to_string()))?;
    check_size(&text)?;
    Ok(text)
}
