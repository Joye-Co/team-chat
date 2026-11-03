//! Stops a bot run through the client API, as the desktop app's 停止 does
//! (`Client::cancel_run`). Used by `apps/daemon/e2e/local-e2e.ts`.
//!
//! Usage: `cancel_run <base-url> <token-file> <run-id> <data-dir>`
//! Prints one JSON line: `{"outcome":"cancelled"|"already_ended","status":"<cached status>"}`,
//! or `{"error":"..."}` with exit code 1. The token is read from the file and never printed.

use chat_core::{CancelRun, Client, ClientConfig};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [base, token_file, run_id, data_dir] = args.as_slice() else {
        eprintln!("usage: cancel_run <base-url> <token-file> <run-id> <data-dir>");
        std::process::exit(2);
    };
    let token = match std::fs::read_to_string(token_file) {
        Ok(t) => t.trim().to_string(),
        Err(e) => fail(&format!("cannot read the token file: {e}")),
    };
    let client = Client::new(ClientConfig::new(base.as_str(), token, data_dir.as_str()))
        .unwrap_or_else(|e| fail(&e.to_string()));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|e| fail(&e.to_string()));
    let outcome = match runtime.block_on(client.cancel_run(run_id)) {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => fail(&e.to_string()),
        Err(e) => fail(&e.to_string()),
    };
    let status = client
        .store()
        .run(run_id)
        .ok()
        .flatten()
        .and_then(|r| serde_json::to_value(r.status).ok());
    let outcome = match outcome {
        CancelRun::Cancelled => "cancelled",
        CancelRun::AlreadyEnded => "already_ended",
    };
    println!(
        "{}",
        serde_json::json!({ "outcome": outcome, "status": status })
    );
}

fn fail(message: &str) -> ! {
    println!("{}", serde_json::json!({ "error": message }));
    std::process::exit(1);
}
