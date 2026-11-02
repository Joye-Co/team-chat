//! Version comparison and checksum checks for the updater. Pure functions, unit-tested.

use std::{
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};

/// The version of this build (`[workspace.package] version`).
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// `MAJOR.MINOR.PATCH` as numbers. Leading zeros, pre-release tags and extra parts are
/// not release versions (docs/protocol.md, `UpdateInfo.version`).
#[cfg(test)]
pub fn parse(v: &str) -> Option<(u64, u64, u64)> {
    chat_core::proto::parse_release_version(v)
}

/// True when `candidate` is a higher release than `current`. A malformed candidate is
/// never newer; a malformed current version (a dev build) counts as older.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    chat_core::updates::is_newer(candidate, current)
}

/// The banner shows a found version unless the user postponed that same version less
/// than `for_how_long` ago. A newer version than the postponed one shows again.
pub fn postponed(
    version: &str,
    postponed: Option<&(String, Instant)>,
    now: Instant,
    for_how_long: Duration,
) -> bool {
    match postponed {
        Some((v, at)) => !is_newer(version, v) && now.saturating_duration_since(*at) < for_how_long,
        None => false,
    }
}

/// Lowercase hex SHA-256 of a file.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// True when the file's SHA-256 equals `expected` (hex, any case).
pub fn checksum_matches(path: &Path, expected: &str) -> std::io::Result<bool> {
    let expected = expected.trim();
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(false);
    }
    Ok(sha256_file(path)?.eq_ignore_ascii_case(expected))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whole percent of a download, 0..=100.
pub fn percent(done: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    ((done.min(total) as u128 * 100) / total as u128) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_compares_numbers_not_text() {
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("1.2.3", "1.2.4"));
    }

    #[test]
    fn malformed_versions() {
        // A bad candidate is never offered.
        for bad in [
            "",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.2.3-beta",
            "v1.2.3",
            " 1.2.3",
            "a.b.c",
        ] {
            assert!(!is_newer(bad, "0.0.1"), "{bad:?}");
            assert_eq!(parse(bad), None, "{bad:?}");
        }
        // A dev build with a strange version still sees releases.
        assert!(is_newer("0.1.0", "0.0.0-dev"));
        assert_eq!(parse("10.20.30"), Some((10, 20, 30)));
        assert_eq!(parse("0.0.0"), Some((0, 0, 0)));
    }

    #[test]
    fn postpone_hides_the_same_version_for_a_while() {
        let now = Instant::now();
        let day = Duration::from_secs(24 * 3600);
        let p = ("1.2.0".to_string(), now);
        assert!(postponed(
            "1.2.0",
            Some(&p),
            now + Duration::from_secs(60),
            day
        ));
        assert!(!postponed("1.2.0", Some(&p), now + day, day));
        assert!(!postponed("1.2.1", Some(&p), now, day));
        assert!(!postponed("1.2.0", None, now, day));
    }

    #[test]
    fn checksum() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.dmg");
        std::fs::write(&path, b"abc").unwrap();
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(sha256_file(&path).unwrap(), abc);
        assert!(checksum_matches(&path, abc).unwrap());
        assert!(checksum_matches(&path, &abc.to_uppercase()).unwrap());
        assert!(!checksum_matches(&path, &abc.replace('b', "c")).unwrap());
        assert!(!checksum_matches(&path, "").unwrap());
        assert!(!checksum_matches(&path, &abc[..63]).unwrap());
        assert!(!checksum_matches(&path, &format!("{}g", &abc[..63])).unwrap());
        assert!(checksum_matches(&dir.path().join("missing"), abc).is_err());
        // Bigger than one read buffer.
        let big = vec![7u8; 600 * 1024];
        std::fs::write(&path, &big).unwrap();
        let expected = hex(&Sha256::digest(&big));
        assert!(checksum_matches(&path, &expected).unwrap());
    }

    #[test]
    fn percents() {
        assert_eq!(percent(0, 0), 0);
        assert_eq!(percent(50, 200), 25);
        assert_eq!(percent(300, 200), 100);
        assert_eq!(percent(u64::MAX / 2, u64::MAX), 49);
    }
}
