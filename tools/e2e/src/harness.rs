//! Members, groups and clients for scenarios. Tokens and invite codes stay in memory or in
//! files with mode 600; nothing here prints them.

use anyhow::{Context, Result, anyhow, bail};
use chat_core::proto::{GroupKind, Member, MemberKind, MemberLevel};
use chat_core::{Client, ClientConfig, ClientUpdate, ConnectionState, DropLiveEvent, HttpClient};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tokio::sync::watch;

pub struct Account {
    pub id: String,
    pub name: String,
    token: String,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl Account {
    pub fn token(&self) -> &str {
        &self.token
    }
}

pub struct Harness {
    pub base: String,
    pub run_id: String,
    owner: HttpClient,
    raw: reqwest::Client,
    data_root: PathBuf,
    created: Mutex<Vec<String>>,
    counter: AtomicU64,
}

fn read_secret(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) if !s.trim().is_empty() => Ok(Some(s.trim().to_string())),
        Ok(_) => Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow!("read {}: {e}", path.display())),
    }
}

fn write_secret(path: &Path, value: &str) -> Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    // Windows has no file modes: the user's profile folder is private already.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(path)
        .with_context(|| format!("write {}", path.display()))?;
    writeln!(f, "{value}")?;
    Ok(())
}

impl Harness {
    /// Finds an owner: `owner-token` in the secrets dir, else a bootstrap with
    /// `admin-secret` (only on a server with no member), whose token is saved there.
    pub async fn setup(base: &str, secrets_dir: &Path, data_root: PathBuf) -> Result<Harness> {
        let base = base.trim_end_matches('/').to_string();
        let raw = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        let health = raw
            .get(format!("{base}/v1/health"))
            .send()
            .await
            .map_err(|e| anyhow!("health: {}", e.without_url()))?;
        if !health.status().is_success() {
            bail!("health: status {}", health.status());
        }
        let owner_path = secrets_dir.join("owner-token");
        let token = match read_secret(&owner_path)? {
            Some(t) => t,
            None => {
                let secret = read_secret(&secrets_dir.join("admin-secret"))?.ok_or_else(|| {
                    anyhow!(
                        "no owner-token and no admin-secret in {}",
                        secrets_dir.display()
                    )
                })?;
                let res = raw
                    .post(format!("{base}/v1/admin/bootstrap"))
                    .header("x-admin-secret", secret)
                    .json(&json!({ "ownerName": "e2e owner" }))
                    .send()
                    .await
                    .map_err(|e| anyhow!("bootstrap: {}", e.without_url()))?;
                let status = res.status();
                let body: Value = res.json().await.unwrap_or(Value::Null);
                if !status.is_success() {
                    bail!(
                        "bootstrap: status {status} {}; put an owner token into {}",
                        body["error"]["code"],
                        owner_path.display()
                    );
                }
                let t = body["token"]
                    .as_str()
                    .ok_or_else(|| anyhow!("bootstrap: no token in answer"))?
                    .to_string();
                std::fs::create_dir_all(secrets_dir)?;
                write_secret(&owner_path, &t)?;
                eprintln!(
                    "bootstrapped an owner; token saved to {}",
                    owner_path.display()
                );
                t
            }
        };
        let owner = HttpClient::new(&base, Some(token))?;
        let me = owner.me().await.context("owner token")?;
        if me.level != MemberLevel::Owner {
            bail!("the token in {} is not an owner's", owner_path.display());
        }
        let run_id = ulid::Ulid::new().to_string()[20..].to_lowercase();
        std::fs::create_dir_all(&data_root)?;
        Ok(Harness {
            base,
            run_id,
            owner,
            raw,
            data_root,
            created: Mutex::new(Vec::new()),
            counter: AtomicU64::new(0),
        })
    }

    pub fn name(&self, label: &str) -> String {
        let mut n = format!("e2e-{}-{label}", self.run_id);
        n.truncate(64);
        n
    }

    /// A human member made through an invite and its redeem.
    pub async fn human(&self, label: &str) -> Result<Account> {
        let name = self.name(label);
        let inv = self
            .owner
            .create_invite(&name, MemberKind::Human, MemberLevel::Member, Some("e2e"))
            .await
            .context("create invite")?;
        let anon = HttpClient::new(&self.base, None)?;
        let r = anon.redeem(&inv.code, "e2e").await.context("redeem")?;
        self.adopt(r.member, r.token).await
    }

    pub async fn bot(&self, label: &str) -> Result<Account> {
        let name = self.name(label);
        let r = self
            .owner
            .create_bot(&name, Some("e2e"), None)
            .await
            .context("create bot")?;
        self.adopt(r.member, r.token).await
    }

    async fn adopt(&self, m: Member, token: String) -> Result<Account> {
        self.created.lock().unwrap().push(m.id.clone());
        let acc = Account {
            id: m.id,
            name: m.name,
            token,
        };
        // New members join the default groups; leave them so runs stay apart. A new person's
        // first DM with the default bot cannot be left and stays.
        let http = self.http(&acc);
        for g in http.groups().await.context("groups")? {
            if g.group.kind == GroupKind::Dm {
                continue;
            }
            http.remove_group_member(&g.group.id, &acc.id)
                .await
                .context("leave default group")?;
        }
        Ok(acc)
    }

    /// The owner's HTTP client (runner and machine setup).
    pub fn owner_http(&self) -> &HttpClient {
        &self.owner
    }

    pub fn http(&self, acc: &Account) -> HttpClient {
        HttpClient::new(&self.base, Some(acc.token.clone())).expect("base url checked in setup")
    }

    /// A group made by `creator` with `others`. Returns its id.
    pub async fn group(
        &self,
        creator: &Account,
        label: &str,
        others: &[&Account],
    ) -> Result<String> {
        let ids: Vec<String> = others.iter().map(|a| a.id.clone()).collect();
        let g = self
            .http(creator)
            .create_group(&self.name(label), &ids)
            .await
            .context("create group")?;
        Ok(g.group.id)
    }

    /// A plain HTTP call: (status, JSON body or null).
    pub async fn api(
        &self,
        method: reqwest::Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> Result<(u16, Value)> {
        let mut rb = self.raw.request(method, format!("{}{path}", self.base));
        if let Some(t) = token {
            rb = rb.bearer_auth(t);
        }
        if let Some(b) = body {
            rb = rb.json(&b);
        }
        let res = rb
            .send()
            .await
            .map_err(|e| anyhow!("{path}: {}", e.without_url()))?;
        let status = res.status().as_u16();
        Ok((status, res.json().await.unwrap_or(Value::Null)))
    }

    /// A chat-core client for `acc`. `dir` names its cache directory; the same name
    /// reopens the same cache (a restart).
    pub fn client(
        &self,
        acc: &Account,
        dir: &str,
        hook: Option<DropLiveEvent>,
    ) -> Result<TestClient> {
        let data_dir = self.data_root.join(format!("{}-{dir}", acc.id));
        let mut cfg = ClientConfig::new(&self.base, acc.token.clone(), &data_dir);
        cfg.client_name = "e2e".into();
        cfg.drop_live_event = hook;
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        TestClient::start(cfg, acc.id.clone(), format!("{dir}#{n}"))
    }

    /// Revokes the tokens of every member this run made.
    pub async fn cleanup(&self) -> Result<()> {
        let ids = std::mem::take(&mut *self.created.lock().unwrap());
        for id in ids {
            self.owner.revoke(Some(&id), true).await.context("revoke")?;
        }
        let _ = std::fs::remove_dir_all(&self.data_root);
        Ok(())
    }
}

/// Arrival time of each message from another member, by `clientMsgId`.
pub type Arrivals = Arc<Mutex<HashMap<String, Instant>>>;

pub struct TestClient {
    pub c: Arc<Client>,
    pub label: String,
    pub arrivals: Arrivals,
    changes: watch::Receiver<u64>,
    task: tokio::task::JoinHandle<()>,
}

impl TestClient {
    fn start(cfg: ClientConfig, me: String, label: String) -> Result<TestClient> {
        let c = Arc::new(Client::new(cfg)?);
        let mut rx = c.subscribe();
        let (tx, changes) = watch::channel(0u64);
        let arrivals: Arrivals = Arc::default();
        let weak: Weak<Client> = Arc::downgrade(&c);
        let a2 = arrivals.clone();
        let task = tokio::spawn(async move {
            let mut seen: HashMap<String, u64> = HashMap::new();
            while let Some(u) = rx.recv().await {
                if let ClientUpdate::MessagesChanged { group_id, topic_id } = &u
                    && let Some(c) = weak.upgrade()
                {
                    // Only messages at or below lastApplied are final: an acked send of mine
                    // can be cached before the events below it are applied.
                    let now = Instant::now();
                    let applied = c.store().last_applied(group_id).unwrap_or(0);
                    let after = seen.entry(topic_id.clone()).or_insert(0);
                    while let Ok(ms) = c.messages_after(topic_id, *after, 500) {
                        let start = *after;
                        let mut a = a2.lock().unwrap();
                        for m in ms.iter().take_while(|m| m.created_seq <= applied) {
                            *after = m.created_seq;
                            if m.author_id != me {
                                a.entry(m.client_msg_id.clone()).or_insert(now);
                            }
                        }
                        if *after == start {
                            break;
                        }
                    }
                }
                tx.send_modify(|n| *n += 1);
            }
        });
        c.start();
        Ok(TestClient {
            c,
            label,
            arrivals,
            changes,
            task,
        })
    }

    /// Polls `f` after every update (and at least every 250 ms) until it returns a value.
    pub async fn wait<T>(
        &self,
        timeout: Duration,
        what: &str,
        mut f: impl FnMut(&Client) -> Option<T>,
    ) -> Result<T> {
        let mut rx = self.changes.clone();
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(v) = f(&self.c) {
                return Ok(v);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                bail!(
                    "{}: timed out after {:?} waiting for {what} (state {:?})",
                    self.label,
                    timeout,
                    self.c.connection_state()
                );
            }
            if tokio::time::timeout(left.min(Duration::from_millis(250)), rx.changed())
                .await
                .is_ok_and(|r| r.is_err())
            {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }

    /// Waits for `welcome` and for `group_id` in the cache.
    pub async fn ready(&self, group_ids: &[&str]) -> Result<()> {
        self.wait(Duration::from_secs(20), "connected with groups", |c| {
            (c.connection_state() == ConnectionState::Connected
                && group_ids
                    .iter()
                    .all(|g| c.store().group(g).ok().flatten().is_some()))
            .then_some(())
        })
        .await
    }

    /// Every cached message of a topic, oldest first.
    pub fn messages(&self, topic_id: &str) -> Vec<chat_core::proto::Message> {
        self.c
            .messages(topic_id, None, 1_000_000)
            .unwrap_or_default()
    }

    /// Stops the engine, waits until it says so, and drops the client.
    pub async fn shutdown(self) {
        self.c.stop();
        let _ = self
            .wait(Duration::from_secs(5), "stopped", |c| {
                (c.connection_state() == ConnectionState::Stopped).then_some(())
            })
            .await;
        self.task.abort();
    }
}

/// p-th percentile (0..=100) of sorted values, nearest rank.
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// p50, p95, p99 and max in milliseconds as report metrics.
pub fn latency_metrics(mut ms: Vec<f64>) -> Value {
    ms.sort_by(|a, b| a.total_cmp(b));
    let r = |v: f64| (v * 10.0).round() / 10.0;
    json!({
        "count": ms.len(),
        "p50Ms": r(percentile(&ms, 50.0)),
        "p95Ms": r(percentile(&ms, 95.0)),
        "p99Ms": r(percentile(&ms, 99.0)),
        "maxMs": r(ms.last().copied().unwrap_or(0.0)),
    })
}
