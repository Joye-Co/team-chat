//! The parts of the system glue that do not touch a system API, so they are tested on
//! every platform: what a Windows toast is made of (the link a click opens, its group in
//! the notification center), the URL scheme's command, the clock
//! jump that tells a missed sleep, and which network addresses count as "the network".
//! The Windows glue ([`super::windows`]) is built on these; the macOS glue keeps its own
//! copies.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::{net::IpAddr, time::Duration};

/// One Windows toast, as plain data: what the toast's XML and its place in the
/// notification center are built from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToastSpec {
    /// The link a click on the toast opens (protocol activation): its 话题 or 群.
    pub launch: String,
    /// The notification center groups a 群's toasts under one header: its id, the 群's
    /// name and the link a click on the header opens. None without a name to show.
    pub header: Option<(String, String, String)>,
    pub title: String,
    pub body: String,
    /// Play the system's notification sound (设置 › 通用 › 声音). The system plays it, so
    /// it stays quiet while Focus Assist holds notifications back.
    pub sound: bool,
    /// The toast's group in the notification center: one per 群, so a 群 that was read
    /// takes all of its toasts out at once.
    pub group: String,
    /// Unique per toast.
    pub tag: String,
}

/// A short stable key for an id: Windows limits toast groups, tags and header ids to 64
/// characters. FNV-1a, so the key of a 群 is the same in every run of the app.
pub fn toast_key(id: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The toast of one notification. `group_name` is the 群's name ("" when unknown);
/// `unique` keeps two notifications of one 话题 apart.
pub fn toast_spec(
    notice: &super::Notice,
    group_name: &str,
    sound: bool,
    unique: &str,
) -> ToastSpec {
    use super::instance::link_to;
    let group = toast_key(&notice.group_id);
    ToastSpec {
        launch: link_to(&notice.group_id, notice.topic_id.as_deref()),
        header: (!group_name.trim().is_empty()).then(|| {
            (
                group.clone(),
                group_name.trim().to_string(),
                link_to(&notice.group_id, None),
            )
        }),
        title: notice.title.clone(),
        body: notice.body.clone(),
        sound,
        group,
        tag: toast_key(unique),
    }
}

/// The value of the URL scheme's `shell\open\command`: the app with the link as its
/// one argument.
pub fn scheme_command(exe: &std::path::Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

/// The program a `shell\open\command` value starts.
pub fn command_program(command: &str) -> Option<std::path::PathBuf> {
    let command = command.trim();
    let program = match command.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?,
        None => command.split_whitespace().next()?,
    };
    (!program.is_empty()).then(|| program.into())
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

    fn notice(topic: Option<&str>) -> super::super::Notice {
        super::super::Notice {
            title: "小王 在 产品研发".into(),
            body: "<b>看一下 & 回我</b>".into(),
            group_id: "g1".into(),
            topic_id: topic.map(str::to_string),
        }
    }

    #[test]
    fn a_toast_opens_its_topic_and_sits_under_its_group() {
        let spec = toast_spec(&notice(Some("t1")), " 产品研发 ", true, "u1");
        let scheme = env!("PRODUCT_URL_SCHEME");
        assert_eq!(spec.launch, format!("{scheme}://group/g1/topic/t1"));
        assert_eq!(
            spec.header,
            Some((
                toast_key("g1"),
                "产品研发".into(),
                format!("{scheme}://group/g1")
            ))
        );
        assert_eq!(spec.group, toast_key("g1"));
        assert!(spec.sound);
        // Two notifications of one 话题 are two toasts of one group.
        let other = toast_spec(&notice(Some("t1")), "产品研发", false, "u2");
        assert_eq!(other.group, spec.group);
        assert_ne!(other.tag, spec.tag);
        assert!(!other.sound);
        // A 群 notification opens the 群聊; without a name there is no header.
        let plain = toast_spec(&notice(None), "", true, "u3");
        assert_eq!(plain.launch, format!("{scheme}://group/g1"));
        assert_eq!(plain.header, None);
    }

    #[test]
    fn toast_keys_are_short_and_stable() {
        assert_eq!(toast_key("g1"), toast_key("g1"));
        assert_ne!(toast_key("g1"), toast_key("g2"));
        let long = toast_key(&"长".repeat(500));
        assert_eq!(long.len(), 16);
        assert!(long.chars().all(|c| c.is_ascii_hexdigit()));
        // The same value in every run and on every system.
        assert_eq!(toast_key(""), "cbf29ce484222325");
    }

    #[test]
    fn the_scheme_command_starts_the_app_with_the_link() {
        let exe = std::path::Path::new(r"C:\Users\a b\App\app.exe");
        let command = scheme_command(exe);
        assert_eq!(command, r#""C:\Users\a b\App\app.exe" "%1""#);
        assert_eq!(command_program(&command).as_deref(), Some(exe));
        assert_eq!(
            command_program(r"C:\app.exe %1").as_deref(),
            Some(std::path::Path::new(r"C:\app.exe"))
        );
        assert_eq!(command_program(""), None);
        assert_eq!(command_program("\"\""), None);
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
