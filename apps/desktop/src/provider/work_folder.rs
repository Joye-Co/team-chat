//! Usable (design-takeover 3.4): the work folder of a run on this computer, for 在访达中打开 /
//! 在资源管理器中打开 while I hold it. The daemon here keeps `<work_root>/runs/<runId>/run.json`;
//! the CLI worked in the work line's `work/` folder it names (`line`), or in the run's own
//! `work/` (daemon `workspace.rs`). A run on another computer has none.

use std::path::{Path, PathBuf};

/// `work_root = "…"` in the daemon config, `~` expanded; the daemon's default without one.
fn work_root_in(config: &str, home: &Path) -> PathBuf {
    let set = config.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        if k.trim() != "work_root" {
            return None;
        }
        let v = v.split('#').next()?.trim().trim_matches(['"', '\'']).trim();
        (!v.is_empty()).then(|| v.to_string())
    });
    match set {
        Some(v) if v == "~" => home.to_path_buf(),
        Some(v) if v.starts_with("~/") || (cfg!(windows) && v.starts_with("~\\")) => {
            home.join(&v[2..])
        }
        Some(v) => PathBuf::from(v),
        None => home.join(format!("{}-runs", env!("PRODUCT_SLUG"))),
    }
}

/// The folder the CLI of `run_id` works in, from its `run.json` under `work_root`.
fn work_folder_in(work_root: &Path, run_id: &str) -> Option<PathBuf> {
    // Run ids are server ids; never let one walk out of the runs folder.
    if run_id.is_empty()
        || !run_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return None;
    }
    let dir = work_root.join("runs").join(run_id);
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("run.json")).ok()?).ok()?;
    let work = match meta.get("line").and_then(|l| l.as_str()) {
        Some(line) => PathBuf::from(line).join("work"),
        None => dir.join("work"),
    };
    work.is_dir().then_some(work)
}

/// The work folder of a run on `machine_id`, when that is this computer's daemon.
pub(super) fn work_folder(machine_id: Option<&str>, run_id: &str) -> Option<PathBuf> {
    let config = std::fs::read_to_string(super::this_mac::config_path()).ok()?;
    let here = super::this_mac::machine_id_in(&config)?;
    if machine_id != Some(here.as_str()) {
        return None;
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)?;
    work_folder_in(&work_root_in(&config, &home), run_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_work_root_comes_from_the_config() {
        let home = Path::new("/home/u");
        assert_eq!(
            work_root_in("work_root = \"~/runs\"  # here\n", home),
            home.join("runs")
        );
        assert_eq!(
            work_root_in("work_root = '/srv/runs'\n", home),
            PathBuf::from("/srv/runs")
        );
        assert_eq!(
            work_root_in("name = \"m\"\n", home),
            home.join(format!("{}-runs", env!("PRODUCT_SLUG")))
        );
    }

    #[test]
    fn a_run_works_in_its_line_or_its_own_folder() {
        let root = tempfile::tempdir().unwrap();
        let line = root.path().join("lines").join("k");
        std::fs::create_dir_all(line.join("work")).unwrap();
        let r1 = root.path().join("runs").join("r1");
        std::fs::create_dir_all(&r1).unwrap();
        std::fs::write(
            r1.join("run.json"),
            serde_json::json!({ "runId": "r1", "line": line }).to_string(),
        )
        .unwrap();
        assert_eq!(work_folder_in(root.path(), "r1"), Some(line.join("work")));
        let r2 = root.path().join("runs").join("r2");
        std::fs::create_dir_all(r2.join("work")).unwrap();
        std::fs::write(r2.join("run.json"), "{\"runId\":\"r2\"}").unwrap();
        assert_eq!(work_folder_in(root.path(), "r2"), Some(r2.join("work")));
        // No run.json, or an id that walks out.
        assert_eq!(work_folder_in(root.path(), "r3"), None);
        assert_eq!(work_folder_in(root.path(), "../r1"), None);
    }
}
