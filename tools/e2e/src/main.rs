//! Headless end-to-end runner on chat-core. See tools/e2e/README.md.

mod harness;
mod raw;
mod report;
mod scenarios;

use anyhow::{Result, anyhow, bail};
use harness::Harness;
use scenarios::{Ctx, registry};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const USAGE: &str = "\
usage: e2e [options]
  --base-url <url>      server origin (default $E2E_BASE_URL or http://127.0.0.1:8790)
  --secrets-dir <dir>   holds owner-token, or admin-secret for a first bootstrap
                        (default ~/.config/team-chat/local-e2e)
  --scenario <a,b|all>  scenarios to run (default all)
  --minutes <n>         soak length; with --scenario all, soak runs only when given
  --report-dir <dir>    default tools/e2e/reports
  --list                list scenarios
";

struct Args {
    base_url: String,
    secrets_dir: PathBuf,
    scenarios: Vec<String>,
    minutes: Option<f64>,
    report_dir: PathBuf,
    list: bool,
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
}

fn expand(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(r) => home().join(r),
        None => PathBuf::from(p),
    }
}

fn parse_args() -> Result<Args> {
    let mut a = Args {
        base_url: std::env::var("E2E_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8790".into()),
        secrets_dir: home().join(".config/team-chat/local-e2e"),
        scenarios: vec!["all".into()],
        minutes: None,
        report_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("reports"),
        list: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut val = || it.next().ok_or_else(|| anyhow!("{k} needs a value"));
        match k.as_str() {
            "--base-url" => a.base_url = val()?,
            "--secrets-dir" => a.secrets_dir = expand(&val()?),
            "--scenario" => a.scenarios = val()?.split(',').map(|s| s.trim().to_string()).collect(),
            "--minutes" => a.minutes = Some(val()?.parse().map_err(|_| anyhow!("bad --minutes"))?),
            "--report-dir" => a.report_dir = expand(&val()?),
            "--list" => a.list = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            _ => bail!("unknown argument {k}\n{USAGE}"),
        }
    }
    Ok(a)
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    match run().await {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(2);
        }
    }
}

async fn run() -> Result<bool> {
    let args = parse_args()?;
    let all = registry();
    if args.list {
        for s in &all {
            println!("{:<12} {}", s.name, s.about);
        }
        return Ok(true);
    }
    let picked: Vec<_> = if args.scenarios.iter().any(|s| s == "all") {
        all.into_iter()
            .filter(|s| s.in_all || (s.name == "soak" && args.minutes.is_some()))
            .collect()
    } else {
        let mut v = Vec::new();
        for name in &args.scenarios {
            let pos = all
                .iter()
                .position(|s| s.name == name)
                .ok_or_else(|| anyhow!("unknown scenario {name}; see --list"))?;
            v.push(pos);
        }
        let mut all: Vec<_> = all.into_iter().map(Some).collect();
        v.into_iter().filter_map(|i| all[i].take()).collect()
    };
    let minutes = args.minutes.unwrap_or(1.0);

    let data_root = std::env::temp_dir().join(format!("team-chat-e2e-{}", std::process::id()));
    let h = Arc::new(Harness::setup(&args.base_url, &args.secrets_dir, data_root).await?);
    println!("run {} against {}", h.run_id, h.base);

    let mut summary = Vec::new();
    let mut ok = true;
    for s in picked {
        let mut ctx = Ctx {
            h: h.clone(),
            minutes,
            details: Vec::new(),
            metrics: Default::default(),
        };
        let started_at = chat_core::now_ms();
        let t0 = Instant::now();
        let limit = Duration::from_secs(
            s.timeout_secs + (minutes * 60.0) as u64 * u64::from(s.name == "soak"),
        );
        let res = match tokio::time::timeout(limit, (s.run)(&mut ctx)).await {
            Ok(r) => r,
            Err(_) => Err(anyhow!("scenario timed out after {limit:?}")),
        };
        let dur = t0.elapsed();
        let passed = res.is_ok();
        ok &= passed;
        let error = res.err().map(|e| format!("{e:#}"));
        println!(
            "{} {:<12} {:>7.1}s {}",
            if passed { "PASS" } else { "FAIL" },
            s.name,
            dur.as_secs_f64(),
            error.as_deref().unwrap_or("")
        );
        let rep = report::Report {
            run_id: h.run_id.clone(),
            scenario: s.name.into(),
            base_url: h.base.clone(),
            passed,
            started_at,
            duration_ms: dur.as_millis() as u64,
            error,
            details: ctx.details,
            metrics: ctx.metrics,
        };
        report::write(
            &args.report_dir,
            &format!("{}-{}.json", h.run_id, s.name),
            &rep,
        )?;
        summary.push(json!({
            "scenario": rep.scenario, "passed": rep.passed, "durationMs": rep.duration_ms,
            "error": rep.error, "metrics": rep.metrics,
        }));
    }
    if let Err(e) = h.cleanup().await {
        eprintln!("cleanup: {e:#}");
    }
    let path = report::write(
        &args.report_dir,
        &format!("{}-summary.json", h.run_id),
        &json!({ "runId": h.run_id, "baseUrl": h.base, "passed": ok, "scenarios": summary }),
    )?;
    println!("report: {}", path.display());
    Ok(ok)
}
