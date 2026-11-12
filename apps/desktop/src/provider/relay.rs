//! 按顺序做 on chat-core (docs/plans/usable/design-relay.zh.md §3.6): 停止 and 重试 of a
//! 话题's relay, 全部停止 of a chain, and why the server refused a message as 按顺序做.

use std::{collections::HashMap, sync::Mutex};

use futures::channel::oneshot;

use super::{CoreProvider, lock};
use crate::data::{AdminError, AdminReply, Change, ClientMsgId, RelayRefusal};

/// The newest refusal per 话题: (group id, topic id) -> (the refused message, why).
#[derive(Default)]
pub(super) struct RelayCache {
    refusals: Mutex<HashMap<(String, String), (ClientMsgId, RelayRefusal)>>,
}

/// Whether a refused write is a message the server would not take as 按顺序做.
pub(super) fn is_relay_refusal(kind: &str, reason: Option<&str>) -> bool {
    kind == "send" && reason.is_some_and(|r| r.starts_with("relay."))
}

impl RelayCache {
    pub(super) fn refused(
        &self,
        group_id: &str,
        topic_id: &str,
        client_msg_id: &str,
        refusal: RelayRefusal,
    ) {
        lock(&self.refusals).insert(
            (group_id.to_string(), topic_id.to_string()),
            (client_msg_id.to_string(), refusal),
        );
    }

    /// Keeps why the server refused a send as 按顺序做. A refused first message of a new 话题
    /// has no topic id: its refusal goes under the pending 话题 the composer stays on.
    pub(super) fn refused_send(&self, f: &chat_core::SendFailure) -> bool {
        if !is_relay_refusal(f.kind, f.reason.as_deref()) {
            return false;
        }
        let topic_id = f
            .topic_id
            .clone()
            .unwrap_or_else(|| super::convert::pending_topic_id(&f.id));
        self.refused(
            &f.group_id,
            &topic_id,
            &f.id,
            RelayRefusal {
                reason: f.reason.clone().unwrap_or_default(),
                member_ids: f.member_ids.clone(),
            },
        );
        true
    }

    /// I wrote in the 话题 again: the old refusal no longer applies.
    pub(super) fn clear_topic(&self, group_id: &str, topic_id: &str) -> bool {
        lock(&self.refusals)
            .remove(&(group_id.to_string(), topic_id.to_string()))
            .is_some()
    }

    /// The refused message was retried or discarded.
    pub(super) fn clear_message(&self, client_msg_id: &str) -> Vec<(String, String)> {
        let mut map = lock(&self.refusals);
        let gone: Vec<(String, String)> = map
            .iter()
            .filter(|(_, (cid, _))| cid == client_msg_id)
            .map(|(k, _)| k.clone())
            .collect();
        for k in &gone {
            map.remove(k);
        }
        gone
    }

    pub(super) fn get(&self, group_id: &str, topic_id: &str) -> Option<RelayRefusal> {
        lock(&self.refusals)
            .get(&(group_id.to_string(), topic_id.to_string()))
            .map(|(_, r)| r.clone())
    }
}

impl CoreProvider {
    pub(super) fn relay_action_inner(
        &self,
        group_id: &str,
        topic_id: &str,
        relay_id: &str,
        retry: bool,
    ) -> AdminReply<()> {
        let (tx, rx) = oneshot::channel();
        let Some(account) = self.account() else {
            let _ = tx.send(Err(AdminError::Offline));
            return rx;
        };
        let handle = if retry {
            account.client.retry_relay(group_id, topic_id, relay_id)
        } else {
            account.client.stop_relay(group_id, topic_id, relay_id)
        };
        let this = self.clone();
        let changed = vec![Change::Topics {
            group_id: group_id.to_string(),
        }];
        account.client.spawn(async move {
            let result = match handle.await {
                Ok(r) => r.map(|_| ()),
                Err(e) => Err(chat_core::ClientError::Runtime(e.to_string())),
            };
            this.emit(changed);
            let _ = tx.send(result.map_err(|e| {
                tracing::warn!(error = %e, retry, "cannot change a relay");
                super::settings::admin_error(&e)
            }));
        });
        rx
    }

    pub(super) fn stop_chain_inner(&self, group_id: &str, chain_id: &str) -> AdminReply<()> {
        let (tx, rx) = oneshot::channel();
        let Some(account) = self.account() else {
            let _ = tx.send(Err(AdminError::Offline));
            return rx;
        };
        let handle = account.client.stop_chain(group_id, chain_id);
        account.client.spawn(async move {
            let result = match handle.await {
                Ok(r) => r,
                Err(e) => Err(chat_core::ClientError::Runtime(e.to_string())),
            };
            let _ = tx.send(match result {
                Ok(r) => {
                    tracing::info!(stopped = r.stopped_runs, "chain stopped");
                    Ok(())
                }
                Err(e) => {
                    tracing::warn!(error = %e, "cannot stop a chain");
                    Err(super::settings::admin_error(&e))
                }
            });
        });
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(who: &str) -> RelayRefusal {
        RelayRefusal {
            reason: "relay.member_unavailable".into(),
            member_ids: vec![who.into()],
        }
    }

    #[test]
    fn only_relay_reasons_of_a_send_count() {
        assert!(is_relay_refusal("send", Some("relay.running")));
        assert!(!is_relay_refusal("send", Some("card.already_answered")));
        assert!(!is_relay_refusal("send", None));
        assert!(!is_relay_refusal("edit", Some("relay.running")));
    }

    #[test]
    fn a_refusal_lasts_until_i_write_retry_or_discard() {
        let c = RelayCache::default();
        c.refused("g", "t", "c1", refusal("b1"));
        c.refused("g", "t2", "c2", refusal("b2"));
        assert_eq!(c.get("g", "t"), Some(refusal("b1")));
        // A newer refusal in the same 话题 replaces it.
        c.refused("g", "t", "c3", refusal("b3"));
        assert_eq!(c.get("g", "t"), Some(refusal("b3")));
        assert!(c.clear_message("c1").is_empty());
        assert_eq!(c.clear_message("c3"), vec![("g".into(), "t".into())]);
        assert_eq!(c.get("g", "t"), None);
        assert!(c.clear_topic("g", "t2"));
        assert!(!c.clear_topic("g", "t2"));
        assert_eq!(c.get("g", "t2"), None);
    }

    fn failure(topic_id: Option<&str>, reason: Option<&str>) -> chat_core::SendFailure {
        chat_core::SendFailure {
            id: "c9".into(),
            kind: "send",
            group_id: "g".into(),
            topic_id: topic_id.map(str::to_string),
            code: chat_core::proto::ErrorCode::Conflict,
            message: String::new(),
            reason: reason.map(str::to_string),
            member_ids: vec!["b1".into()],
        }
    }

    #[test]
    fn a_refused_order_in_a_topic_is_kept_under_it() {
        let c = RelayCache::default();
        assert!(c.refused_send(&failure(Some("t"), Some("relay.member_unavailable"))));
        assert_eq!(c.get("g", "t"), Some(refusal("b1")));
    }

    #[test]
    fn a_refused_order_starting_a_topic_is_kept_under_the_pending_topic() {
        let c = RelayCache::default();
        assert!(c.refused_send(&failure(None, Some("relay.member_unavailable"))));
        let pending = super::super::convert::pending_topic_id("c9");
        assert_eq!(c.get("g", &pending), Some(refusal("b1")));
        assert_eq!(c.clear_message("c9"), vec![("g".into(), pending)]);
    }

    #[test]
    fn other_refusals_are_not_kept() {
        let c = RelayCache::default();
        assert!(!c.refused_send(&failure(None, Some("card.already_answered"))));
        assert!(!c.refused_send(&failure(None, None)));
        assert_eq!(
            c.get("g", &super::super::convert::pending_topic_id("c9")),
            None
        );
    }
}
