//! The kept home folder of each AI member on a `[vm]` host (机 3.3).
//!
//! `<homes>/<member id>` is that member's `$HOME` in every VM it runs in, and holds its CLIs'
//! folders (`.claude`, `.codex`, `.pi/agent`: session records stay per member, see
//! [`crate::vm_login`]). With `[vm].home_gb` > 0 (default 20) the folder is a fixed-size ext4
//! disk image in `<homes>/../home-images/<member id>.ext4`, made and mounted by the root helper
//! (`runner-vm home-up`), so one member cannot fill the host. A home folder from before the
//! images (a plain folder) moves into its image the first time it is used. `home_gb = 0` keeps
//! plain folders without a limit. Without `homes` the CLIs' folders are kept in
//! `<[vm].dir>/cli/<member id>` and the rest of `$HOME` goes with the VM.
//!
//! The daemon reports each image's size and the bytes it takes on the host disk
//! (`MachineMetrics.homes`), and empties a member's home on `home.clear`.

use crate::config::VmConfig;
use crate::proto::HomeUse;
use std::path::{Path, PathBuf};

/// A member id the daemon uses as a folder name: 1-64 ASCII letters and digits.
pub fn plain_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Whether homes are disk images.
pub fn images_on(vm: &VmConfig) -> bool {
    !vm.homes.as_os_str().is_empty() && vm.home_gb > 0
}

/// Where the root helper keeps the home images: `home-images` next to `homes`.
pub fn images_dir(vm: &VmConfig) -> Option<PathBuf> {
    images_on(vm).then(|| {
        vm.homes
            .parent()
            .unwrap_or(Path::new("/"))
            .join("home-images")
    })
}

/// The folder that holds a member's CLI folders on this host: its home folder, or without
/// homes `<dir>/cli/<member>`. None for an id that is not a plain folder name.
pub fn member_dir(vm: &VmConfig, bot: &str) -> Option<PathBuf> {
    if !plain_id(bot) {
        return None;
    }
    Some(if vm.homes.as_os_str().is_empty() {
        vm.dir.join("cli").join(bot)
    } else {
        vm.homes.join(bot)
    })
}

/// Makes the member's folder ready: mounts its home image (made on first use) or makes the
/// plain folder. Returns the folder.
pub async fn ensure(vm: &VmConfig, bot: &str) -> Result<PathBuf, String> {
    let dir = member_dir(vm, bot).ok_or("the AI member's id is not a folder name")?;
    if images_on(vm) {
        let homes = vm.homes.to_string_lossy().to_string();
        let gb = vm.home_gb.to_string();
        helper(vm, &["home-up", &homes, bot, &gb]).await?;
    } else {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    if !dir.is_dir() {
        return Err(format!("{} is not there", dir.display()));
    }
    Ok(dir)
}

/// Empties a member's home: the image and its mount go, the next run starts with a new one.
pub async fn clear(vm: &VmConfig, bot: &str) -> Result<(), String> {
    let dir = member_dir(vm, bot).ok_or("the AI member's id is not a folder name")?;
    if images_on(vm) {
        let homes = vm.homes.to_string_lossy().to_string();
        helper(vm, &["home-clear", &homes, bot]).await
    } else {
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{}: {e}", dir.display())),
        }
    }
}

#[cfg(unix)]
async fn helper(vm: &VmConfig, args: &[&str]) -> Result<(), String> {
    crate::executor::firecracker::helper(vm, args).await
}

#[cfg(not(unix))]
async fn helper(_vm: &VmConfig, _args: &[&str]) -> Result<(), String> {
    Err("[vm] runs only on Linux".into())
}

/// Each member's home image: its size and the bytes it takes on the host disk. The images are
/// sparse and mounted with `discard`, so what a member deletes is given back.
pub fn usage(vm: &VmConfig) -> Vec<HomeUse> {
    let Some(dir) = images_dir(vm) else {
        return vec![];
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    let mut out: Vec<HomeUse> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let id = name.strip_suffix(".ext4")?;
            if !plain_id(id) {
                return None;
            }
            let m = e.metadata().ok()?;
            Some(HomeUse {
                member_id: id.to_string(),
                used_bytes: allocated(&m),
                size_bytes: Some(m.len()),
            })
        })
        .collect();
    out.sort_by(|a, b| a.member_id.cmp(&b.member_id));
    out.truncate(crate::proto::MAX_HOMES);
    out
}

#[cfg(unix)]
fn allocated(m: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    m.blocks() * 512
}

#[cfg(not(unix))]
fn allocated(m: &std::fs::Metadata) -> u64 {
    m.len()
}

/// The disk the homes are on (their images, or the plain folders): `(free, total)`, or None
/// without kept homes.
pub fn disk(vm: &VmConfig) -> Option<(u64, u64)> {
    let at =
        images_dir(vm).or_else(|| (!vm.homes.as_os_str().is_empty()).then(|| vm.homes.clone()))?;
    Some(crate::health::disk(&at))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vm(homes: &str, gb: u32, dir: &str) -> VmConfig {
        let mut c = crate::config::Config::parse(&format!(
            "server_url = \"http://127.0.0.1:1\"\ntoken_file = \"/nonexistent\"\n[runtimes.claude]\nkind = \"claude\"\n[vm]\nkernel = \"/k\"\nrootfs = \"/r\"\ndir = '{dir}'\nhomes = '{homes}'\nhome_gb = {gb}\n"
        ))
        .unwrap();
        c.vm.take().unwrap()
    }

    #[test]
    fn a_member_dir_is_its_home_or_a_cli_folder() {
        let v = vm("/srv/runner/homes", 20, "/srv/runner/vms");
        assert_eq!(
            member_dir(&v, "01BOT"),
            Some(PathBuf::from("/srv/runner/homes/01BOT"))
        );
        assert_eq!(member_dir(&v, "../x"), None);
        assert_eq!(member_dir(&v, ""), None);
        assert!(images_on(&v));
        assert_eq!(
            images_dir(&v),
            Some(PathBuf::from("/srv/runner/home-images"))
        );
        let plain = vm("/srv/runner/homes", 0, "/srv/runner/vms");
        assert!(!images_on(&plain) && images_dir(&plain).is_none());
        let none = vm("", 20, "/srv/runner/vms");
        assert!(!images_on(&none));
        assert_eq!(
            member_dir(&none, "01BOT"),
            Some(PathBuf::from("/srv/runner/vms/cli/01BOT"))
        );
        assert!(disk(&none).is_none());
    }

    #[tokio::test]
    async fn plain_folders_are_made_and_emptied_without_the_helper() {
        let tmp = tempfile::tempdir().unwrap();
        let homes = tmp.path().join("homes");
        let v = vm(
            &homes.to_string_lossy(),
            0,
            &tmp.path().join("vms").to_string_lossy(),
        );
        let dir = ensure(&v, "01BOT").await.unwrap();
        assert_eq!(dir, homes.join("01BOT"));
        std::fs::write(dir.join("big"), b"x").unwrap();
        clear(&v, "01BOT").await.unwrap();
        assert!(!dir.exists());
        // Emptying a home that is not there is fine.
        clear(&v, "01BOT").await.unwrap();
        assert!(ensure(&v, "a/b").await.is_err());
    }

    /// Unix only: a VM host is Linux, and elsewhere a 20 GB file may not be sparse.
    #[cfg(unix)]
    #[test]
    fn usage_lists_each_image_with_its_size() {
        let tmp = tempfile::tempdir().unwrap();
        let homes = tmp.path().join("homes");
        let images = tmp.path().join("home-images");
        std::fs::create_dir_all(&images).unwrap();
        let f = std::fs::File::create(images.join("01BOTB.ext4")).unwrap();
        f.set_len(20 << 30).unwrap();
        std::fs::write(images.join("01BOTA.ext4"), vec![1u8; 64 * 1024]).unwrap();
        std::fs::write(images.join("01BOTA.ext4.tmp"), b"x").unwrap();
        std::fs::write(images.join("bad-id.ext4"), b"x").unwrap();
        let v = vm(&homes.to_string_lossy(), 20, "/srv/runner/vms");
        let u = usage(&v);
        assert_eq!(
            u.iter().map(|h| h.member_id.as_str()).collect::<Vec<_>>(),
            ["01BOTA", "01BOTB"]
        );
        assert_eq!(u[0].size_bytes, Some(64 * 1024));
        assert!(u[0].used_bytes >= 64 * 1024);
        // A sparse image takes far less than its size.
        assert_eq!(u[1].size_bytes, Some(20 << 30));
        assert!(u[1].used_bytes < 1 << 20);
        assert!(disk(&v).is_some());
    }
}
