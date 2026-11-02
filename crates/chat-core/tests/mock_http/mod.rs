#![allow(dead_code)]
//! In-process mock of the V1 HTTP API (docs/protocol.md): files, 产出, members admin,
//! prefs, avatars, audit, hooks, updates and diagnostics. Enough to drive the client
//! code paths; it checks the token, upload length and SHA-256 like the server.

use chat_core::proto::*;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::{Method, Request, Response, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

pub const TOKEN: &str = "tc_mock_token_for_tests";

#[derive(Default)]
pub struct HttpState {
    pub base: String,
    next: u64,
    pub files: BTreeMap<String, (FileMeta, Vec<u8>)>,
    pub artifacts: BTreeMap<String, (Artifact, Vec<ArtifactVersion>)>,
    pub members: BTreeMap<String, Member>,
    pub groups: BTreeMap<String, GroupSummary>,
    pub avatars: BTreeMap<String, Vec<u8>>,
    pub hooks: Vec<Hook>,
    pub release: Option<(UpdateInfo, Vec<u8>)>,
    pub diagnostics: Vec<Vec<u8>>,
    pub runs: BTreeMap<String, (BotRun, Vec<chat_core::proto::machine::RunLogEntry>)>,
    // Knobs.
    /// Serve file contents with the last byte flipped.
    pub corrupt_downloads: bool,
    // Observations.
    /// (method, path) of every request.
    pub requests: Vec<(String, String)>,
    /// Content-Length header of every upload PUT.
    pub upload_lengths: Vec<Option<u64>>,
}

impl HttpState {
    fn id(&mut self, p: &str) -> String {
        self.next += 1;
        format!("{p}{:025}", self.next)
    }
    pub fn count(&self, method: &str, prefix: &str) -> usize {
        self.requests
            .iter()
            .filter(|(m, p)| m == method && p.starts_with(prefix))
            .count()
    }
}

pub struct MockHttp {
    pub addr: SocketAddr,
    pub state: Arc<Mutex<HttpState>>,
}

impl MockHttp {
    pub async fn start() -> MockHttp {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(Mutex::new(HttpState {
            base: format!("http://{addr}"),
            ..Default::default()
        }));
        let st = state.clone();
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else {
                    break;
                };
                let st = st.clone();
                tokio::spawn(async move {
                    let svc = hyper::service::service_fn(move |req| {
                        let st = st.clone();
                        async move { Ok::<_, std::convert::Infallible>(handle(st, req).await) }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(hyper_util::rt::TokioIo::new(tcp), svc)
                        .await;
                });
            }
        });
        MockHttp { addr, state }
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn with<T>(&self, f: impl FnOnce(&mut HttpState) -> T) -> T {
        f(&mut self.state.lock().unwrap())
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn sha(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

fn reply(status: StatusCode, v: Value) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(v.to_string())))
        .unwrap()
}

fn ok(v: impl serde::Serialize) -> Response<Full<Bytes>> {
    reply(StatusCode::OK, serde_json::to_value(v).unwrap())
}

fn err(status: StatusCode, code: &str) -> Response<Full<Bytes>> {
    reply(status, json!({"error": {"code": code, "message": code}}))
}

fn bytes(b: Vec<u8>, mime: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", mime)
        .body(Full::new(Bytes::from(b)))
        .unwrap()
}

fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap()
}

async fn handle(st: Arc<Mutex<HttpState>>, req: Request<Incoming>) -> Response<Full<Bytes>> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("").to_string();
    let authed = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        == Some(&format!("Bearer {TOKEN}"));
    let content_length = req
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let content_type = req
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = req.into_body().collect().await.unwrap().to_bytes().to_vec();
    let mut s = st.lock().unwrap();
    s.requests.push((method.to_string(), path.clone()));
    if !authed {
        return err(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let seg: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let json_body = || serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    match (&method, seg.as_slice()) {
        (&Method::POST, ["v1", "files"]) => {
            let r: CreateFileRequest = serde_json::from_value(json_body()).unwrap();
            let id = s.id("F");
            let meta = FileMeta {
                id: id.clone(),
                group_id: r.group_id,
                topic_id: r.topic_id.map(Some),
                name: r.name,
                size: r.size,
                mime: r.mime,
                sha256: r.sha256,
                width: r.width,
                height: r.height,
                status: FileStatus::Pending,
                created_by: "me".into(),
                created_at: 1,
            };
            s.files.insert(id.clone(), (meta.clone(), Vec::new()));
            let url = format!("{}/v1/files/{id}/content", s.base);
            ok(json!({"file": meta, "uploadUrl": url}))
        }
        (&Method::PUT, ["v1", "files", id, "content"]) => {
            s.upload_lengths.push(content_length);
            let Some((meta, data)) = s.files.get_mut(*id) else {
                return err(StatusCode::NOT_FOUND, "not_found");
            };
            if meta.status == FileStatus::Ready {
                return err(StatusCode::CONFLICT, "conflict");
            }
            if body.len() as u64 != meta.size || sha(&body) != meta.sha256 {
                return err(StatusCode::BAD_REQUEST, "invalid");
            }
            meta.status = FileStatus::Ready;
            *data = body;
            ok(meta.clone())
        }
        (&Method::GET, ["v1", "files", id]) => match s.files.get(*id) {
            Some((m, _)) => ok(m),
            None => err(StatusCode::NOT_FOUND, "not_found"),
        },
        (&Method::GET, ["v1", "files", id, "content"]) => {
            let corrupt = s.corrupt_downloads;
            match s.files.get(*id) {
                Some((m, d)) if m.status == FileStatus::Ready => {
                    let mut d = d.clone();
                    if corrupt && let Some(b) = d.last_mut() {
                        *b ^= 1;
                    }
                    bytes(d, &m.mime.clone())
                }
                _ => err(StatusCode::NOT_FOUND, "not_found"),
            }
        }
        (&Method::POST, ["v1", "artifacts"]) => {
            let r: CreateArtifactRequest = serde_json::from_value(json_body()).unwrap();
            let id = s.id("A");
            let files = match version_files(&s, &r.files) {
                Some(f) => f,
                None => return err(StatusCode::BAD_REQUEST, "invalid"),
            };
            let a = Artifact {
                id: id.clone(),
                group_id: r.group_id,
                topic_id: r.topic_id,
                title: r.title,
                kind: r.kind,
                current_version: 1,
                created_by: "me".into(),
                created_at: 10,
                updated_at: 10,
            };
            let v = ArtifactVersion {
                artifact_id: id.clone(),
                version: 1,
                entry: r.entry,
                files,
                note: r.note,
                created_by: "me".into(),
                created_at: 10,
            };
            s.artifacts.insert(id, (a.clone(), vec![v.clone()]));
            ok(ArtifactVersionResponse {
                artifact: a,
                version: v,
            })
        }
        (&Method::POST, ["v1", "artifacts", id, "versions"]) => {
            let r: AddArtifactVersionRequest = serde_json::from_value(json_body()).unwrap();
            let Some(files) = version_files(&s, &r.files) else {
                return err(StatusCode::BAD_REQUEST, "invalid");
            };
            let Some((a, vs)) = s.artifacts.get_mut(*id) else {
                return err(StatusCode::NOT_FOUND, "not_found");
            };
            a.current_version += 1;
            a.updated_at += 1;
            if let Some(t) = r.title {
                a.title = t;
            }
            let v = ArtifactVersion {
                artifact_id: a.id.clone(),
                version: a.current_version,
                entry: r.entry,
                files,
                note: r.note,
                created_by: "me".into(),
                created_at: a.updated_at,
            };
            vs.insert(0, v.clone());
            ok(ArtifactVersionResponse {
                artifact: a.clone(),
                version: v,
            })
        }
        (&Method::GET, ["v1", "artifacts"]) => {
            let q: BTreeMap<String, String> = query
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .map(|(k, v)| (k.to_string(), unescape(v)))
                .collect();
            let list: Vec<&Artifact> = s
                .artifacts
                .values()
                .map(|(a, _)| a)
                .filter(|a| Some(&a.group_id) == q.get("groupId"))
                .filter(|a| q.get("topicId").is_none_or(|t| *t == a.topic_id))
                .collect();
            ok(list)
        }
        (&Method::GET, ["v1", "artifacts", id]) => match s.artifacts.get(*id) {
            Some((a, vs)) => ok(ArtifactResponse {
                artifact: a.clone(),
                versions: vs.clone(),
            }),
            None => err(StatusCode::NOT_FOUND, "not_found"),
        },
        (&Method::GET, ["v1", "artifacts", id, "versions", v, "files", rest @ ..]) => {
            let p = unescape(&rest.join("/"));
            let v: u32 = v.parse().unwrap();
            let Some((_, vs)) = s.artifacts.get(*id) else {
                return err(StatusCode::NOT_FOUND, "not_found");
            };
            let Some(f) = vs
                .iter()
                .find(|x| x.version == v)
                .and_then(|x| x.files.iter().find(|f| f.path == p))
            else {
                return err(StatusCode::NOT_FOUND, "not_found");
            };
            let data = s.files[&f.file_id].1.clone();
            bytes(data, &f.mime)
        }
        (&Method::PATCH, ["v1", "groups", id, "prefs"]) => {
            let r: UpdateGroupPrefsRequest = serde_json::from_value(json_body()).unwrap();
            let Some(g) = s.groups.get_mut(*id) else {
                return err(StatusCode::NOT_FOUND, "not_found");
            };
            if let Some(m) = r.muted {
                g.muted = Some(m);
            }
            if let Some(n) = r.notify {
                g.notify = Some(n);
            }
            ok(g.clone())
        }
        (&Method::PATCH, ["v1", "groups", id]) => {
            let r: UpdateGroupRequest = serde_json::from_value(json_body()).unwrap();
            let Some(g) = s.groups.get_mut(*id) else {
                return err(StatusCode::NOT_FOUND, "not_found");
            };
            if g.group.kind == GroupKind::Dm {
                return err(StatusCode::FORBIDDEN, "forbidden");
            }
            g.group.name = r.name.split_whitespace().collect::<Vec<_>>().join(" ");
            ok(g.clone())
        }
        (&Method::PATCH, ["v1", "members", id]) => {
            let r: UpdateMemberRequest = serde_json::from_value(json_body()).unwrap();
            let Some(m) = s.members.get_mut(*id) else {
                return err(StatusCode::NOT_FOUND, "not_found");
            };
            if let Some(n) = r.name {
                m.name = n;
            }
            if let Some(role) = r.role {
                m.role = Some(role);
            }
            if let Some(l) = r.level {
                m.level = l;
            }
            if let Some(d) = r.deactivated {
                m.deactivated = Some(d);
            }
            ok(m.clone())
        }
        (&Method::PUT, ["v1", "members", id, "avatar"]) => {
            if !content_type.starts_with("image/") {
                return err(StatusCode::BAD_REQUEST, "invalid");
            }
            let id = id.to_string();
            s.avatars.insert(id.clone(), body);
            let n = s.avatars.len();
            let url = format!("{}/v1/members/{id}/avatar?v={n}", s.base);
            let Some(m) = s.members.get_mut(&id) else {
                return err(StatusCode::NOT_FOUND, "not_found");
            };
            m.avatar_url = Some(Some(url));
            ok(m.clone())
        }
        (&Method::GET, ["v1", "members", id, "avatar"]) => match s.avatars.get(*id) {
            Some(b) => bytes(b.clone(), "image/png"),
            None => err(StatusCode::NOT_FOUND, "not_found"),
        },
        (&Method::GET, ["v1", "audit"]) => ok(json!({
            "entries": [{"id": "E1", "at": 5, "actorId": "me", "credential": {"kind": "token", "id": "K1"}, "action": "member.update", "target": {"kind": "member", "id": "M1"}, "details": {"level": ["member", "owner"]}}],
            "hasMore": query.contains("before="),
        })),
        (&Method::POST, ["v1", "hooks"]) => {
            let r: CreateHookRequest = serde_json::from_value(json_body()).unwrap();
            let id = s.id("H");
            let hook = Hook {
                id: id.clone(),
                name: r.name,
                kind: r.kind,
                group_id: r.group_id,
                topic_id: r.topic_id.map(Some),
                member_id: s.id("B"),
                created_by: "me".into(),
                created_at: 1,
                last_used_at: None,
            };
            s.hooks.push(hook.clone());
            let url = format!("{}/v1/hooks/{id}/in", s.base);
            ok(json!({"hook": hook, "url": url, "secret": format!("whk_{}", "s".repeat(43))}))
        }
        (&Method::GET, ["v1", "hooks"]) => ok(s.hooks.clone()),
        (&Method::DELETE, ["v1", "hooks", id]) => {
            let id = id.to_string();
            s.hooks.retain(|h| h.id != id);
            ok(json!({"ok": true}))
        }
        (&Method::GET, ["v1", "runs", id]) => match s.runs.get(*id) {
            Some((r, _)) => ok(r),
            None => err(StatusCode::NOT_FOUND, "not_found"),
        },
        (&Method::PUT, ["v1", "runs", id, "plan"]) => {
            let r: chat_core::proto::SetRunPlanRequest =
                serde_json::from_value(json_body()).unwrap();
            match s.runs.get_mut(*id) {
                Some((run, _)) => {
                    run.plan = (!r.plan.is_empty()).then_some(r.plan);
                    run.updated_at += 1;
                    ok(&*run)
                }
                None => err(StatusCode::NOT_FOUND, "not_found"),
            }
        }
        (&Method::GET, ["v1", "runs", id, "events"]) => {
            let q: BTreeMap<String, u64> = query
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .filter_map(|(k, v)| Some((k.to_string(), v.parse().ok()?)))
                .collect();
            let after = q.get("afterSeq").copied().unwrap_or(0);
            let limit = q.get("limit").copied().unwrap_or(200) as usize;
            let Some((r, log)) = s.runs.get(*id) else {
                return err(StatusCode::NOT_FOUND, "not_found");
            };
            let seq = |e: &chat_core::proto::machine::RunLogEntry| chat_core::runs::entry_seq(e);
            let rest: Vec<_> = log.iter().filter(|e| seq(e) > after).cloned().collect();
            let page: Vec<_> = rest.iter().take(limit).cloned().collect();
            let last = log.iter().map(seq).max().unwrap_or(0);
            ok(json!({"run": r, "entries": page, "hasMore": rest.len() > limit, "lastSeq": last}))
        }
        (&Method::GET, ["v1", "updates", "latest"]) => match &s.release {
            Some((u, _)) => ok(u),
            None => err(StatusCode::NOT_FOUND, "not_found"),
        },
        (&Method::GET, ["v1", "updates", v, "download"]) => match &s.release {
            Some((u, b)) if u.version == *v => bytes(b.clone(), "application/x-apple-diskimage"),
            _ => err(StatusCode::NOT_FOUND, "not_found"),
        },
        (&Method::POST, ["v1", "diagnostics"]) => {
            if content_type != "application/gzip" {
                return err(StatusCode::BAD_REQUEST, "invalid");
            }
            if body.len() as u64 > MAX_DIAGNOSTICS_BYTES {
                return err(StatusCode::PAYLOAD_TOO_LARGE, "too_large");
            }
            s.diagnostics.push(body);
            let id = s.id("D");
            ok(json!({"id": id}))
        }
        _ => err(StatusCode::NOT_FOUND, "not_found"),
    }
}

fn version_files(s: &HttpState, input: &[VersionFileInput]) -> Option<Vec<ArtifactFile>> {
    input
        .iter()
        .map(|f| {
            let (m, _) = s.files.get(&f.file_id)?;
            (m.status == FileStatus::Ready).then(|| ArtifactFile {
                path: f.path.clone(),
                file_id: f.file_id.clone(),
                size: m.size,
                mime: m.mime.clone(),
            })
        })
        .collect()
}
