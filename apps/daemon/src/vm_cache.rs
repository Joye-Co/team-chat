//! What a `[vm]` host keeps for its runs beside the topics' work lines
//! (docs/plans/usable/design-vm-state.zh.md). For now the small helpers the VM runs share.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// One lock per key in this process: runs that start together take turns (writing one
/// folder's sign-in copy).
pub async fn lock(key: &str) -> tokio::sync::OwnedMutexGuard<()> {
    static LOCKS: Mutex<Option<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = Mutex::new(None);
    let m = {
        let mut g = LOCKS.lock().unwrap_or_else(|e| e.into_inner());
        g.get_or_insert_with(HashMap::new)
            .entry(key.to_string())
            .or_default()
            .clone()
    };
    m.lock_owned().await
}

/// A member id the daemon uses as a folder name: 1-64 ASCII letters and digits.
pub fn plain_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runs_of_one_key_take_turns() {
        let a = lock("login:01BOT").await;
        // Another key is not held up.
        drop(lock("login:01OTHER").await);
        let waiting = tokio::spawn(async { drop(lock("login:01BOT").await) });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!waiting.is_finished());
        drop(a);
        waiting.await.unwrap();
    }

    #[test]
    fn a_plain_id_is_a_folder_name() {
        assert!(plain_id("01BOT"));
        assert!(!plain_id("") && !plain_id("../x") && !plain_id(&"a".repeat(65)));
    }
}
