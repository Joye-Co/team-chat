//! 引用提问: labels, value snapshots, part addresses, the Markdown block counter, the body
//! prefix and the addressee (REFS §C3 desktop unit tests).

use chat_core::proto::{BlockRef, BlockRefInput, PartKey};
use serde_json::{Value, json};

use super::{
    RefSource, addressee, label,
    parts::{self, ChartData, ChartPick, ChartSeriesData, TableData, TablePick},
    prefix, side_chat_body, split_quote_lines, strip_prefix, value, with_prefix,
};
use crate::markdown::{MdBlock, top_level_blocks};

fn src() -> RefSource {
    RefSource::Message {
        group_id: "g".into(),
        topic_id: "t".into(),
        message_id: "m-1".into(),
    }
}

fn keys(p: &chat_core::proto::card2::PartRef) -> Vec<(String, PartKey)> {
    p.keys.clone().unwrap_or_default().into_iter().collect()
}

fn channel() -> (Vec<String>, Vec<Vec<String>>) {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    (
        s(&["渠道", "新增群", "占比"]),
        vec![
            s(&["微博", "14", "37%"]),
            s(&["群内邀请", "10", "26%"]),
            s(&["B 站", "9", "24%"]),
        ],
    )
}

// --- labels -----------------------------------------------------------------------------

#[test]
fn the_worked_example_label_is_exact() {
    let (columns, rows) = channel();
    let ids = ["weibo".to_string(), "invite".into(), "bili".into()];
    let data = TableData {
        columns: &columns,
        rows: &rows,
        row_keys: Some(&ids),
    };
    let t = parts::table(&data, TablePick::Cell { row: 2, col: 1 })
        .unwrap()
        .target(src(), "10 月数据月报", "channels", Some("渠道表"), Some(2));
    assert_eq!(t.label, "10 月数据月报 · 渠道表 · B 站 · 新增群 9");
    let p = t.part.clone().unwrap();
    assert_eq!(p.kind, "cell");
    assert_eq!(
        keys(&p),
        vec![
            ("col".to_string(), PartKey::Index(1)),
            ("row".to_string(), PartKey::Key("bili".into()))
        ]
    );
    assert_eq!(
        t.value,
        Some(json!({"row":"B 站","column":"新增群","display":"9","raw":9}))
    );
    // Without row keys the row is its index.
    let data = TableData {
        columns: &columns,
        rows: &rows,
        row_keys: None,
    };
    let p = parts::table(&data, TablePick::Cell { row: 2, col: 1 })
        .unwrap()
        .part
        .unwrap();
    assert_eq!(
        keys(&p),
        vec![
            ("col".to_string(), PartKey::Index(1)),
            ("row".to_string(), PartKey::Index(2))
        ]
    );
    // The chip draws the source and the block quiet, the part strong.
    let l = label::build("10 月数据月报", Some("渠道表"), Some("B 站 · 新增群 9"));
    assert_eq!(label::split(&l.text, l.lead_chars).1, "B 站 · 新增群 9");
    assert_eq!(label::lead_of(&l.text), l.lead_chars);
}

#[test]
fn segments_are_cut_at_their_limits_and_the_label_at_120() {
    let long = "字".repeat(100);
    let l = label::build(&long, Some(&long), Some(&long));
    let segs: Vec<&str> = l.text.split(label::SEP).collect();
    assert_eq!(segs[0].chars().count(), label::SOURCE_MAX);
    assert!(segs[0].ends_with('…'));
    assert_eq!(segs[1].chars().count(), label::BLOCK_MAX);
    assert!(l.text.chars().count() <= label::LABEL_MAX);
    assert!(l.text.ends_with('…'));
    // Line breaks and control characters never reach a label.
    assert_eq!(label::cut("a\nb\tc\u{7}", 10), "a b c");
    // The whole label is valid on the wire.
    let input = BlockRefInput {
        message_id: Some("m".into()),
        block_id: "x".into(),
        label: l.text,
        ..Default::default()
    };
    assert!(input.validate().is_ok());
}

#[test]
fn a_block_name_is_omitted_for_the_only_block_of_its_type() {
    assert_eq!(parts::block_name(None, parts::BlockKind::Table, 1), None);
    assert_eq!(
        parts::block_name(None, parts::BlockKind::Table, 2).as_deref(),
        Some("表格")
    );
    assert_eq!(
        parts::block_name(Some("渠道表"), parts::BlockKind::Table, 1).as_deref(),
        Some("渠道表")
    );
    let l = label::build("新增群从哪来", None, Some("B 站 · 新增群 9"));
    assert_eq!(l.text, "新增群从哪来 · B 站 · 新增群 9");
}

// --- value snapshots ----------------------------------------------------------------------

#[test]
fn values_are_capped_at_2048_bytes_and_marked_truncated() {
    let small = json!({"a": 1});
    assert_eq!(value::cap(small.clone()), small);
    let big = json!({
        "column": "日活",
        "values": (0..2000).map(|i| format!("值{i}")).collect::<Vec<_>>(),
        "count": 2000
    });
    let capped = value::cap(big);
    let bytes = serde_json::to_vec(&capped).unwrap().len();
    assert!(bytes <= value::MAX_BYTES, "{bytes}");
    assert_eq!(capped["truncated"], Value::Bool(true));
    assert_eq!(capped["column"], "日活");
    let text = json!("长".repeat(3000));
    let capped = value::cap(text);
    assert!(serde_json::to_vec(&capped).unwrap().len() <= value::MAX_BYTES);
    assert_eq!(capped["truncated"], Value::Bool(true));
    // Many small members: a text of them.
    let mut wide = serde_json::Map::new();
    for i in 0..400 {
        wide.insert(format!("k{i}"), json!(i));
    }
    let capped = value::cap(Value::Object(wide));
    assert!(serde_json::to_vec(&capped).unwrap().len() <= value::MAX_BYTES);
    assert_eq!(value::display_number(2340.), "2,340");
    assert_eq!(value::display_number(-1234567.), "-1,234,567");
    assert_eq!(value::display_number(1.25), "1.25");
    assert_eq!(value::parse_number("2,340"), Some(json!(2340)));
    assert_eq!(value::parse_number("24%"), None);
}

// --- the Markdown block counter (REFS §A2.1) ----------------------------------------------

#[test]
fn top_level_blocks_count_headings_lists_fences_and_tables() {
    let body = "# 标题\n\n第一段\n第一段续\n\n- a\n- b\n\n  - c\n\n```rust\nfn x() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n> 引用\n\n---\n\n[x]: https://example.com\n\n    缩进代码\n";
    let kinds: Vec<MdBlock> = top_level_blocks(body).into_iter().map(|(k, _)| k).collect();
    assert_eq!(
        kinds,
        vec![
            MdBlock::Heading,
            MdBlock::Paragraph,
            MdBlock::List,
            MdBlock::Code,
            MdBlock::Table,
            MdBlock::Quote,
            MdBlock::Rule,
            MdBlock::Code,
        ]
    );
    // The quote lines of a message's own references are one block.
    let refs = vec![stored("A · B"), stored("C · D")];
    let full = with_prefix("| a |\n|---|\n| 1 |", &refs);
    let kinds: Vec<MdBlock> = top_level_blocks(&full)
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(kinds, vec![MdBlock::Quote, MdBlock::Table]);
    assert!(top_level_blocks("").is_empty());
}

// --- parts per block type (REFS §A2.2) ----------------------------------------------------

#[test]
fn table_rows_and_columns() {
    let (columns, rows) = channel();
    let data = TableData {
        columns: &columns,
        rows: &rows,
        row_keys: None,
    };
    let row = parts::table(&data, TablePick::Row(0)).unwrap();
    assert_eq!(row.text.as_deref(), Some("微博 · 整行"));
    assert_eq!(row.value.unwrap()["cells"]["占比"], "37%");
    let col = parts::table(&data, TablePick::Column(2)).unwrap();
    assert_eq!(col.text.as_deref(), Some("占比 · 整列"));
    let v = col.value.unwrap();
    assert_eq!(v["count"], 3);
    assert_eq!(v["values"][2], "24%");
    assert!(parts::table(&data, TablePick::Cell { row: 9, col: 0 }).is_none());
    assert!(parts::table(&data, TablePick::Column(5)).is_none());
}

#[test]
fn v1_card_blocks_are_addressed_by_index() {
    let card: crate::data::card::Card = serde_json::from_value(json!({
        "v": 1, "title": "新增群从哪来",
        "blocks": [
            {"type": "text", "text": "10 月新增 38 个群"},
            {"type": "table", "columns": ["渠道", "新增群"], "rows": [["B 站", "9"]]},
            {"type": "divider"},
            {"type": "table", "columns": ["周"], "rows": [["10-27"]]}
        ]
    }))
    .unwrap();
    let (columns, rows) = (
        vec!["渠道".to_string(), "新增群".into()],
        vec![vec!["B 站".to_string(), "9".into()]],
    );
    let data = TableData {
        columns: &columns,
        rows: &rows,
        row_keys: None,
    };
    let part = parts::table(&data, TablePick::Cell { row: 0, col: 1 }).unwrap();
    let t = parts::v1::target(&card, 1, part, src());
    assert_eq!(t.block_id, "v1:1");
    // Two tables: the block is named 表格.
    assert_eq!(t.label, "新增群从哪来 · 表格 · B 站 · 新增群 9");
    assert!(chat_core::proto::is_block_ref_id(&t.block_id));
    // A divider is not referenceable; text is a whole block.
    assert!(parts::v1::whole_block(&card, 2).is_none());
    let whole = parts::v1::whole_block(&card, 0).unwrap();
    assert!(whole.part.is_none());
    assert_eq!(whole.value, Some(json!({"text": "10 月新增 38 个群"})));
}

#[test]
fn chart_points_bars_and_series() {
    let x = ["10-20".to_string(), "10-27".into()];
    let series = [ChartSeriesData {
        key: "dau".into(),
        name: "日活".into(),
        values: vec![Some(2210.), Some(2340.)],
    }];
    let line = ChartData {
        bar: false,
        x: &x,
        series: &series,
        unit: None,
    };
    let p = parts::chart(&line, ChartPick::Point { series: 0, x: 1 }).unwrap();
    assert_eq!(p.text.as_deref(), Some("日活 · 10-27 · 2,340"));
    let r = p.part.unwrap();
    assert_eq!(r.kind, "point");
    assert_eq!(
        keys(&r),
        vec![
            ("series".to_string(), PartKey::Key("dau".into())),
            ("x".to_string(), PartKey::Key("10-27".into()))
        ]
    );
    assert_eq!(p.value.unwrap()["value"], json!(2340.0));
    let bar = ChartData { bar: true, ..line };
    assert_eq!(
        parts::chart(&bar, ChartPick::Point { series: 0, x: 0 })
            .unwrap()
            .part
            .unwrap()
            .kind,
        "bar"
    );
    let s = parts::chart(&line, ChartPick::Series(0)).unwrap();
    assert_eq!(s.text.as_deref(), Some("日活 · 全部"));
    assert_eq!(s.value.unwrap()["values"], json!([2210.0, 2340.0]));
}

#[test]
fn metric_list_step_code_image_evidence_and_node() {
    let m = parts::metric("新增群", 38., None, Some(12.), None);
    assert_eq!(m.text.as_deref(), Some("新增群 38"));
    assert_eq!(m.part.as_ref().unwrap().kind, "metric");
    assert!(m.part.unwrap().keys.is_none());

    let l = parts::list_item(3, None, &"很长的事项".repeat(20), Some("done"));
    assert_eq!(l.text.unwrap().chars().count(), 40);
    assert_eq!(
        keys(&l.part.unwrap()),
        vec![("item".to_string(), PartKey::Index(3))]
    );

    let s = parts::step(2, "跑一万条消息的压测", "working", None);
    assert_eq!(s.text.as_deref(), Some("第 3 步 · 跑一万条消息的压测"));
    assert_eq!(
        keys(&s.part.unwrap()),
        vec![("step".to_string(), PartKey::Index(2))]
    );

    let c = parts::code_line(45, "if (store.has(msg.seq)) continue;", None);
    assert_eq!(c.text.as_deref(), Some("第 45 行"));

    let i = parts::image("f-1", None);
    assert_eq!(i.text.as_deref(), Some("图片"));
    assert_eq!(
        parts::image("f-1", Some("登录页")).text.as_deref(),
        Some("登录页")
    );

    let shot = parts::evidence(1, true, None, Some("f-2"), None);
    assert_eq!(shot.text.as_deref(), Some("截图 2"));
    assert_eq!(shot.part.as_ref().unwrap().kind, "image");
    let check = parts::evidence(0, false, Some("重连测试"), None, Some(true));
    assert_eq!(check.text.as_deref(), Some("重连测试 · 通过"));
    assert_eq!(check.value.unwrap()["passed"], true);

    let n = parts::node("dedupe", "seq 去重", Some("sequence"));
    assert_eq!(n.text.as_deref(), Some("seq 去重"));
    assert_eq!(
        keys(&n.part.unwrap()),
        vec![("node".to_string(), PartKey::Key("dedupe".into()))]
    );
}

#[test]
fn diff_lines_hunks_and_files() {
    let diff = "diff --git a/src/sync/reconnect.ts b/src/sync/reconnect.ts\n--- a/src/sync/reconnect.ts\n+++ b/src/sync/reconnect.ts\n@@ -43,3 +43,4 @@ export\n const batch = await fetchSince(from);\n-for (const msg of batch) store.push(msg);\n+for (const msg of batch) {\n+  if (store.has(msg.seq)) continue;\n";
    let (lines, hunks) = parts::parse_diff(diff, None);
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].file, "src/sync/reconnect.ts");
    let removed = lines.iter().find(|l| l.side == "old").unwrap();
    assert_eq!(removed.n, 44);
    let added = lines.iter().find(|l| l.text.contains("store.has")).unwrap();
    assert_eq!((added.side, added.n), ("new", 45));
    let p = parts::diff_line(added);
    assert_eq!(p.text.as_deref(), Some("reconnect.ts:45"));
    assert_eq!(p.part.as_ref().unwrap().kind, "line");
    let h = parts::diff_hunk(&hunks[0]);
    assert_eq!(h.text.as_deref(), Some("reconnect.ts · 第 1 段"));
    assert!(
        h.value.unwrap()["patch"]
            .as_str()
            .unwrap()
            .contains("store.has")
    );
    let f = parts::diff_file("src/sync/reconnect.ts", "modified", 5, 2);
    assert_eq!(f.text.as_deref(), Some("reconnect.ts"));
    // Every part is valid on the wire.
    for p in [p.part.unwrap(), h.part.unwrap(), f.part.unwrap()] {
        assert!(p.validate().is_ok(), "{p:?}");
    }
}

// --- the body prefix (REFS §A6) -------------------------------------------------------------

fn stored(label: &str) -> BlockRef {
    BlockRef {
        source: BlockRefInput {
            message_id: Some("m".into()),
            block_id: "x".into(),
            label: label.into(),
            ..Default::default()
        },
        source_topic_id: "t".into(),
        ..Default::default()
    }
}

#[test]
fn the_exact_prefix_is_stripped_and_anything_else_shown_unchanged() {
    let refs = vec![stored("10 月数据月报 · 渠道表 · B 站 · 新增群 9")];
    let body = "> ↳ 10 月数据月报 · 渠道表 · B 站 · 新增群 9\n\n为什么比上月少了 1 个？";
    assert_eq!(with_prefix("为什么比上月少了 1 个？", &refs), body);
    assert_eq!(strip_prefix(body, &refs), Some("为什么比上月少了 1 个？"));
    // Chips and an empty text: the body is the prefix only.
    assert_eq!(strip_prefix(&prefix(&refs), &refs), Some(""));
    // Any difference: shown unchanged.
    assert_eq!(strip_prefix("> ↳ 别的\n\n问题", &refs), None);
    assert_eq!(
        strip_prefix("> ↳ 10 月数据月报 · 渠道表 · B 站 · 新增群 9\n问题", &refs),
        None
    );
    assert_eq!(strip_prefix(body, &[]), None);
    // A deleted source: its line says 原消息已撤回.
    let mut gone = stored("");
    gone.deleted = Some(chat_core::proto::True);
    let two = vec![refs[0].clone(), gone];
    let p = prefix(&two);
    assert_eq!(
        p,
        "> ↳ 10 月数据月报 · 渠道表 · B 站 · 新增群 9\n> ↳ 原消息已撤回"
    );
    assert_eq!(strip_prefix(&format!("{p}\n\n好"), &two), Some("好"));
    // The client's prefix is the protocol's.
    assert_eq!(
        prefix(&refs),
        chat_core::proto::block_ref_prefix(["10 月数据月报 · 渠道表 · B 站 · 新增群 9"])
    );
}

#[test]
fn side_chat_questions_carry_their_chips_as_quote_lines() {
    let r = crate::data::DraftRef {
        input: stored("新增群从哪来 · B 站 · 新增群 9").source,
        group_id: "g".into(),
        topic_id: "t".into(),
        lead_chars: 0,
    };
    let body = side_chat_body("为什么？", std::slice::from_ref(&r));
    assert_eq!(body, "> ↳ 新增群从哪来 · B 站 · 新增群 9\n\n为什么？");
    let (labels, rest) = split_quote_lines(&body);
    assert_eq!(labels, vec!["新增群从哪来 · B 站 · 新增群 9".to_string()]);
    assert_eq!(rest, "为什么？");
    assert_eq!(split_quote_lines("没有引用"), (vec![], "没有引用"));
    let only = side_chat_body("", &[r]);
    assert_eq!(split_quote_lines(&only).1, "");
}

// --- who is asked (REFS §A5) ----------------------------------------------------------------

#[test]
fn the_addressee_is_the_author_bot_else_the_responsible_bot_else_nobody() {
    use crate::fake::v2 as d;
    let members = d::members();
    let m = |id: &str| members.iter().find(|m| m.id == id).cloned();
    let (ledger, forge, joye) = (m(d::LEDGER), m(d::FORGE), m(d::ME));
    // The author is a bot and responsible (or nobody is): the author.
    assert_eq!(
        addressee(ledger.as_ref(), ledger.as_ref()).as_deref(),
        Some(d::LEDGER)
    );
    assert_eq!(addressee(ledger.as_ref(), None).as_deref(), Some(d::LEDGER));
    // Another bot is responsible: that bot.
    assert_eq!(
        addressee(ledger.as_ref(), forge.as_ref()).as_deref(),
        Some(d::FORGE)
    );
    // A person wrote the part, a bot is responsible: the bot.
    assert_eq!(
        addressee(joye.as_ref(), forge.as_ref()).as_deref(),
        Some(d::FORGE)
    );
    // A person wrote it and a person is responsible: nobody.
    assert_eq!(addressee(joye.as_ref(), joye.as_ref()), None);
    // A deactivated bot is never asked.
    let mut off = ledger.clone().unwrap();
    off.deactivated = true;
    assert_eq!(addressee(Some(&off), None), None);
    assert!(super::names_someone("@Ledger 看下", &[]));
    assert!(!super::names_someone("看下", &[]));
}

#[test]
fn a_target_becomes_a_valid_send_input() {
    let (columns, rows) = channel();
    let data = TableData {
        columns: &columns,
        rows: &rows,
        row_keys: None,
    };
    let t = parts::table(&data, TablePick::Cell { row: 0, col: 0 })
        .unwrap()
        .target(src(), "卡", "v1:0", None, Some(3));
    let input = t.input();
    assert!(input.validate().is_ok());
    assert!(chat_core::proto::check_block_refs(Some(std::slice::from_ref(&input))).is_ok());
    assert_eq!(input.card_rev, Some(3));
    assert_eq!(input.message_id.as_deref(), Some("m-1"));
    assert_eq!(t.address(), super::address_of(&input));
}
