//! Permissions: a non-member can neither read nor post; bad tokens are refused before the
//! upgrade; a member cannot edit another member's message.

use super::{Ctx, Run, Scenario};
use crate::check;
use crate::raw::{RawWs, upgrade_status};
use anyhow::Result;
use reqwest::Method;
use serde_json::{Value, json};

pub const SCENARIO: Scenario = Scenario {
    name: "permissions",
    about: "a non-member can neither read nor post",
    in_all: true,
    timeout_secs: 60,
    run: start,
};

fn start(c: &mut Ctx) -> Run<'_> {
    Box::pin(run(c))
}

/// The answer must be a refusal: `ack` with ok false (or a connection `error`), with one
/// of `codes`.
fn refused(what: &str, f: &Value, codes: &[&str]) -> Result<String> {
    let code = f["error"]["code"].as_str().unwrap_or_default().to_string();
    let is_refusal = (f["t"] == "ack" && f["ok"] == false) || f["t"] == "error";
    check!(is_refusal, "{what}: not refused: {f}");
    check!(
        codes.contains(&code.as_str()),
        "{what}: code {code}, want one of {codes:?}"
    );
    Ok(format!("{what}: {code}"))
}

async fn run(c: &mut Ctx) -> Result<()> {
    let h = c.h.clone();
    let a = h.human("perm-a").await?;
    let b = h.human("perm-b").await?;
    let x = h.human("perm-outsider").await?;
    let g = h.group(&a, "perm", &[&b]).await?;

    let mut ra = RawWs::connect(&h.base, a.token()).await?;
    let ack = ra
        .request(json!({ "t": "send", "groupId": g, "clientMsgId": chat_core::new_id(), "body": "members only" }))
        .await?;
    check!(ack["ok"] == true, "A's send failed: {ack}");
    let topic = ack["result"]["topic"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let msg = ack["result"]["message"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    let mut rx = RawWs::connect(&h.base, x.token()).await?;
    let groups = rx.welcome["groups"].as_array().cloned().unwrap_or_default();
    check!(
        !groups.iter().any(|s| s["group"]["id"] == g.as_str()),
        "outsider's welcome lists the group"
    );
    let deny = ["forbidden", "not_found"];
    let mut seen = Vec::new();
    let frames = [
        (
            "fetch",
            json!({ "t": "fetch", "groupId": g, "afterSeq": 0 }),
        ),
        (
            "history",
            json!({ "t": "history", "groupId": g, "topicId": topic }),
        ),
        (
            "send new topic",
            json!({ "t": "send", "groupId": g, "clientMsgId": chat_core::new_id(), "body": "x" }),
        ),
        (
            "send reply",
            json!({ "t": "send", "groupId": g, "topicId": topic, "clientMsgId": chat_core::new_id(), "body": "x" }),
        ),
        (
            "edit",
            json!({ "t": "edit", "groupId": g, "messageId": msg, "body": "x" }),
        ),
        (
            "delete",
            json!({ "t": "delete", "groupId": g, "messageId": msg }),
        ),
        (
            "topic.update",
            json!({ "t": "topic.update", "groupId": g, "topicId": topic, "title": "x" }),
        ),
        (
            "topic.follow",
            json!({ "t": "topic.follow", "groupId": g, "topicId": topic, "follow": true }),
        ),
        (
            "search",
            json!({ "t": "search", "groupId": g, "query": "members" }),
        ),
    ];
    for (what, f) in frames {
        let ans = rx.request(f).await?;
        // V1 extension frames: a server without them answers `invalid` (unknown frame type).
        let ext = what == "search" || what == "topic.follow";
        let codes: &[&str] = if ext {
            &["forbidden", "not_found", "invalid"]
        } else {
            &deny
        };
        seen.push(refused(what, &ans, codes)?);
    }
    // Frames without an ack must not leak either: typing and read are ignored or refused.
    rx.send(json!({ "t": "typing", "groupId": g, "topicId": topic }))
        .await?;
    rx.send(json!({ "t": "read", "groupId": g, "seq": 2 }))
        .await?;

    // HTTP: the outsider cannot add itself, and does not see the group.
    let (st, _) = h
        .api(
            Method::POST,
            &format!("/v1/groups/{g}/members"),
            Some(x.token()),
            Some(json!({ "memberId": x.id })),
        )
        .await?;
    check!(
        st == 403 || st == 404,
        "outsider adding itself: status {st}"
    );
    seen.push(format!("POST /v1/groups/:id/members: {st}"));
    let (st, list) = h
        .api(Method::GET, "/v1/groups", Some(x.token()), None)
        .await?;
    check!(st == 200, "GET /v1/groups: {st}");
    check!(
        !list
            .as_array()
            .is_some_and(|l| l.iter().any(|s| s["group"]["id"] == g.as_str())),
        "outsider lists the group"
    );

    // A member that is not the author cannot edit; nor delete as a non-owner.
    let mut rb = RawWs::connect(&h.base, b.token()).await?;
    let e = rb
        .request(json!({ "t": "edit", "groupId": g, "messageId": msg, "body": "B was here" }))
        .await?;
    seen.push(refused(
        "member edits another's message",
        &e,
        &["forbidden"],
    )?);
    let d = rb
        .request(json!({ "t": "delete", "groupId": g, "messageId": msg }))
        .await?;
    seen.push(refused(
        "member deletes another's message",
        &d,
        &["forbidden"],
    )?);

    // Tokens: none, garbage, and revoked are refused with 401 before the upgrade.
    check!(
        upgrade_status(&h.base, None).await? == Some(401),
        "no token: not 401"
    );
    let fake = format!("tc_{}", "A".repeat(43));
    check!(
        upgrade_status(&h.base, Some(&fake)).await? == Some(401),
        "unknown token: not 401"
    );
    let (st, _) = h.api(Method::GET, "/v1/me", Some(&fake), None).await?;
    check!(st == 401, "GET /v1/me with an unknown token: {st}");
    let (st, _) = h
        .api(
            Method::POST,
            "/v1/tokens/revoke",
            Some(x.token()),
            Some(json!({})),
        )
        .await?;
    check!(st == 200, "outsider revoking its own token: {st}");
    check!(
        upgrade_status(&h.base, Some(x.token())).await? == Some(401),
        "revoked token: not 401"
    );
    seen.push("no, unknown and revoked tokens: 401".into());

    // The log is untouched by the outsider and by B.
    let (events, _) = ra.fetch_all(&g).await?;
    check!(
        !events
            .iter()
            .any(|e| e["actorId"] == x.id.as_str() || e["actorId"] == b.id.as_str()),
        "a refused write reached the log"
    );
    let body = events
        .iter()
        .find(|e| e["type"] == "message.created")
        .map(|e| e["data"]["message"]["body"].clone());
    check!(body == Some(json!("members only")), "the message changed");
    check!(
        !events
            .iter()
            .any(|e| e["type"] == "message.updated" || e["type"] == "message.deleted"),
        "the message was edited or deleted"
    );
    c.metric("refusals", seen.len() as u64);
    c.details.extend(seen);
    Ok(())
}
