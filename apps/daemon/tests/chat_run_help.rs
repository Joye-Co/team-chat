//! The help a member needs before it posts runs from the `chat-run` binary alone, without a
//! daemon: the card schema in pieces, working examples, the check, the page guide and the
//! starter page.

use std::process::Command;

fn linked(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let p = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    #[cfg(unix)]
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_daemon"), &p).unwrap();
    #[cfg(not(unix))]
    if std::fs::hard_link(env!("CARGO_BIN_EXE_daemon"), &p).is_err() {
        std::fs::copy(env!("CARGO_BIN_EXE_daemon"), &p).unwrap();
    }
    p
}

#[test]
fn the_card_and_page_help_commands_need_no_daemon() {
    let tmp = tempfile::tempdir().unwrap();
    let chat_run = linked(tmp.path(), "chat-run");
    let run = |args: &[&str]| {
        let out = Command::new(&chat_run)
            .args(args)
            .current_dir(tmp.path())
            .env_remove("CHAT_RUN_SOCKET")
            .env_remove("CHAT_RUN_DIR")
            .output()
            .unwrap();
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };
    let json = |text: &str| -> serde_json::Value { serde_json::from_str(text).expect(text) };

    let (code, index, _) = run(&["card", "schema"]);
    assert_eq!(code, Some(0));
    assert!(index.starts_with("A card is one JSON object"), "{index}");
    assert!(index.len() < 9000, "{} bytes", index.len());
    let (code, block, _) = run(&["card", "schema", "--block", "chart"]);
    assert_eq!(code, Some(0));
    assert_eq!(json(&block)["properties"]["type"]["const"], "chart");
    assert!(block.len() < 4000, "{} bytes", block.len());
    assert!(json(&run(&["card", "schema", "--ops"]).1)["items"]["oneOf"].is_array());
    assert!(json(&run(&["card", "schema", "--full"]).1)["$defs"]["block"]["oneOf"].is_array());
    let (code, out, err) = run(&["card", "schema", "--block", "pie"]);
    assert_eq!(code, Some(1));
    let said = json(&out)["error"]["message"].as_str().unwrap().to_string();
    assert!(said.contains("unknown block type pie"), "{said}");
    assert!(
        err.contains("chat-run card schema [--block <type> | --ops | --full]"),
        "{err}"
    );

    // An example is a card the check takes; a wrong card gets every problem at once.
    assert!(run(&["card", "example"]).1.contains("  review: "));
    let (code, ask, _) = run(&["card", "example", "ask"]);
    assert_eq!(code, Some(0));
    assert_eq!(json(&ask)["intent"], "ask");
    std::fs::write(tmp.path().join("card.json"), &ask).unwrap();
    let (code, report, _) = run(&["card", "check", "--file", "card.json"]);
    assert_eq!(code, Some(0), "{report}");
    assert!(
        report.starts_with("card.json: 0 problems, 0 warnings"),
        "{report}"
    );
    std::fs::write(
        tmp.path().join("bad.json"),
        r#"{"v":2,"intent":"info","title":"t","blocks":[{"type":"text","text":"no id"},{"type":"plan","id":"p","steps":[]}]}"#,
    )
    .unwrap();
    let (code, report, _) = run(&["card", "check", "--file", "bad.json"]);
    assert_eq!(code, Some(1));
    assert!(
        report.starts_with("bad.json: 1 problem, 2 warnings"),
        "{report}"
    );
    assert!(
        report.contains("problem: blocks.0 (text): ") && report.contains("unique id"),
        "{report}"
    );
    assert!(
        report.contains("warning: blocks.1.steps: unknown field"),
        "{report}"
    );
    std::fs::write(
        tmp.path().join("ops.json"),
        run(&["card", "example", "patch"]).1,
    )
    .unwrap();
    assert_eq!(run(&["card", "check", "--ops", "ops.json"]).0, Some(0));

    // Posting still needs the run: the check does not stand in for the server.
    let (code, out, _) = run(&["card", "post", "--file", "card.json"]);
    assert_eq!(code, Some(1));
    let said = json(&out)["error"]["message"].as_str().unwrap().to_string();
    assert!(said.contains("CHAT_RUN_SOCKET"), "{said}");
    let (code, out, _) = run(&[
        "card",
        "set",
        "--message",
        "M",
        "--block",
        "bar",
        "--props",
        r#"{"value":0.5}"#,
    ]);
    assert_eq!(code, Some(1));
    assert_eq!(json(&out)["error"]["code"], "unavailable");

    // The page guide and the starter; the manifests the starter writes are taken by the CLI.
    let (code, guide, _) = run(&["artifact", "guide"]);
    assert_eq!(code, Some(0));
    assert!(
        guide.contains("window.teamchat") && guide.contains("events.json"),
        "{guide}"
    );
    let (code, wrote, _) = run(&["artifact", "example", "--out", "starter"]);
    assert_eq!(code, Some(0), "{wrote}");
    for f in ["page/index.html", "events.json", "data.json", "reply.json"] {
        assert!(tmp.path().join("starter").join(f).is_file(), "{f}");
    }
    let (code, out, _) = run(&[
        "artifact",
        "create",
        "--title",
        "t",
        "--dir",
        "starter/page",
        "--events",
        "starter/events.json",
        "--data",
        "starter/data.json",
        "--summary",
        "s",
    ]);
    assert_eq!(code, Some(1));
    assert_eq!(json(&out)["error"]["code"], "unavailable", "{out}");
    std::fs::write(
        tmp.path().join("wrong.json"),
        r#"[{"name":"toggle","type":"ask"}]"#,
    )
    .unwrap();
    let (code, out, _) = run(&[
        "artifact",
        "create",
        "--title",
        "t",
        "--dir",
        "starter/page",
        "--events",
        "wrong.json",
    ]);
    assert_eq!(code, Some(1));
    let v = json(&out);
    assert_eq!(v["error"]["code"], "invalid");
    let said = v["error"]["message"].as_str().unwrap();
    assert!(said.contains("unknown key type"), "{said}");
}
