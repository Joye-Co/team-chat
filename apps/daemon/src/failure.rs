//! Why a run failed, sorted into the open set of `RunFailure.kind` (machine-protocol.md,
//! "Failure kinds and the sign-in state"). The CLIs' typed fields come first; their words only
//! when they gave none. The words checked are the CLIs' own (Claude Code 2.1, Codex 0.159).

use crate::proto::{RunFailure, failure_kind as kind};
use serde_json::Value;

fn of(k: &str) -> Option<RunFailure> {
    Some(RunFailure::new(k))
}

/// `…usage limit reached|1736112000`: older Claude Code put the reset (epoch seconds) after a
/// bar.
fn reset_after_bar(text: &str) -> Option<u64> {
    let (_, tail) = text.rsplit_once('|')?;
    let digits: String = tail
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let secs: u64 = digits.parse().ok()?;
    // Seconds since the epoch between 2020 and 2100.
    (1_577_836_800..4_102_444_800)
        .contains(&secs)
        .then_some(secs * 1000)
}

/// A kind from the CLI's (or the daemon's) words. None when nothing matches.
pub fn from_text(text: &str) -> Option<RunFailure> {
    let t = text.to_lowercase();
    let has = |w: &[&str]| w.iter().any(|x| t.contains(x));
    if t.trim_start().starts_with("cannot start ")
        && has(&[
            "(os error 2)",
            "no such file",
            "cannot find the file",
            "not found",
        ])
    {
        return of(kind::NOT_INSTALLED);
    }
    if has(&[
        "not logged in",
        "please run /login",
        "invalid api key",
        "token has expired",
        "session expired",
        "unauthorized",
        "authentication_error",
        "authentication failed",
        "failed to authenticate",
        "please log in",
        "login required",
    ]) {
        return of(kind::SIGNED_OUT);
    }
    if has(&[
        "usage limit",
        "credit balance is too low",
        "out of extra usage",
        "usage_limit",
    ]) || (t.contains("hit your") && t.contains("limit"))
    {
        return Some(RunFailure {
            kind: kind::LIMIT.into(),
            resets_at: reset_after_bar(text),
        });
    }
    if has(&[
        "model_not_found",
        "issue with the selected model",
        "model is not supported",
        "unknown model",
        "invalid model",
    ]) || (t.contains("model") && has(&["not_found_error", "does not exist"]))
    {
        return of(kind::MODEL);
    }
    if has(&[
        "connection error",
        "econnrefused",
        "econnreset",
        "enotfound",
        "etimedout",
        "fetch failed",
        "getaddrinfo",
        "network is unreachable",
        "error sending request",
        "dns error",
        "overloaded",
        "request timed out",
    ]) {
        return of(kind::NETWORK);
    }
    None
}

/// A kind from the stderr tail of a CLI that exited early. That tail may hold another program's
/// words (an MCP server's or a hook's "401 Unauthorized"), so it says `signed_out` only with the
/// CLI's own sign-in words; other sign-in words read as `other`, which takes no runtime out.
pub fn from_stderr(text: &str) -> Option<RunFailure> {
    let f = from_text(text)?;
    if f.kind != kind::SIGNED_OUT {
        return Some(f);
    }
    let t = text.to_lowercase();
    let own = ["not logged in", "please run /login", "invalid api key"]
        .iter()
        .any(|w| t.contains(w));
    if own { Some(f) } else { of(kind::OTHER) }
}

/// Claude Code: the `error` of an assistant message that ends a turn with an API error, with its
/// `api_error`. `usage_limit`: the CLI also said the account's limit refused the request (a
/// `rate_limit_event` with status "rejected").
pub fn from_claude(
    error: &str,
    api_error: Option<&str>,
    text: &str,
    usage_limit: bool,
) -> Option<RunFailure> {
    match api_error.unwrap_or_default() {
        "stored_login_refused" | "gateway_signin_required" | "gateway_session_expired" => {
            return of(kind::SIGNED_OUT);
        }
        "usage_limit_reached" => return of(kind::LIMIT),
        "model_requires_usage_credits" | "no_allowed_fallback" => return of(kind::MODEL),
        "connection_failed"
        | "stream_idle_timeout"
        | "no_response"
        | "network_edge_blocked"
        | "tls_untrusted_ca"
        | "computer_slept" => return of(kind::NETWORK),
        _ => {}
    }
    match error {
        "authentication_failed" | "oauth_org_not_allowed" => of(kind::SIGNED_OUT),
        "billing_error" => of(kind::LIMIT),
        "rate_limit" if usage_limit => of(kind::LIMIT),
        // A refusal that is no limit of the account: a server throttle, which passes.
        "rate_limit" => from_text(text)
            .filter(|f| f.kind == kind::LIMIT)
            .or_else(|| of(kind::NETWORK)),
        "model_not_found" => of(kind::MODEL),
        "overloaded" | "server_error" => of(kind::NETWORK),
        _ => from_text(text),
    }
}

/// Codex app-server: a turn error's `codexErrorInfo` (a string, or an object with one key).
pub fn from_codex(info: &Value, text: &str) -> Option<RunFailure> {
    let name = match info {
        Value::String(s) => s.as_str(),
        Value::Object(o) => o.keys().next().map(String::as_str).unwrap_or_default(),
        _ => "",
    };
    match name {
        "unauthorized" => of(kind::SIGNED_OUT),
        "usageLimitExceeded" => of(kind::LIMIT),
        "httpConnectionFailed"
        | "responseStreamConnectionFailed"
        | "responseStreamDisconnected"
        | "responseTooManyFailedAttempts"
        | "serverOverloaded"
        | "internalServerError" => of(kind::NETWORK),
        _ => from_text(text),
    }
}

/// The failure `run.done` reports for a failed run: the runtime's own, else from the error text,
/// else `other`.
pub fn of_run(found: Option<&RunFailure>, error: Option<&str>) -> RunFailure {
    found
        .cloned()
        .or_else(|| error.and_then(from_text))
        .unwrap_or_else(|| RunFailure::new(kind::OTHER))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn k(f: Option<RunFailure>) -> String {
        f.map(|f| f.kind).unwrap_or_default()
    }

    #[test]
    fn claude_code_words() {
        // As Claude Code 2.1 prints them.
        assert_eq!(
            k(from_text("Not logged in · Please run /login")),
            "signed_out"
        );
        assert_eq!(
            k(from_text(
                "API Error: 401 Invalid API key · Please run /login"
            )),
            "signed_out"
        );
        assert_eq!(
            k(from_text(
                "OAuth token has expired. Please obtain a new token"
            )),
            "signed_out"
        );
        assert_eq!(
            k(from_text(
                "You've hit your session limit · resets 3pm (Asia/Shanghai)"
            )),
            "limit"
        );
        assert_eq!(
            k(from_text(
                "There's an issue with the selected model (claude-x). It may not exist or you may not have access to it."
            )),
            "model"
        );
        assert_eq!(k(from_text("API Error: Connection error.")), "network");
        assert_eq!(k(from_text("Credit balance is too low")), "limit");
    }

    #[test]
    fn the_old_limit_text_carries_the_reset() {
        let f = from_text("Claude AI usage limit reached|1762142400").unwrap();
        assert_eq!(f.kind, "limit");
        assert_eq!(f.resets_at, Some(1_762_142_400_000));
        assert_eq!(from_text("usage limit reached|12").unwrap().resets_at, None);
    }

    #[test]
    fn codex_words() {
        assert_eq!(
            k(from_text(
                r#"{"error":{"message":"The model 'gpt-x' does not exist","code":"model_not_found"}}"#
            )),
            "model"
        );
        assert_eq!(
            k(from_text(
                "You've hit your usage limit. Try again at 3:42 PM."
            )),
            "limit"
        );
        assert_eq!(
            k(from_text("unexpected status 401 Unauthorized: ")),
            "signed_out"
        );
        assert_eq!(
            k(from_text("stream error: error sending request for url")),
            "network"
        );
    }

    #[test]
    fn a_program_that_is_not_there() {
        assert_eq!(
            k(from_text(
                "cannot start claude: No such file or directory (os error 2)"
            )),
            "not_installed"
        );
        assert_eq!(
            k(from_text(
                "cannot start codex: The system cannot find the file specified. (os error 2)"
            )),
            "not_installed"
        );
        // Any other reason a start fails is not "not installed".
        assert_eq!(
            k(from_text(
                "cannot start claude: Permission denied (os error 13)"
            )),
            ""
        );
    }

    #[test]
    fn other_errors_have_no_kind() {
        assert_eq!(from_text("the approval hook did not start"), None);
        assert_eq!(from_text(""), None);
        assert_eq!(of_run(None, Some("something odd")).kind, "other");
        assert_eq!(of_run(None, None).kind, "other");
        assert_eq!(of_run(None, Some("Not logged in")).kind, "signed_out");
        let typed = RunFailure::new("model");
        assert_eq!(of_run(Some(&typed), Some("Not logged in")).kind, "model");
    }

    #[test]
    fn a_stderr_tail_signs_out_only_with_the_cli_s_own_words() {
        assert_eq!(
            k(from_stderr("mcp server github: 401 Unauthorized")),
            "other"
        );
        assert_eq!(k(from_stderr("hook: login required")), "other");
        assert_eq!(
            k(from_stderr("Not logged in · Please run /login")),
            "signed_out"
        );
        assert_eq!(k(from_stderr("Error: connect ECONNREFUSED")), "network");
        assert_eq!(from_stderr("something odd"), None);
    }

    #[test]
    fn claude_code_typed_errors_win_over_words() {
        assert_eq!(
            k(from_claude("authentication_failed", None, "", false)),
            "signed_out"
        );
        assert_eq!(k(from_claude("billing_error", None, "", false)), "limit");
        assert_eq!(
            k(from_claude(
                "rate_limit",
                Some("usage_limit_reached"),
                "",
                false
            )),
            "limit"
        );
        assert_eq!(k(from_claude("rate_limit", None, "", true)), "limit");
        // A throttle of the service, not the account's limit.
        assert_eq!(
            k(from_claude(
                "rate_limit",
                None,
                "Server is temporarily limiting requests",
                false
            )),
            "network"
        );
        assert_eq!(k(from_claude("model_not_found", None, "", false)), "model");
        assert_eq!(
            k(from_claude("unknown", Some("connection_failed"), "", false)),
            "network"
        );
        assert_eq!(k(from_claude("overloaded", None, "", false)), "network");
        assert_eq!(
            k(from_claude(
                "unknown",
                Some("gateway_session_expired"),
                "",
                false
            )),
            "signed_out"
        );
        assert_eq!(
            k(from_claude("invalid_request", None, "Not logged in", false)),
            "signed_out"
        );
        assert_eq!(from_claude("max_output_tokens", None, "", false), None);
    }

    #[test]
    fn codex_error_info() {
        assert_eq!(k(from_codex(&json!("unauthorized"), "")), "signed_out");
        assert_eq!(k(from_codex(&json!("usageLimitExceeded"), "")), "limit");
        assert_eq!(
            k(from_codex(
                &json!({ "httpConnectionFailed": { "httpStatusCode": null } }),
                ""
            )),
            "network"
        );
        assert_eq!(k(from_codex(&json!("serverOverloaded"), "")), "network");
        assert_eq!(
            k(from_codex(
                &json!("badRequest"),
                "model_not_found: the model does not exist"
            )),
            "model"
        );
        assert_eq!(
            from_codex(&json!("contextWindowExceeded"), "context full"),
            None
        );
        assert_eq!(from_codex(&Value::Null, "odd"), None);
    }
}
