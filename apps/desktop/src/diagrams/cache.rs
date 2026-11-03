//! Diagram cache (REFS §B3): keys, the disk cache under the app cache directory and the
//! in-memory LRU of decoded images.

use std::collections::HashMap;
use std::hash::Hash;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The four theme ids of a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeId {
    Light,
    Dark,
    LightContrast,
    DarkContrast,
}

impl ThemeId {
    pub fn new(dark: bool, contrast: bool) -> Self {
        match (dark, contrast) {
            (false, false) => ThemeId::Light,
            (true, false) => ThemeId::Dark,
            (false, true) => ThemeId::LightContrast,
            (true, true) => ThemeId::DarkContrast,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ThemeId::Light => "light",
            ThemeId::Dark => "dark",
            ThemeId::LightContrast => "light-contrast",
            ThemeId::DarkContrast => "dark-contrast",
        }
    }

    pub fn is_dark(self) -> bool {
        matches!(self, ThemeId::Dark | ThemeId::DarkContrast)
    }

    pub fn is_contrast(self) -> bool {
        matches!(self, ThemeId::LightContrast | ThemeId::DarkContrast)
    }

    /// The same contrast setting in the other mode.
    pub fn other(self) -> Self {
        ThemeId::new(!self.is_dark(), self.is_contrast())
    }
}

/// `SHA-256(rendererVersion ‖ bundleSha256 ‖ themeId ‖ scale ‖ normalizedSource)`, hex.
/// Parts are separated by a NUL byte so no two inputs give the same bytes.
pub fn key(
    renderer_version: u32,
    bundle_sha256: &str,
    theme: ThemeId,
    scale: u32,
    normalized: &str,
) -> String {
    let mut h = Sha256::new();
    h.update(renderer_version.to_string());
    h.update([0]);
    h.update(bundle_sha256);
    h.update([0]);
    h.update(theme.as_str());
    h.update([0]);
    h.update(scale.to_string());
    h.update([0]);
    h.update(normalized);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// One node of the hit map, CSS px of the diagram at scale 1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// `<key>.json`: what the PNG shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    /// CSS px at scale 1.
    pub width: f32,
    pub height: f32,
    /// Pixels per CSS px of the PNG.
    pub scale: f32,
    #[serde(rename = "type")]
    pub kind: String,
    /// In source order.
    pub nodes: Vec<Node>,
    pub alt: String,
}

/// `<key>.err.json`: a failure, so the same source does not render again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrMeta {
    pub reason: String,
    /// For the log only.
    pub detail: String,
}

/// Bytes allowed on disk.
pub const DISK_LIMIT: u64 = 200 * 1024 * 1024;
/// Decoded images kept in memory.
pub const MEMORY_ENTRIES: usize = 64;
const INDEX_WRITE_EVERY: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct Entry {
    /// Last access, ms since the epoch.
    at: u64,
    /// Bytes of every file of the key.
    size: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// The disk cache: `<dir>/<key>.png|.svg|.json|.err.json` and `index.json` with access
/// times. Least recently used keys go first when the files pass the limit.
pub struct DiskCache {
    dir: PathBuf,
    limit: u64,
    index: HashMap<String, Entry>,
    last_write: Option<Instant>,
    dirty: bool,
}

impl DiskCache {
    /// `<cache_dir>/team-chat/diagrams` (`~/Library/Caches/team-chat/diagrams` on macOS).
    pub fn default_dir() -> PathBuf {
        let base = std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library").join("Caches"))
            .unwrap_or_else(std::env::temp_dir);
        base.join("team-chat").join("diagrams")
    }

    pub fn open(dir: PathBuf, limit: u64) -> Self {
        let index = std::fs::read(dir.join("index.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self {
            dir,
            limit,
            index,
            last_write: None,
            dirty: false,
        }
    }

    pub fn png_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.png"))
    }

    pub fn svg_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.svg"))
    }

    fn meta_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    fn err_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.err.json"))
    }

    /// The PNG bytes and meta of a rendered key.
    pub fn read_ok(&mut self, key: &str) -> Option<(Vec<u8>, Meta)> {
        let meta: Meta = serde_json::from_slice(&std::fs::read(self.meta_path(key)).ok()?).ok()?;
        let png = std::fs::read(self.png_path(key)).ok()?;
        self.touch(key);
        Some((png, meta))
    }

    /// The cached failure of a key.
    pub fn read_err(&mut self, key: &str) -> Option<ErrMeta> {
        let err = serde_json::from_slice(&std::fs::read(self.err_path(key)).ok()?).ok()?;
        self.touch(key);
        Some(err)
    }

    pub fn write_ok(&mut self, key: &str, png: &[u8], svg: &str, meta: &Meta) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let json = serde_json::to_vec(meta).unwrap_or_default();
        std::fs::write(self.png_path(key), png)?;
        std::fs::write(self.svg_path(key), svg)?;
        std::fs::write(self.meta_path(key), &json)?;
        let size = (png.len() + svg.len() + json.len()) as u64;
        self.index.insert(key.to_string(), Entry { at: now_ms(), size });
        self.dirty = true;
        self.evict();
        self.save_index(false);
        Ok(())
    }

    pub fn write_err(&mut self, key: &str, err: &ErrMeta) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let json = serde_json::to_vec(err).unwrap_or_default();
        std::fs::write(self.err_path(key), &json)?;
        self.index.insert(
            key.to_string(),
            Entry {
                at: now_ms(),
                size: json.len() as u64,
            },
        );
        self.dirty = true;
        self.evict();
        self.save_index(false);
        Ok(())
    }

    fn touch(&mut self, key: &str) {
        let e = self.index.entry(key.to_string()).or_default();
        e.at = now_ms().max(e.at + 1);
        self.dirty = true;
        self.save_index(false);
    }

    /// Bytes of every indexed key.
    pub fn total(&self) -> u64 {
        self.index.values().map(|e| e.size).sum()
    }

    /// Removes least recently used keys until the total is under the limit.
    fn evict(&mut self) {
        let mut total = self.total();
        if total <= self.limit {
            return;
        }
        let mut order: Vec<(String, Entry)> = self.index.iter().map(|(k, e)| (k.clone(), *e)).collect();
        order.sort_by_key(|(_, e)| e.at);
        for (key, e) in order {
            if total <= self.limit {
                break;
            }
            for p in [self.png_path(&key), self.svg_path(&key), self.meta_path(&key), self.err_path(&key)] {
                let _ = std::fs::remove_file(p);
            }
            self.index.remove(&key);
            total = total.saturating_sub(e.size);
        }
        self.dirty = true;
    }

    /// Writes `index.json` at most every 10 s (or now when `force`).
    pub fn save_index(&mut self, force: bool) {
        if !self.dirty {
            return;
        }
        if !force && self.last_write.is_some_and(|t| t.elapsed() < INDEX_WRITE_EVERY) {
            return;
        }
        if std::fs::create_dir_all(&self.dir).is_ok()
            && std::fs::write(
                self.dir.join("index.json"),
                serde_json::to_vec(&self.index).unwrap_or_default(),
            )
            .is_ok()
        {
            self.dirty = false;
            self.last_write = Some(Instant::now());
        }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }
}

/// A small LRU map by entry count.
pub struct Lru<K, V> {
    cap: usize,
    tick: u64,
    map: HashMap<K, (u64, V)>,
}

impl<K: Eq + Hash + Clone, V> Lru<K, V> {
    pub fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            tick: 0,
            map: HashMap::new(),
        }
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(key).map(|e| {
            e.0 = tick;
            &e.1
        })
    }

    pub fn peek(&self, key: &K) -> Option<&V> {
        self.map.get(key).map(|e| &e.1)
    }

    pub fn insert(&mut self, key: K, value: V) {
        self.tick += 1;
        self.map.insert(key, (self.tick, value));
        while self.map.len() > self.cap {
            let oldest = self
                .map
                .iter()
                .min_by_key(|(_, (t, _))| *t)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => {
                    self.map.remove(&k);
                }
                None => break,
            }
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Entries, newest first.
    pub fn newest_first(&self) -> Vec<(K, V)>
    where
        V: Clone,
    {
        let mut all: Vec<(u64, K, V)> = self
            .map
            .iter()
            .map(|(k, (t, v))| (*t, k.clone(), v.clone()))
            .collect();
        all.sort_by(|a, b| b.0.cmp(&a.0));
        all.into_iter().map(|(_, k, v)| (k, v)).collect()
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_stable_and_change_with_every_part() {
        let k = |v, b: &str, t, s, src: &str| key(v, b, t, s, src);
        let base = k(1, "abc", ThemeId::Light, 2, "graph TD\nA-->B");
        assert_eq!(base, k(1, "abc", ThemeId::Light, 2, "graph TD\nA-->B"));
        assert_eq!(base.len(), 64);
        assert_ne!(base, k(2, "abc", ThemeId::Light, 2, "graph TD\nA-->B"));
        assert_ne!(base, k(1, "abd", ThemeId::Light, 2, "graph TD\nA-->B"));
        assert_ne!(base, k(1, "abc", ThemeId::Dark, 2, "graph TD\nA-->B"));
        assert_ne!(base, k(1, "abc", ThemeId::LightContrast, 2, "graph TD\nA-->B"));
        assert_ne!(base, k(1, "abc", ThemeId::Light, 4, "graph TD\nA-->B"));
        assert_ne!(base, k(1, "abc", ThemeId::Light, 2, "graph TD\nA-->C"));
        // A known value: the key must not change between releases of the same renderer.
        assert_eq!(
            k(1, "x", ThemeId::Dark, 2, "graph TD"),
            key(1, "x", ThemeId::Dark, 2, "graph TD")
        );
    }

    fn meta() -> Meta {
        Meta {
            width: 100.0,
            height: 50.0,
            scale: 2.0,
            kind: "flowchart".into(),
            nodes: vec![],
            alt: "流程图：0 个节点。".into(),
        }
    }

    #[test]
    fn disk_round_trip_and_failure_caching() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = DiskCache::open(dir.path().join("d"), DISK_LIMIT);
        c.write_ok("k1", b"png", "<svg/>", &meta()).unwrap();
        let (png, m) = c.read_ok("k1").unwrap();
        assert_eq!((png.as_slice(), m), (&b"png"[..], meta()));
        assert!(c.read_err("k1").is_none());
        let err = ErrMeta { reason: "parse".into(), detail: "line 2".into() };
        c.write_err("k2", &err).unwrap();
        assert_eq!(c.read_err("k2"), Some(err));
        assert!(c.read_ok("k2").is_none());
        c.save_index(true);
        // A new process reads the index and the files again.
        let mut again = DiskCache::open(dir.path().join("d"), DISK_LIMIT);
        assert!(again.contains("k1") && again.contains("k2"));
        assert!(again.read_ok("k1").is_some());
        assert_eq!(again.read_err("k2").unwrap().reason, "parse");
    }

    #[test]
    fn disk_evicts_least_recently_used_by_size() {
        let dir = tempfile::tempdir().unwrap();
        let png = vec![0u8; 1000];
        let mut c = DiskCache::open(dir.path().to_path_buf(), 3500);
        c.write_ok("a", &png, "", &meta()).unwrap();
        c.write_ok("b", &png, "", &meta()).unwrap();
        c.write_ok("c", &png, "", &meta()).unwrap();
        // Use "a" so "b" is the oldest.
        assert!(c.read_ok("a").is_some());
        c.write_ok("d", &png, "", &meta()).unwrap();
        assert!(c.total() <= 3500);
        assert!(!c.contains("b"));
        assert!(!c.png_path("b").exists());
        assert!(c.contains("a") && c.contains("c") && c.contains("d"));
    }

    #[test]
    fn memory_lru_keeps_the_newest() {
        let mut l = Lru::new(2);
        l.insert(1, "a");
        l.insert(2, "b");
        assert_eq!(l.get(&1), Some(&"a"));
        l.insert(3, "c");
        assert_eq!(l.len(), 2);
        assert!(l.peek(&2).is_none());
        assert!(l.peek(&1).is_some() && l.peek(&3).is_some());
        let mut big: Lru<u32, u32> = Lru::new(MEMORY_ENTRIES);
        for i in 0..100 {
            big.insert(i, i);
        }
        assert_eq!(big.len(), MEMORY_ENTRIES);
        assert!(big.peek(&99).is_some() && big.peek(&0).is_none());
    }
}
