//! The wire between the daemon and `vm-agent` in a run's microVM: one vsock stream (port
//! [`AGENT_PORT`]) of frames `[tag u8][length u32 big-endian][payload]`.
//!
//! Daemon → agent: [`Tag::Start`] (JSON [`Start`]) once, then stdin bytes, stdin end, signals and
//! sync requests. Agent → daemon: stdout bytes, stdout end, the exit status, errors and the sync
//! answer. A run's `chat-run` connects from the VM to the daemon on [`CHAT_RUN_PORT`].
//!
//! 接管 (design-takeover 3.6): the daemon stops and continues the CLI ([`Tag::Hold`]), runs
//! the person's commands ([`Tag::Exec`], [`Tag::ExecCancel`]) and hands back ([`Tag::Restore`]);
//! the agent answers with each command's output ([`Tag::ExecOut`]) and end ([`Tag::ExecExit`]).

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
    /// 接管: JSON [`Hold`]: stop or continue the CLI's process group and its tools.
    Hold = 6,
    /// 接管: JSON [`Exec`]: run one command of the person holding the run.
    Exec = 7,
    /// 接管: JSON [`ExecCancel`]: end that command's process group.
    ExecCancel = 8,
    /// 接管: hand-back; files the person's user made go back to the run's user.
    Restore = 9,
    Stdout = 101,
    StdoutEnd = 102,
    Exit = 103,
    Error = 104,
    Synced = 105,
    /// Output of a person's command: [`exec_out`].
    ExecOut = 106,
    /// JSON [`ExecExit`].
    ExecExit = 107,
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
            6 => Hold,
            7 => Exec,
            8 => ExecCancel,
            9 => Restore,
            101 => Stdout,
            102 => StdoutEnd,
            103 => Exit,
            104 => Error,
            105 => Synced,
            106 => ExecOut,
            107 => ExecExit,
            _ => return None,
        })
    }
}

/// Stop (`true`) or continue the CLI for 接管. The VM keeps running.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Hold {
    pub stop: bool,
}

/// One command of the person holding the run, run by the agent as another user than the run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Exec {
    pub id: String,
    pub argv: Vec<String>,
    /// The run's work folder.
    pub cwd: String,
    /// The whole environment (the agent adds `HOME`).
    pub env: Vec<(String, String)>,
    /// The shares the command may see (the work folder and the run's git dir); every other
    /// share (the member's home, the run folder) is hidden from it.
    #[serde(default)]
    pub open: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecCancel {
    pub id: String,
}

/// A person's command ended. Past [`EXEC_OUT_CAP`] the agent only counts (`dropped`) and
/// sends the last bytes here (`tail`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ExecExit {
    pub id: String,
    pub code: Option<i64>,
    pub ms: u64,
    #[serde(default)]
    pub dropped: u64,
    #[serde(default)]
    pub tail: String,
    /// "unsupported": the work folder could not be opened to the person's user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Output the agent sends per command before it only counts.
pub const EXEC_OUT_CAP: usize = 1 << 20;
/// Largest [`Tag::ExecOut`] chunk.
pub const EXEC_OUT_CHUNK: usize = 16 << 10;

/// [`Tag::ExecOut`] payload: `[id length u8][id][bytes]`.
pub fn exec_out(id: &str, bytes: &[u8]) -> Vec<u8> {
    let id = &id.as_bytes()[..id.len().min(255)];
    let mut p = Vec::with_capacity(1 + id.len() + bytes.len());
    p.push(id.len() as u8);
    p.extend_from_slice(id);
    p.extend_from_slice(bytes);
    p
}

/// The command id and bytes of a [`Tag::ExecOut`] payload.
pub fn parse_exec_out(p: &[u8]) -> Option<(String, &[u8])> {
    let n = *p.first()? as usize;
    let id = std::str::from_utf8(p.get(1..1 + n)?).ok()?;
    Some((id.to_string(), &p[1 + n..]))
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

    /// 接管 frames: hold, exec, cancel and restore one way, output and exit the other.
    #[test]
    fn takeover_frames_round_trip() {
        let exec = Exec {
            id: "7".into(),
            argv: vec!["/bin/bash".into(), "-c".into(), "git status".into()],
            cwd: "/w/lines/k/work".into(),
            env: vec![("TERM".into(), "dumb".into())],
            open: vec!["/w/lines/k/work".into()],
        };
        let exit = ExecExit {
            id: "7".into(),
            code: Some(1),
            ms: 12,
            dropped: 3,
            tail: "end".into(),
            error: None,
        };
        let mut buf = vec![];
        write_frame(
            &mut buf,
            Tag::Hold,
            &serde_json::to_vec(&Hold { stop: true }).unwrap(),
        )
        .unwrap();
        write_frame(&mut buf, Tag::Exec, &serde_json::to_vec(&exec).unwrap()).unwrap();
        write_frame(&mut buf, Tag::ExecCancel, br#"{"id":"7"}"#).unwrap();
        write_frame(&mut buf, Tag::Restore, b"").unwrap();
        write_frame(&mut buf, Tag::ExecOut, &exec_out("7", b"on branch main\n")).unwrap();
        write_frame(&mut buf, Tag::ExecExit, &serde_json::to_vec(&exit).unwrap()).unwrap();
        let mut r = std::io::Cursor::new(buf);
        let mut next = || read_frame(&mut r).unwrap().unwrap();
        let (t, p) = next();
        assert_eq!(t, Tag::Hold);
        assert_eq!(
            serde_json::from_slice::<Hold>(&p).unwrap(),
            Hold { stop: true }
        );
        let (t, p) = next();
        assert_eq!(t, Tag::Exec);
        assert_eq!(serde_json::from_slice::<Exec>(&p).unwrap(), exec);
        let (t, p) = next();
        assert_eq!(t, Tag::ExecCancel);
        assert_eq!(serde_json::from_slice::<ExecCancel>(&p).unwrap().id, "7");
        assert_eq!(next(), (Tag::Restore, vec![]));
        let (t, p) = next();
        assert_eq!(t, Tag::ExecOut);
        assert_eq!(
            parse_exec_out(&p),
            Some(("7".to_string(), &b"on branch main\n"[..]))
        );
        let (t, p) = next();
        assert_eq!(t, Tag::ExecExit);
        assert_eq!(serde_json::from_slice::<ExecExit>(&p).unwrap(), exit);
        // A payload shorter than its id is refused.
        assert_eq!(parse_exec_out(&[5, b'a']), None);
        assert_eq!(parse_exec_out(&[]), None);
    }

    #[test]
    fn unknown_tags_and_huge_frames_are_refused() {
        let mut r = std::io::Cursor::new(vec![10u8, 0, 0, 0, 0]);
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
