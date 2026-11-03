//! Limits on page events (`emitEvent`) before the host forwards them.
//!
//! - A page event counts as the viewer's only when the viewer used that web view a moment
//!   before ([`recent_input`]): a click, a drag or a key press in it. The native view records
//!   the time of that input; events without it are dropped by the host.
//! - [`EventLimiter`] paces the events that remain: per 产出 version at most
//!   [`PAGE_EVENT_RATE`] per second after a burst of [`PAGE_EVENT_BURST`], and all versions
//!   together under the server's per-member limit, so page events never get `rate_limited`
//!   and never hold the group's outbox. An event over the limit waits; a later event with
//!   the same name replaces it (the latest wins).

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// How long after the viewer's last input in the web view a page event is accepted.
pub const USER_ACTION_WINDOW: Duration = Duration::from_secs(2);
/// Page events per second, per 产出 version.
pub const PAGE_EVENT_RATE: f64 = 5.0;
/// Page events a version may send at once before the rate applies.
pub const PAGE_EVENT_BURST: f64 = 3.0;
/// Page events per second for all versions together (the server allows 5 per member per
/// second with a burst of 10).
pub const TOTAL_RATE: f64 = 5.0;
pub const TOTAL_BURST: f64 = 5.0;
/// Waiting events per version (a page declares at most 20 names).
const MAX_PENDING: usize = 20;

/// True when the viewer's last input at `last` is at most [`USER_ACTION_WINDOW`] before
/// `now`.
pub fn recent_input(last: Option<Instant>, now: Instant) -> bool {
    last.and_then(|t| now.checked_duration_since(t))
        .is_some_and(|d| d <= USER_ACTION_WINDOW)
}

#[derive(Clone, Debug)]
struct Bucket {
    rate: f64,
    burst: f64,
    tokens: f64,
    last: Instant,
}

impl Bucket {
    fn new(rate: f64, burst: f64, now: Instant) -> Self {
        Self {
            rate,
            burst,
            tokens: burst,
            last: now,
        }
    }

    fn refill(&mut self, now: Instant) {
        if let Some(d) = now.checked_duration_since(self.last) {
            self.tokens = (self.tokens + d.as_secs_f64() * self.rate).min(self.burst);
            self.last = now;
        }
    }

    fn ready(&mut self, now: Instant) -> bool {
        self.refill(now);
        self.tokens >= 1.0
    }

    fn take(&mut self) {
        self.tokens -= 1.0;
    }

    fn full(&mut self, now: Instant) -> bool {
        self.refill(now);
        self.tokens >= self.burst
    }

    /// When the next token is there (`now` when one is there already).
    fn next_at(&mut self, now: Instant) -> Instant {
        self.refill(now);
        if self.tokens >= 1.0 {
            now
        } else {
            now + Duration::from_secs_f64((1.0 - self.tokens) / self.rate)
        }
    }
}

struct Slot<T> {
    bucket: Bucket,
    /// Waiting events in order, at most one per name.
    pending: Vec<(String, T)>,
}

/// Paces page events per key (an artifact id and version). See the module docs.
pub struct EventLimiter<K: Ord + Clone, T> {
    slots: BTreeMap<K, Slot<T>>,
    total: Option<Bucket>,
}

impl<K: Ord + Clone, T> Default for EventLimiter<K, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord + Clone, T> EventLimiter<K, T> {
    pub fn new() -> Self {
        Self {
            slots: BTreeMap::new(),
            total: None,
        }
    }

    /// Offers one event. Returns it when it may go now; otherwise it waits (replacing a
    /// waiting event of the same name) and the caller takes it later with
    /// [`take_due`](Self::take_due) at [`next_due`](Self::next_due).
    pub fn offer(&mut self, key: K, name: &str, value: T, now: Instant) -> Option<T> {
        self.prune(now);
        let total = self
            .total
            .get_or_insert_with(|| Bucket::new(TOTAL_RATE, TOTAL_BURST, now));
        let slot = self.slots.entry(key).or_insert_with(|| Slot {
            bucket: Bucket::new(PAGE_EVENT_RATE, PAGE_EVENT_BURST, now),
            pending: Vec::new(),
        });
        if slot.pending.is_empty() && slot.bucket.ready(now) && total.ready(now) {
            slot.bucket.take();
            total.take();
            return Some(value);
        }
        if let Some(p) = slot.pending.iter_mut().find(|(n, _)| n == name) {
            p.1 = value;
        } else {
            if slot.pending.len() >= MAX_PENDING {
                slot.pending.remove(0);
            }
            slot.pending.push((name.to_string(), value));
        }
        None
    }

    /// When the next waiting event may go, or `None` when nothing waits.
    pub fn next_due(&mut self, now: Instant) -> Option<Instant> {
        let total_at = self.total.as_mut().map(|b| b.next_at(now))?;
        self.slots
            .values_mut()
            .filter(|s| !s.pending.is_empty())
            .map(|s| s.bucket.next_at(now).max(total_at))
            .min()
    }

    /// The waiting events that may go now, oldest first per key.
    pub fn take_due(&mut self, now: Instant) -> Vec<(K, String, T)> {
        let mut out = Vec::new();
        let Some(total) = self.total.as_mut() else {
            return out;
        };
        for (key, slot) in self.slots.iter_mut() {
            while !slot.pending.is_empty() && slot.bucket.ready(now) && total.ready(now) {
                slot.bucket.take();
                total.take();
                let (name, value) = slot.pending.remove(0);
                out.push((key.clone(), name, value));
            }
        }
        self.prune(now);
        out
    }

    /// Forgets keys with nothing waiting and a full bucket.
    fn prune(&mut self, now: Instant) {
        self.slots
            .retain(|_, s| !s.pending.is_empty() || !s.bucket.full(now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn input_must_be_recent() {
        let t = Instant::now();
        assert!(!recent_input(None, t));
        assert!(recent_input(Some(t), t));
        assert!(recent_input(Some(t), t + USER_ACTION_WINDOW));
        assert!(!recent_input(Some(t), t + USER_ACTION_WINDOW + ms(1)));
        // Input recorded after the event (clock order) does not count for it.
        assert!(!recent_input(Some(t + ms(5)), t));
    }

    #[test]
    fn burst_then_five_per_second_and_the_latest_wins() {
        let t = Instant::now();
        let mut l = EventLimiter::<(&str, u32), u32>::new();
        let k = ("a1", 2);
        // The burst goes at once.
        for i in 0..3 {
            assert_eq!(l.offer(k, "pick", i, t), Some(i));
        }
        // Over the burst: events wait, and the same name keeps only the latest.
        assert_eq!(l.offer(k, "pick", 10, t), None);
        assert_eq!(l.offer(k, "pick", 11, t), None);
        assert_eq!(l.offer(k, "other", 20, t), None);
        let due = l.next_due(t).unwrap();
        assert_eq!(due, t + ms(200));
        assert!(l.take_due(t + ms(199)).is_empty());
        assert_eq!(l.take_due(due), vec![(k, "pick".to_string(), 11)]);
        let due = l.next_due(due).unwrap();
        assert_eq!(l.take_due(due), vec![(k, "other".to_string(), 20)]);
        assert_eq!(l.next_due(due), None);
    }

    #[test]
    fn a_flood_sends_at_most_the_server_limit() {
        // A page that emits every 10 ms for 10 s, in two 产出 at once.
        let t = Instant::now();
        let mut l = EventLimiter::<(&str, u32), u32>::new();
        let mut sent: Vec<Duration> = Vec::new();
        for step in 0..1000u64 {
            let now = t + ms(step * 10);
            for key in [("a1", 1), ("a2", 1)] {
                if l.offer(key, "tick", step as u32, now).is_some() {
                    sent.push(now - t);
                }
            }
            if l.next_due(now).is_some_and(|d| d <= now) {
                sent.extend(l.take_due(now).iter().map(|_| now - t));
            }
        }
        // Server bucket: 5 per second, burst 10. Replay it over what was sent.
        let (mut tokens, mut last) = (10.0f64, Duration::ZERO);
        for at in &sent {
            tokens = (tokens + (*at - last).as_secs_f64() * 5.0).min(10.0);
            last = *at;
            assert!(tokens >= 1.0, "rate_limited at {at:?}");
            tokens -= 1.0;
        }
        assert!(sent.len() <= 5 + 50 + 1, "{}", sent.len());
        // Waiting events are one per name and key, not one per call.
        assert!(l.slots.values().all(|s| s.pending.len() <= 1));
    }

    #[test]
    fn idle_keys_are_forgotten() {
        let t = Instant::now();
        let mut l = EventLimiter::<u32, ()>::new();
        for k in 0..3 {
            assert_eq!(l.offer(k, "n", (), t), Some(()));
        }
        assert_eq!(l.slots.len(), 3);
        l.offer(1000, "n", (), t + Duration::from_secs(60));
        assert_eq!(l.slots.len(), 1);
    }
}
