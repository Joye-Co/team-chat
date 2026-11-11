//! The wire between the daemon and `vm-agent` in a run's microVM: one vsock stream (port
//! [`AGENT_PORT`]) of frames `[tag u8][length u32 big-endian][payload]`.
//!
//! Daemon → agent: [`Tag::Start`] (JSON [`Start`]) once, then stdin bytes, stdin end, signals and
//! sync requests. Agent → daemon: stdout bytes, stdout end, the exit status, errors and the sync
//! answer. A run's `chat-run` connects from the VM to the daemon on [`CHAT_RUN_PORT`].

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// The guest port vm-agent listens on.
pub const AGENT_PORT: u32 = 1024;
/// The host port a guest connects to for `chat-run` (Firecracker forwards it to the unix
/// socket `<vsock uds>_1025`).
pub const CHAT_RUN_PORT: u32 = 1025;
/// Largest frame payload.
pub const MAX_FRAME: usize = 16 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Tag {
    Start = 1,
    Stdin = 2,
    StdinEnd = 3,
    Signal = 4,
    Sync = 5,
    Stdout = 101,
    StdoutEnd = 102,
    Exit = 103,
    Error = 104,
    Synced = 105,
}

impl Tag {
    pub fn from_u8(b: u8) -> Option<Tag> {
        use Tag::*;
        Some(match b {
            1 => Start,
            2 => Stdin,
            3 => StdinEnd,
            4 => Signal,
            5 => Sync,
            101 => Stdout,
            102 => StdoutEnd,
            103 => Exit,
            104 => Error,
            105 => Synced,
            _ => return None,
        })
    }
}

/// An NFS share of the host: the agent mounts it at the same path, or at `at`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Mount {
    pub path: String,
    #[serde(default)]
    pub read_only: bool,
    /// Where it goes in the VM when that is not the host path (the bot's home folder).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

impl Mount {
    /// The path in the VM.
    pub fn target(&self) -> &str {
        self.at.as_deref().unwrap_or(&self.path)
    }
}

/// What the agent starts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Start {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: String,
    /// The file the CLI's stderr is appended to (on a share).
    pub stderr: String,
    pub uid: u32,
    pub gid: u32,
    /// The host's address on the VM's link: the NFS server.
    pub host_ip: String,
    pub guest_ip: String,
    pub mounts: Vec<Mount>,
    /// A unix socket the agent listens on in the VM and relays to the host's `chat-run` port.
    #[serde(default)]
    pub chat_run_socket: Option<String>,
}

/// The CLI's end.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Exit {
    pub code: Option<i64>,
    pub signal: Option<i32>,
}

pub fn write_frame(w: &mut impl Write, tag: Tag, payload: &[u8]) -> std::io::Result<()> {
    let mut head = [0u8; 5];
    head[0] = tag as u8;
    head[1..].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    w.write_all(&head)?;
    w.write_all(payload)?;
    w.flush()
}

/// The next frame, or None at a clean end of the stream.
pub fn read_frame(r: &mut impl Read) -> std::io::Result<Option<(Tag, Vec<u8>)>> {
    let mut head = [0u8; 5];
    match r.read_exact(&mut head) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    parse_head(head).and_then(|(tag, len)| {
        let mut payload = vec![0u8; len];
        r.read_exact(&mut payload)?;
        Ok(Some((tag, payload)))
    })
}

fn parse_head(head: [u8; 5]) -> std::io::Result<(Tag, usize)> {
    let tag = Tag::from_u8(head[0]).ok_or_else(|| bad(format!("unknown frame {}", head[0])))?;
    let len = u32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len > MAX_FRAME {
        return Err(bad(format!("frame of {len} bytes")));
    }
    Ok((tag, len))
}

pub async fn write_frame_async(
    w: &mut (impl tokio::io::AsyncWrite + Unpin),
    tag: Tag,
    payload: &[u8],
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut buf = Vec::with_capacity(5 + payload.len());
    buf.push(tag as u8);
    buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(payload);
    w.write_all(&buf).await?;
    w.flush().await
}

pub async fn read_frame_async(
    r: &mut (impl tokio::io::AsyncRead + Unpin),
) -> std::io::Result<Option<(Tag, Vec<u8>)>> {
    use tokio::io::AsyncReadExt;
    let mut head = [0u8; 5];
    match r.read_exact(&mut head).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let (tag, len) = parse_head(head)?;
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload).await?;
    Ok(Some((tag, payload)))
}

fn bad(msg: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_end_cleanly() {
        let mut buf = vec![];
        write_frame(&mut buf, Tag::Stdin, b"hello\n").unwrap();
        write_frame(&mut buf, Tag::StdinEnd, b"").unwrap();
        let mut r = std::io::Cursor::new(buf);
        assert_eq!(
            read_frame(&mut r).unwrap(),
            Some((Tag::Stdin, b"hello\n".to_vec()))
        );
        assert_eq!(read_frame(&mut r).unwrap(), Some((Tag::StdinEnd, vec![])));
        assert_eq!(read_frame(&mut r).unwrap(), None);
    }

    #[test]
    fn unknown_tags_and_huge_frames_are_refused() {
        let mut r = std::io::Cursor::new(vec![9u8, 0, 0, 0, 0]);
        assert!(read_frame(&mut r).is_err());
        let mut r = std::io::Cursor::new(vec![2u8, 0xff, 0xff, 0xff, 0xff]);
        assert!(read_frame(&mut r).is_err());
    }

    #[tokio::test]
    async fn async_and_blocking_sides_agree() {
        let mut buf = vec![];
        write_frame_async(&mut buf, Tag::Exit, br#"{"code":0,"signal":null}"#)
            .await
            .unwrap();
        let (tag, p) = read_frame(&mut std::io::Cursor::new(buf)).unwrap().unwrap();
        assert_eq!(tag, Tag::Exit);
        let exit: Exit = serde_json::from_slice(&p).unwrap();
        assert_eq!(exit.code, Some(0));
    }
}
