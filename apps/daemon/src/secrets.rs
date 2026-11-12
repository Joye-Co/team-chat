//! Gated secrets: variables a run gets only after a person approves (`chat-run secret`).
//!
//! `gated_env` in `daemon.toml` names variables of the daemon's environment that are never put
//! in a CLI's environment. A run whose `envAllowlist` names one may ask for it with
//! `chat-run secret <NAME>`. The first ask sends `approval.request` (the server shows an
//! approval card to the requester and the owners) and fails with `pending`; once the server
//! answers `run.approval approve`, the same command prints the value for [`GRANT`]. The daemon
//! answers `approval.resolved` itself; the CLI never sees the approval. The values are in the
//! redaction list like `env_passthrough` values. A key saved in the app with "ask before each
//! use" (机 4.3, `secret.set.ask`) is gated the same way for the AI members it is for; its value
//! comes from the machine's key store instead of the daemon's environment.

use crate::proto::RunEvent;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The `chat-run` op.
pub const OP: &str = "secret.get";
/// How long an approval lets the run read the value.
pub const GRANT: Duration = Duration::from_secs(30 * 60);
/// Approval ids of this module start with this.
const PREFIX: &str = "daemon-secret-";

#[derive(Default)]
struct State {
    /// The approval that waits for an answer: (approval id, name). One at a time: the server
    /// keeps one open approval per run.
    pending: Option<(String, String)>,
    granted: HashMap<String, Instant>,
    denied: HashSet<String>,
    next: u64,
}

/// The gated secrets of one run.
pub struct Gate {
    names: Vec<String>,
    /// 机 4.3: the values of keys saved in the app with "ask before each use"; the other names
    /// are read from the daemon's environment.
    values: HashMap<String, String>,
    state: Mutex<State>,
}

impl Gate {
    /// `names`: the gated variables this run may ask for.
    pub fn new(names: Vec<String>) -> Gate {
        Gate {
            names,
            values: HashMap::new(),
            state: Mutex::default(),
        }
    }

    /// 机 4.3: adds the app's keys that ask before each use (`SecretStore::ask_for`).
    pub fn with_values(mut self, keys: Vec<(String, String)>) -> Gate {
        for (name, value) in keys {
            if !self.names.contains(&name) {
                self.names.push(name.clone());
            }
            self.values.insert(name, value);
        }
        self
    }

    /// The gated variables of `gated_env` that the assignment allows (none for a side chat).
    pub fn for_run(gated_env: &[String], env_allowlist: &[String], aside: bool) -> Gate {
        let names = gated_env
            .iter()
            .filter(|n| !aside && env_allowlist.contains(n))
            .cloned()
            .collect();
        Gate::new(names)
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Answers `chat-run secret <name>` (the JSON answer `{ok, result?, error?}`), at `now`.
    pub fn get(
        &self,
        body: &Value,
        emit: &(dyn Fn(RunEvent) + Send + Sync),
        now: Instant,
    ) -> Value {
        let Some(name) = body["name"].as_str().filter(|n| !n.is_empty()) else {
            return err("invalid", "name is required");
        };
        if !self.names.iter().any(|n| n == name) {
            return err(
                "not_allowed",
                format!("{name} is not a secret this run may ask for"),
            );
        }
        let mut st = self.lock();
        if st.denied.contains(name) {
            return err("denied", format!("a person denied {name} for this run"));
        }
        if st.granted.get(name).is_some_and(|until| now < *until) {
            if let Some(v) = self.values.get(name) {
                return json!({ "ok": true, "result": v });
            }
            return match std::env::var(name) {
                Ok(v) if !v.is_empty() => json!({ "ok": true, "result": v }),
                _ => err("unavailable", format!("{name} is not set on this machine")),
            };
        }
        match &st.pending {
            Some((_, n)) if n == name => {
                return err(
                    "pending",
                    format!(
                        "approval for {name} is waiting for a person; run this command again after it is approved"
                    ),
                );
            }
            Some((_, other)) => {
                return err(
                    "busy",
                    format!(
                        "approval for {other} is still waiting; ask for {name} after it is answered"
                    ),
                );
            }
            None => {}
        }
        st.next += 1;
        let id = format!("{PREFIX}{}", st.next);
        st.pending = Some((id.clone(), name.to_string()));
        drop(st);
        emit(RunEvent::ApprovalRequest {
            approval_id: id,
            item_id: None,
            title: format!("使用 {name}"),
            detail: Some(format!("chat-run secret {name}")),
            kind: None,
            command: None,
        });
        err(
            "pending",
            format!(
                "asked a person to approve {name}; run this command again after it is approved"
            ),
        )
    }

    /// A secret's approval waits for a person's answer.
    pub fn waiting(&self) -> bool {
        self.lock().pending.is_some()
    }

    /// The approval id is one this module made (not the CLI's).
    pub fn owns(&self, approval_id: &str) -> bool {
        approval_id.starts_with(PREFIX)
    }

    /// A `run.approval` for this gate: records it at `now` and returns `approval.resolved`.
    /// None when it is not the approval that waits (not this gate's, or answered already).
    pub fn answer(&self, approval_id: &str, approve: bool, now: Instant) -> Option<RunEvent> {
        if !self.owns(approval_id) {
            return None;
        }
        let mut st = self.lock();
        let name = match &st.pending {
            Some((id, name)) if id == approval_id => name.clone(),
            _ => return None,
        };
        st.pending = None;
        if approve {
            st.granted.insert(name, now + GRANT);
        } else {
            st.denied.insert(name);
        }
        Some(RunEvent::ApprovalResolved {
            approval_id: approval_id.to_string(),
            decision: if approve { "approve" } else { "deny" }.into(),
        })
    }

    /// The line the CLI gets in its system prompt, when the run has gated secrets.
    pub fn note(&self) -> Option<String> {
        (!self.names.is_empty()).then(|| {
            format!(
                "Production credentials ({}) are not in your environment. When a task needs one, \
run `chat-run secret <NAME>`: the first call asks a person to approve and fails with \"pending\"; \
run it again after approval and it prints the value (for example `TOKEN=$(chat-run secret NAME) \
|| exit 1`). Never print the value or write it to a file.",
                self.names.join(", ")
            )
        })
    }
}

fn err(code: &str, message: impl Into<String>) -> Value {
    json!({ "ok": false, "error": { "code": code, "message": message.into() } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn emitted() -> (Arc<Mutex<Vec<RunEvent>>>, impl Fn(RunEvent) + Send + Sync) {
        let seen = Arc::new(Mutex::new(vec![]));
        let s = seen.clone();
        (seen, move |e| s.lock().unwrap().push(e))
    }

    #[test]
    fn only_allowed_gated_names_and_never_for_aside() {
        let gated = vec!["A".to_string(), "B".to_string()];
        let allow = vec!["B".to_string(), "C".to_string()];
        assert_eq!(Gate::for_run(&gated, &allow, false).names(), ["B"]);
        assert!(Gate::for_run(&gated, &allow, true).names().is_empty());
        assert!(Gate::for_run(&gated, &allow, true).note().is_none());
    }

    #[test]
    fn approval_then_value_until_the_grant_ends() {
        // SAFETY: tests in this module are the only readers of this variable.
        unsafe { std::env::set_var("TC_SECRET_TEST_A", "s3cret-value") };
        let gate = Gate::new(vec!["TC_SECRET_TEST_A".into()]);
        let (seen, emit) = emitted();
        let t0 = Instant::now();
        let body = json!({ "name": "TC_SECRET_TEST_A" });

        let first = gate.get(&body, &emit, t0);
        assert_eq!(first["error"]["code"], "pending");
        let id = match seen.lock().unwrap().as_slice() {
            [RunEvent::ApprovalRequest { approval_id, .. }] => approval_id.clone(),
            other => panic!("expected one approval.request, got {other:?}"),
        };
        // Asking again while it waits sends nothing new.
        assert_eq!(gate.get(&body, &emit, t0)["error"]["code"], "pending");
        assert_eq!(seen.lock().unwrap().len(), 1);

        // An approval of the CLI is not the gate's.
        assert!(gate.answer("item-7", true, t0).is_none());
        let resolved = gate.answer(&id, true, t0).expect("the gate's approval");
        assert!(
            matches!(resolved, RunEvent::ApprovalResolved { ref decision, .. } if decision == "approve")
        );

        let ok = gate.get(&body, &emit, t0 + Duration::from_secs(60));
        assert_eq!(ok, json!({ "ok": true, "result": "s3cret-value" }));

        // After the grant a new approval is needed.
        let later = gate.get(&body, &emit, t0 + GRANT + Duration::from_secs(1));
        assert_eq!(later["error"]["code"], "pending");
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[test]
    fn denied_stays_denied_and_unknown_names_are_refused() {
        let gate = Gate::new(vec!["X".into(), "Y".into()]);
        let (seen, emit) = emitted();
        let t0 = Instant::now();
        assert_eq!(
            gate.get(&json!({ "name": "Z" }), &emit, t0)["error"]["code"],
            "not_allowed"
        );
        assert_eq!(gate.get(&json!({}), &emit, t0)["error"]["code"], "invalid");

        gate.get(&json!({ "name": "X" }), &emit, t0);
        assert_eq!(
            gate.get(&json!({ "name": "Y" }), &emit, t0)["error"]["code"],
            "busy"
        );
        let id = match seen.lock().unwrap().as_slice() {
            [RunEvent::ApprovalRequest { approval_id, .. }] => approval_id.clone(),
            other => panic!("expected one approval.request, got {other:?}"),
        };
        gate.answer(&id, false, t0).unwrap();
        assert_eq!(
            gate.get(&json!({ "name": "X" }), &emit, t0)["error"]["code"],
            "denied"
        );
        // The answered approval is gone: answering it again is not the gate's any more.
        assert!(gate.answer(&id, true, t0).is_none());
        // Y can be asked now.
        assert_eq!(
            gate.get(&json!({ "name": "Y" }), &emit, t0)["error"]["code"],
            "pending"
        );
    }

    #[test]
    fn a_key_from_the_app_that_asks_first_is_given_after_approval() {
        let name = "TEAMCHAT_TEST_APP_ASK_KEY";
        let gate = Gate::for_run(&[], &[], false)
            .with_values(vec![(name.to_string(), "app-value-123".to_string())]);
        assert_eq!(gate.names(), [name]);
        assert!(gate.note().is_some_and(|n| n.contains(name)));
        let (seen, emit) = emitted();
        let t0 = Instant::now();
        let body = json!({ "name": name });
        assert_eq!(gate.get(&body, &emit, t0)["error"]["code"], "pending");
        let id = match seen.lock().unwrap().as_slice() {
            [RunEvent::ApprovalRequest { approval_id, .. }] => approval_id.clone(),
            other => panic!("expected one approval.request, got {other:?}"),
        };
        gate.answer(&id, true, t0).unwrap();
        // The value is the app's, not the daemon's environment (where it is not set).
        assert_eq!(
            gate.get(&body, &emit, t0),
            json!({ "ok": true, "result": "app-value-123" })
        );
    }
}
