//! The parts of the system glue that do not touch a system API, so they are tested on
//! every platform: the tag of a notification (which 群 and 话题 a click opens), the clock
//! jump that tells a missed sleep, and which network addresses count as "the network".
//! The Windows glue ([`super::windows`]) is built on these; the macOS glue keeps its own
//! copies.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::{net::IpAddr, time::Duration};

/// Separates the parts of a notification tag: group, topic, unique suffix.
pub const SEP: char = '\u{1f}';

/// The tag of one notification. `unique` keeps two notifications of one 话题 apart.
pub fn notice_tag(group_id: &str, topic_id: Option<&str>, unique: &str) -> String {
    format!("{group_id}{SEP}{}{SEP}{unique}", topic_id.unwrap_or(""))
}

/// The (group id, topic id) of a clicked notification; the topic is empty for a 群.
pub fn parse_tag(tag: &str) -> Option<(String, String)> {
    let mut parts = tag.split(SEP);
    let (group, topic) = (parts.next()?, parts.next()?);
    (!group.is_empty()).then(|| (group.to_string(), topic.to_string()))
}

/// True when the wall clock moved much further than the monotonic clock between two ticks
/// of the watcher: the computer slept and nothing told us.
pub fn clock_jumped(wall_gap: Duration, mono_gap: Duration, tick: Duration) -> bool {
    wall_gap > mono_gap + Duration::from_secs(10) || wall_gap > tick * 10
}

/// How an address of an interface that is up counts toward the network's identity: an IPv4
/// address by value (not link-local), a global IPv6 address only by its presence (temporary
/// addresses rotate), anything else not at all.
pub fn interface_key(name: &str, ip: IpAddr) -> Option<String> {
    match ip {
        IpAddr::V4(ip) => {
            (!ip.is_link_local() && !ip.is_loopback()).then(|| format!("{name} {ip}"))
        }
        IpAddr::V6(ip) => ((ip.segments()[0] & 0xe000) == 0x2000).then(|| format!("{name} v6")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_carry_group_and_topic() {
        let tag = notice_tag("g1", Some("t1"), "x");
        assert_eq!(parse_tag(&tag), Some(("g1".into(), "t1".into())));
        let tag = notice_tag("g1", None, "x");
        assert_eq!(parse_tag(&tag), Some(("g1".into(), String::new())));
        assert_eq!(parse_tag("garbage"), None);
        assert_eq!(parse_tag(&notice_tag("", Some("t"), "x")), None);
    }

    #[test]
    fn a_long_wall_clock_gap_is_a_missed_sleep() {
        let tick = Duration::from_secs(3);
        assert!(!clock_jumped(tick, tick, tick));
        assert!(!clock_jumped(
            Duration::from_secs(9),
            Duration::from_secs(8),
            tick
        ));
        assert!(clock_jumped(Duration::from_secs(600), tick, tick));
        assert!(clock_jumped(
            Duration::from_secs(31),
            Duration::from_secs(31),
            tick
        ));
    }

    #[test]
    fn only_routable_addresses_identify_the_network() {
        let key = |ip: &str| interface_key("eth", ip.parse().unwrap());
        assert_eq!(key("192.168.1.5").as_deref(), Some("eth 192.168.1.5"));
        assert_eq!(key("169.254.3.4"), None);
        assert_eq!(key("127.0.0.1"), None);
        assert_eq!(key("2408:8000::1").as_deref(), Some("eth v6"));
        assert_eq!(key("fe80::1"), None);
        assert_eq!(key("::1"), None);
    }
}
