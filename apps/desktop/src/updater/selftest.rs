//! Live update self-test against a real server (`wrangler dev` or an environment). No
//! window, no focus, nothing installed outside the given folders.
//!
//!   desktop --selftest-update <url> --token-file <path> --workdir <dir> --report <path>
//!   desktop --selftest-update <url> --token-file <path> --install-dir <dir> --report <path>
//!           [--workdir <dir>]
//!
//! Without `--install-dir` it runs the whole update path on a fake bundle in the work
//! folder. The server must then have a release whose DMG holds `<product>.app` with this
//! app's bundle id, the release version as `CFBundleShortVersionString`, and an executable
//! script that writes "<anything>" to `$UPDATER_TEST_OUT` (see `e2e.sh` next to this file).
//! Steps: check (as version 0.0.1, and as the release version: no update), download with
//! progress, verify SHA-256, install over a fake 0.0.1 bundle, relaunch after a stand-in
//! process exits, and finish the update as the new version.
//!
//! With `--install-dir` it updates the real `<product>.app` in that folder (a copy of a
//! released app, never `/Applications`) as the running app would: check as that bundle's
//! version, download, verify SHA-256, install (the new bundle must satisfy the old
//! bundle's designated requirement), read the new version and requirement, and remove the
//! old bundle. It never starts the new app. The work folder defaults to the install folder.
//!
//! Windows (`scripts/test-windows-update.sh`, run by the installed app itself):
//!
//!   <app>.exe --selftest-update <url> --token-file <path> --install-dir <dir> --workdir <dir>
//!             --report <path> --finish-report <path>
//!   <app>.exe --selftest-updated <marker> --report <path>
//!   <app>.exe --selftest-installed --report <path>
//!
//! `--selftest-update --install-dir` goes the way the app does: check as this build's
//! version, download the installer, verify SHA-256, start the update helper and exit. The
//! helper then replaces the install folder and starts the new version with
//! `--selftest-updated`, which finishes the update as the app does at start (removes the
//! marker and the backup) and writes its version to `--finish-report`.
//! `--selftest-installed` reports the version and the install folder of an installed copy,
//! and which server URLs the build carries (as digests).

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use super::{install, version};

struct Args {
    url: String,
    token_file: PathBuf,
    workdir: PathBuf,
    install_dir: Option<PathBuf>,
    report: PathBuf,
    /// Windows: where the new version writes its report after the update.
    finish_report: Option<PathBuf>,
}

/// Runs the self-test when `--selftest-update` is on the command line.
pub fn main_if_requested() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value_of = |name: &str| {
        let at = args.iter().position(|a| a == name)?;
        args.get(at + 1).cloned()
    };
    if args.iter().any(|a| a == "--selftest-installed") {
        return Some(report_installed(value_of("--report").map(PathBuf::from)));
    }
    if let Some(marker) = value_of("--selftest-updated") {
        return Some(report_updated(
            Path::new(&marker),
            value_of("--report").map(PathBuf::from),
        ));
    }
    if !args.iter().any(|a| a == "--selftest-update") {
        return None;
    }
    let finish_report = value_of("--finish-report").map(PathBuf::from);
    let mut url = None;
    let mut token_file = None;
    let mut workdir = None;
    let mut install_dir = None;
    let mut report = None;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--selftest-update" => url = it.next(),
            "--token-file" => token_file = it.next().map(PathBuf::from),
            "--workdir" => workdir = it.next().map(PathBuf::from),
            "--install-dir" => install_dir = it.next().map(PathBuf::from),
            "--report" => report = it.next().map(PathBuf::from),
            "--finish-report" => {
                it.next();
            }
            _ => {}
        }
    }
    let workdir = workdir.or_else(|| install_dir.clone());
    let (Some(url), Some(token_file), Some(workdir), Some(report)) =
        (url, token_file, workdir, report)
    else {
        eprintln!(
            "usage: desktop --selftest-update <url> --token-file <path> (--workdir <dir> | --install-dir <dir>) --report <path>"
        );
        return Some(2);
    };
    let args = Args {
        url,
        token_file,
        workdir,
        install_dir,
        report,
        finish_report,
    };
    let started = Instant::now();
    let mut steps = serde_json::Map::new();
    let result = match &args.install_dir {
        Some(dir) if cfg!(windows) => run_installed_windows(&args, dir, &mut steps),
        Some(dir) => run_install_dir(&args, dir, &mut steps),
        None => run(&args, &mut steps),
    };
    let ok = result.is_ok();
    let json = serde_json::json!({
        "ok": ok,
        "error": result.err(),
        "steps": steps,
        "ms": started.elapsed().as_millis() as u64,
    });
    if let Some(p) = args.report.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(
        &args.report,
        serde_json::to_string_pretty(&json).unwrap_or_default() + "\n",
    );
    Some(if ok { 0 } else { 1 })
}

fn step(steps: &mut serde_json::Map<String, serde_json::Value>, name: &str, v: serde_json::Value) {
    steps.insert(name.into(), v);
}

/// A fresh folder of its own inside the work folder (only this one is removed) and a
/// client signed in with the token file.
fn client_in_workdir(args: &Args) -> Result<(PathBuf, chat_core::Client), String> {
    let token = std::fs::read_to_string(&args.token_file)
        .map_err(|e| format!("token file: {e}"))?
        .trim()
        .to_string();
    let workdir = args
        .workdir
        .join(format!(".update-selftest-{}", std::process::id()));
    std::fs::create_dir_all(&workdir).map_err(|e| e.to_string())?;
    let workdir = std::fs::canonicalize(&workdir).map_err(|e| e.to_string())?;
    let client = chat_core::Client::new(chat_core::ClientConfig::new(
        args.url.clone(),
        token,
        workdir.join("data"),
    ))
    .map_err(|e| format!("client: {e}"))?;
    Ok((workdir, client))
}

/// Downloads the update with progress into the work folder and checks its SHA-256.
fn download_verified(
    client: &chat_core::Client,
    info: &chat_core::proto::UpdateInfo,
    workdir: &Path,
    steps: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<PathBuf, String> {
    let calls = Arc::new(AtomicU64::new(0));
    let last = Arc::new(AtomicU64::new(0));
    let (c, l) = (calls.clone(), last.clone());
    let progress: chat_core::ProgressFn = Arc::new(move |p: chat_core::Progress| {
        c.fetch_add(1, Ordering::Relaxed);
        l.store(p.done, Ordering::Relaxed);
    });
    let dmg = workdir
        .join("updates")
        .join(super::download_name(&info.version));
    let path = futures::executor::block_on(client.download_update(info.clone(), &dmg, progress))
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("download: {e}"))?;
    let matches = version::checksum_matches(&path, &info.sha256).map_err(|e| e.to_string())?;
    if !matches {
        return Err("download: checksum mismatch".into());
    }
    step(
        steps,
        "download",
        serde_json::json!({
            "bytes": std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
            "progress_calls": calls.load(Ordering::Relaxed),
            "progress_last": last.load(Ordering::Relaxed),
            "sha256_ok": true,
        }),
    );
    Ok(path)
}

fn write_report(path: Option<PathBuf>, json: serde_json::Value) {
    let Some(path) = path else { return };
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(
        path,
        serde_json::to_string_pretty(&json).unwrap_or_default() + "\n",
    );
}

/// `--selftest-installed`: what an installed copy says about itself.
fn report_installed(report: Option<PathBuf>) -> i32 {
    let dir = super::running_bundle();
    let ok = dir.is_some();
    write_report(
        report,
        serde_json::json!({
            "ok": ok,
            "version": version::CURRENT,
            "install_dir": dir,
            "exe": std::env::current_exe().ok(),
            "app_id": env!("PRODUCT_BUNDLE_ID"),
            "url_scheme": env!("PRODUCT_URL_SCHEME"),
            // The server URLs compiled into this build (the 正式服务器 / 测试服务器 menu and
            // the sign-in default), as SHA-256 so a test log never shows a deployment's
            // address; null when the build has none and sign-in asks for the address.
            "servers": {
                "production": url_digest(env!("PRODUCT_PRODUCTION_URL")),
                "staging": url_digest(env!("PRODUCT_STAGING_URL")),
            },
        }),
    );
    if ok { 0 } else { 1 }
}

/// SHA-256 (hex) of a compiled-in server URL; `None` when the build has none.
fn url_digest(url: &str) -> Option<String> {
    use sha2::{Digest as _, Sha256};
    (!url.is_empty()).then(|| {
        Sha256::digest(url.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    })
}

/// `--selftest-updated <marker>`: the first start of the new version after an update, as
/// the app does it in `updater::install`.
fn report_updated(marker: &Path, report: Option<PathBuf>) -> i32 {
    let dir = super::running_bundle();
    let old = std::fs::read(marker)
        .ok()
        .and_then(|b| serde_json::from_slice::<install::Marker>(&b).ok())
        .map(|m| m.old);
    let finish = install::finish_on_launch(marker, dir.as_deref(), version::CURRENT);
    let ok = finish == install::Finish::RemovedOld;
    write_report(
        report,
        serde_json::json!({
            "ok": ok,
            "version": version::CURRENT,
            "finish": format!("{finish:?}"),
            "install_dir": dir,
            "old_removed": old.is_some_and(|o| !o.exists()),
        }),
    );
    if ok { 0 } else { 1 }
}

/// Windows: updates the installed app in `dir` the way the running app does, up to the
/// point where it quits: the update helper does the rest after this process exits.
fn run_installed_windows(
    args: &Args,
    dir: &Path,
    steps: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    // Plain absolute paths: the installer does not take a canonical `\\?\` path.
    let bundle = std::path::absolute(dir).map_err(|e| format!("install dir: {e}"))?;
    if !bundle.join(install::windows::APP_EXE).is_file() {
        return Err(format!("install dir: no app in {}", bundle.display()));
    }
    let finish_report = args
        .finish_report
        .as_ref()
        .ok_or("--finish-report <path> is missing")?;
    let finish_report = std::path::absolute(finish_report).map_err(|e| e.to_string())?;
    step(
        steps,
        "before",
        serde_json::json!({ "version": version::CURRENT }),
    );
    let token = std::fs::read_to_string(&args.token_file)
        .map_err(|e| format!("token file: {e}"))?
        .trim()
        .to_string();
    // Kept after this process exits: the helper reads the installer and writes the marker,
    // its log and its result here.
    let data = std::path::absolute(args.workdir.join("data")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let client = chat_core::Client::new(chat_core::ClientConfig::new(
        args.url.clone(),
        token,
        data.clone(),
    ))
    .map_err(|e| format!("client: {e}"))?;
    let info = futures::executor::block_on(client.check_update(version::CURRENT))
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("check: {e}"))?
        .ok_or_else(|| format!("check: no update offered to {}", version::CURRENT))?;
    step(
        steps,
        "check",
        serde_json::json!({ "version": info.version, "size": info.size, "platform": info.platform }),
    );
    let path = download_verified(&client, &info, &data, steps)?;
    let plan = install::Plan {
        bundle,
        dmg: path,
        version: info.version.clone(),
        sha256: info.sha256.clone(),
        bundle_id: env!("PRODUCT_BUNDLE_ID").into(),
        app_name: None,
        marker: data.join("updates/pending.json"),
    };
    let launch = vec![
        "--selftest-updated".into(),
        plan.marker.clone().into(),
        "--report".into(),
        finish_report.into(),
    ];
    let (marker, staged) =
        install::windows::prepare(&plan, launch).map_err(|e| format!("install: {e}"))?;
    install::windows::start(&staged, std::process::id()).map_err(|e| format!("helper: {e}"))?;
    step(
        steps,
        "helper",
        serde_json::json!({
            "program": staged.helper,
            "result": staged.job.result,
            "log": staged.job.log,
            "backup": marker.old,
        }),
    );
    Ok(())
}

/// Updates the real bundle in `dir` the way the running app updates itself.
fn run_install_dir(
    args: &Args,
    dir: &Path,
    steps: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let bundle = std::fs::canonicalize(dir)
        .map_err(|e| format!("install dir: {e}"))?
        .join(format!("{}.app", env!("PRODUCT_NAME")));
    if !bundle.is_dir() {
        return Err(format!("install dir: no {}", bundle.display()));
    }
    let current = install::bundle_version(&bundle).map_err(|e| format!("bundle: {e}"))?;
    let required = install::designated_requirement(&bundle);
    step(
        steps,
        "before",
        serde_json::json!({ "version": current, "requirement": required }),
    );
    let (workdir, client) = client_in_workdir(args)?;
    let result = (|| {
        let info = futures::executor::block_on(client.check_update(&current))
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("check: {e}"))?
            .ok_or_else(|| format!("check: no update offered to {current}"))?;
        step(
            steps,
            "check",
            serde_json::json!({ "version": info.version, "size": info.size }),
        );
        let path = download_verified(&client, &info, &workdir, steps)?;
        let plan = install::Plan {
            bundle: bundle.clone(),
            dmg: path,
            version: info.version.clone(),
            sha256: info.sha256.clone(),
            bundle_id: env!("PRODUCT_BUNDLE_ID").into(),
            app_name: Some(format!("{}.app", env!("PRODUCT_NAME"))),
            marker: workdir.join("data/updates/pending.json"),
        };
        let marker = install::install(&plan).map_err(|e| format!("install: {e}"))?;
        let version = install::bundle_version(&bundle).map_err(|e| format!("installed: {e}"))?;
        let requirement = install::designated_requirement(&bundle);
        step(
            steps,
            "after",
            serde_json::json!({ "version": version, "requirement": requirement }),
        );
        if version != info.version {
            return Err(format!(
                "installed: version {version}, expected {}",
                info.version
            ));
        }
        if required.is_some() && requirement != required {
            return Err("installed: the designated requirement changed".into());
        }
        if let Some(req) = &required {
            install::check_signature(&bundle, req).map_err(|e| format!("installed: {e}"))?;
        }
        let finish = install::finish_on_launch(&plan.marker, Some(&bundle), &info.version);
        if finish != install::Finish::RemovedOld || marker.old.exists() {
            return Err(format!("finish: {finish:?}"));
        }
        step(steps, "finish", serde_json::json!("old bundle removed"));
        Ok(())
    })();
    let _ = install::remove_all(&workdir);
    result
}

fn run(args: &Args, steps: &mut serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    let (workdir, client) = client_in_workdir(args)?;

    // 1. Check as an old version, then as the release version.
    let info = futures::executor::block_on(client.check_update("0.0.1"))
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("check: {e}"))?
        .ok_or("check: no update offered to 0.0.1")?;
    step(
        steps,
        "check",
        serde_json::json!({ "version": info.version, "size": info.size }),
    );
    let same = futures::executor::block_on(client.check_update(&info.version))
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("check same: {e}"))?;
    if same.is_some() {
        return Err("check: an update was offered to the release version itself".into());
    }
    step(steps, "check_same_version", serde_json::json!("none"));

    // 2. Download with progress, verify.
    let path = download_verified(&client, &info, &workdir, steps)?;

    // 3. Install over a fake old bundle.
    let apps = workdir.join("Applications");
    let bundle = apps.join(format!("{}.app", env!("PRODUCT_NAME")));
    fake_old_bundle(&bundle)?;
    let plan = install::Plan {
        bundle: bundle.clone(),
        dmg: path.clone(),
        version: info.version.clone(),
        sha256: info.sha256.clone(),
        bundle_id: env!("PRODUCT_BUNDLE_ID").into(),
        app_name: Some(format!("{}.app", env!("PRODUCT_NAME"))),
        marker: workdir.join("data/updates/pending.json"),
    };
    let marker = install::install(&plan).map_err(|e| format!("install: {e}"))?;
    install::check_bundle(&bundle, env!("PRODUCT_BUNDLE_ID"), &info.version)
        .map_err(|e| format!("installed bundle: {e}"))?;
    step(
        steps,
        "install",
        serde_json::json!({ "old_kept": marker.old.exists() }),
    );

    // 4. Relaunch after the stand-in app process exits.
    let out = workdir.join("started.txt");
    let mut app = Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .map_err(|e| e.to_string())?;
    let exe = bundle.join("Contents/MacOS").join(env!("PRODUCT_NAME"));
    let mut env_arg = OsString::from("UPDATER_TEST_OUT=");
    env_arg.push(&out);
    install::relaunch(
        app.id(),
        &install::Launch::Exec(vec!["/usr/bin/env".into(), env_arg, exe.into()]),
    )
    .map_err(|e| format!("relaunch: {e}"))?;
    std::thread::sleep(Duration::from_millis(300));
    let early = out.exists();
    let _ = app.kill();
    let _ = app.wait();
    if early {
        return Err("relaunch: started before the old process exited".into());
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while !out.exists() {
        if Instant::now() > deadline {
            return Err("relaunch: the new version did not start".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    step(steps, "relaunch", serde_json::json!("started"));

    // 5. The new version cleans up.
    let finish = install::finish_on_launch(&plan.marker, Some(&bundle), &info.version);
    if finish != install::Finish::RemovedOld || marker.old.exists() {
        return Err(format!("finish: {finish:?}"));
    }
    step(steps, "finish", serde_json::json!("old bundle removed"));
    let _ = install::remove_all(&workdir);
    Ok(())
}

fn fake_old_bundle(bundle: &Path) -> Result<(), String> {
    let macos = bundle.join("Contents/MacOS");
    std::fs::create_dir_all(&macos).map_err(|e| e.to_string())?;
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>{}</string>
  <key>CFBundleExecutable</key><string>{}</string>
  <key>CFBundleShortVersionString</key><string>0.0.1</string>
</dict></plist>
"#,
        env!("PRODUCT_BUNDLE_ID"),
        env!("PRODUCT_NAME")
    );
    std::fs::write(bundle.join("Contents/Info.plist"), plist).map_err(|e| e.to_string())?;
    let exe = macos.join(env!("PRODUCT_NAME"));
    std::fs::write(&exe, "#!/bin/sh\nexit 0\n").map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
