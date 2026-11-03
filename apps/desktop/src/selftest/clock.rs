//! The app's wall clock. The screenshot self-test pins it to the artboards'
//! morning (2026-11-03 10:21 local), so live times (`已 3 分钟`, `10:21`,
//! `今天`) read as the artboards do and screenshots are stable. The real app
//! and the unit tests never pin it: they read the real clock.

use std::sync::atomic::{AtomicI64, Ordering};

use chrono::{DateTime, Local, TimeZone as _, Utc};

use crate::data::Millis;

/// No pin: the real clock.
const UNPINNED: i64 = i64::MIN;

static PINNED: AtomicI64 = AtomicI64::new(UNPINNED);

/// The artboards' "now": 2026-11-03 10:21 local time.
pub fn artboard_now() -> Millis {
    Local
        .with_ymd_and_hms(2026, 11, 3, 10, 21, 0)
        .earliest()
        .map(|t| t.timestamp_millis())
        .unwrap_or_default()
}

/// Pins the clock to [`artboard_now`] for this process (the screenshot
/// self-test, before it builds the sample data).
pub fn pin_to_artboards() {
    PINNED.store(artboard_now(), Ordering::Relaxed);
}

fn pinned() -> Option<Millis> {
    let ms = PINNED.load(Ordering::Relaxed);
    (ms != UNPINNED).then_some(ms)
}

/// Now, in milliseconds since the epoch.
pub fn now_ms() -> Millis {
    pinned().unwrap_or_else(|| Utc::now().timestamp_millis())
}

/// Now in local time.
pub fn local_now() -> DateTime<Local> {
    pinned()
        .and_then(|ms| Local.timestamp_millis_opt(ms).single())
        .unwrap_or_else(Local::now)
}

/// Now in UTC.
pub fn utc_now() -> DateTime<Utc> {
    pinned()
        .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
        .unwrap_or_else(Utc::now)
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike as _, Timelike as _};

    use super::*;

    #[test]
    fn the_artboard_morning_is_10_21_on_3_november_2026() {
        let t = Local
            .timestamp_millis_opt(artboard_now())
            .single()
            .expect("a local time");
        assert_eq!((t.year(), t.month(), t.day()), (2026, 11, 3));
        assert_eq!((t.hour(), t.minute()), (10, 21));
    }

    #[test]
    fn tests_read_the_real_clock() {
        // No test pins the clock, so every test sees the wall clock.
        assert!(pinned().is_none());
        let before = Utc::now().timestamp_millis();
        let now = now_ms();
        assert!(now >= before && now - before < 60_000);
    }
}
