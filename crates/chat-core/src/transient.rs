//! Transient state kept in memory only: typing (5 s expiry), presence, and the text of
//! streaming bot replies merged from `message.delta` events.

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

pub const TYPING_TTL: Duration = Duration::from_secs(5);

#[derive(Default, Debug)]
pub struct TransientState {
    typing: HashMap<(String, String), HashMap<String, Instant>>,
    presence: HashMap<String, bool>,
    streams: HashMap<String, String>,
}

impl TransientState {
    /// Records that a member types in a topic, at `now`.
    pub fn on_typing(&mut self, group_id: &str, topic_id: &str, member_id: &str, now: Instant) {
        self.typing
            .entry((group_id.to_string(), topic_id.to_string()))
            .or_default()
            .insert(member_id.to_string(), now + TYPING_TTL);
    }

    /// Stops showing a member as typing (for example when its message arrives).
    pub fn clear_typing(&mut self, group_id: &str, topic_id: &str, member_id: &str) -> bool {
        let key = (group_id.to_string(), topic_id.to_string());
        let removed = self
            .typing
            .get_mut(&key)
            .is_some_and(|m| m.remove(member_id).is_some());
        if self.typing.get(&key).is_some_and(|m| m.is_empty()) {
            self.typing.remove(&key);
        }
        removed
    }

    /// Removes expired entries and returns the (group, topic) pairs that changed.
    pub fn expire_typing(&mut self, now: Instant) -> BTreeSet<(String, String)> {
        let mut changed = BTreeSet::new();
        self.typing.retain(|k, members| {
            let before = members.len();
            members.retain(|_, until| *until > now);
            if members.len() != before {
                changed.insert(k.clone());
            }
            !members.is_empty()
        });
        changed
    }

    /// Members typing in a topic now, sorted.
    pub fn typing_in(&self, group_id: &str, topic_id: &str, now: Instant) -> Vec<String> {
        let mut v: Vec<String> = self
            .typing
            .get(&(group_id.to_string(), topic_id.to_string()))
            .map(|m| {
                m.iter()
                    .filter(|(_, u)| **u > now)
                    .map(|(id, _)| id.clone())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    /// Returns true when the value changed.
    pub fn set_presence(&mut self, member_id: &str, online: bool) -> bool {
        self.presence.insert(member_id.to_string(), online) != Some(online)
    }

    pub fn is_online(&self, member_id: &str) -> bool {
        self.presence.get(member_id).copied().unwrap_or(false)
    }

    pub fn online_members(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .presence
            .iter()
            .filter(|(_, o)| **o)
            .map(|(k, _)| k.clone())
            .collect();
        v.sort();
        v
    }

    /// Forgets presence (after a disconnect the server sends it again).
    pub fn clear_presence(&mut self) {
        self.presence.clear();
    }

    pub fn append_delta(&mut self, message_id: &str, append: &str) {
        self.streams
            .entry(message_id.to_string())
            .or_default()
            .push_str(append);
    }

    /// Drops the delta buffer once a stored body replaces it.
    pub fn clear_stream(&mut self, message_id: &str) -> bool {
        self.streams.remove(message_id).is_some()
    }

    /// Text appended by deltas since the last stored body.
    pub fn stream_suffix(&self, message_id: &str) -> Option<&str> {
        self.streams.get(message_id).map(String::as_str)
    }

    pub fn clear_streams(&mut self) {
        self.streams.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_expires_after_five_seconds() {
        let mut t = TransientState::default();
        let t0 = Instant::now();
        t.on_typing("g", "t", "a", t0);
        t.on_typing("g", "t", "b", t0 + Duration::from_secs(3));
        assert_eq!(
            t.typing_in("g", "t", t0 + Duration::from_secs(1)),
            vec!["a", "b"]
        );
        assert_eq!(
            t.typing_in("g", "t", t0 + Duration::from_millis(5500)),
            vec!["b"]
        );
        let changed = t.expire_typing(t0 + Duration::from_millis(5500));
        assert_eq!(changed.len(), 1);
        assert!(t.expire_typing(t0 + Duration::from_millis(6000)).is_empty());
        assert_eq!(t.expire_typing(t0 + Duration::from_secs(9)).len(), 1);
        assert!(
            t.typing_in("g", "t", t0 + Duration::from_secs(9))
                .is_empty()
        );
    }

    #[test]
    fn deltas_merge_until_cleared() {
        let mut t = TransientState::default();
        t.append_delta("m", "Hel");
        t.append_delta("m", "lo");
        assert_eq!(t.stream_suffix("m"), Some("Hello"));
        assert!(t.clear_stream("m"));
        assert_eq!(t.stream_suffix("m"), None);
    }
}
