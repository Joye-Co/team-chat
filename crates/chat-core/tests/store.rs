mod common;

use chat_core::Store;
use chat_core::proto::*;
use common::*;
use std::time::Instant;

fn open() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("cache.sqlite3")).unwrap();
    store
        .apply_welcome(
            &member(ME, "me"),
            &[member(ME, "me"), member(ALICE, "alice")],
            &[summary(group(G1, &[ME, ALICE]), 0, 0, 0)],
        )
        .unwrap();
    (dir, store)
}

fn seed_topic(store: &Store) {
    let out = store
        .apply_events(
            G1,
            &[
                topic_created(1, G1, topic("T1", G1, "M1", 1)),
                msg_created(2, message("M1", G1, "T1", ME, 2, "root")),
            ],
        )
        .unwrap();
    assert_eq!(out.applied, 2);
}

#[test]
fn migrations_are_named_and_run_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.sqlite3");
    drop(Store::open(&path).unwrap());
    let s = Store::open(&path).unwrap();
    let names: Vec<String> = s
        .with_reader(|c| {
            let mut st = c.prepare("SELECT name FROM migrations ORDER BY name")?;
            st.query_map([], |r| r.get(0))?.collect()
        })
        .unwrap();
    assert_eq!(
        names,
        vec![
            "asks-1",
            "characters-1",
            "core-1",
            "ext-1",
            "members-2",
            "roles-1",
            "topic-mute-1",
            "v11-1",
            "v11-2",
            "v11-3",
            "v11-files-1"
        ]
    );
    let mode: String = s
        .with_reader(|c| c.query_row("PRAGMA journal_mode", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(mode, "wal");
}

#[test]
fn a_cache_from_before_ext_1_upgrades_and_keeps_its_rows() {
    use chat_core::store::migrations::{MIGRATIONS, run};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.sqlite3");
    {
        let mut conn = rusqlite::Connection::open(&path).unwrap();
        run(&mut conn, &MIGRATIONS[..1]).unwrap();
        conn.execute(
            "INSERT INTO members (id, kind, name, level) VALUES ('M1', 'human', 'old', 'member')",
            [],
        )
        .unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.member("M1").unwrap().unwrap().name, "old");
    assert!(s.artifacts("T1").unwrap().is_empty());
    assert!(s.runs("T1").unwrap().is_empty());
}

#[test]
fn a_cache_from_v11_2_upgrades_and_keeps_its_answers() {
    use chat_core::store::migrations::{MIGRATIONS, run};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.sqlite3");
    {
        let mut conn = rusqlite::Connection::open(&path).unwrap();
        let upto = MIGRATIONS.iter().position(|m| m.name == "v11-2").unwrap();
        run(&mut conn, &MIGRATIONS[..=upto]).unwrap();
        conn.execute(
            "INSERT INTO card_answers (group_id, seq, message_id, rev, actor_id, action_id, at, answer_id)
             VALUES ('G1', 5, 'C1', 1, 'M1', 'ok', 1000, 'AN5')",
            [],
        )
        .unwrap();
    }
    let s = Store::open(&path).unwrap();
    let a = &s.card_answers("C1").unwrap()[0];
    assert_eq!(
        (a.action_id.as_str(), a.answer_id.as_deref()),
        ("ok", Some("AN5"))
    );
    assert_eq!(
        (a.event.as_deref(), a.block_id.as_deref(), a.value.as_ref()),
        (None, None, None)
    );
}

#[test]
fn member_characters_are_stored_changed_and_cleared() {
    let (_d, s) = open();
    assert_eq!(s.member(ALICE).unwrap().unwrap().character(), None);
    let bot = Member {
        kind: MemberKind::Bot,
        character: Some(Some(CharacterId::O03)),
        ..member("B1", "Tata")
    };
    s.apply_directory(Some(std::slice::from_ref(&bot)), None, None)
        .unwrap();
    assert_eq!(
        s.member("B1").unwrap().unwrap().character(),
        Some(CharacterId::O03)
    );
    assert!(
        s.members()
            .unwrap()
            .iter()
            .any(|m| m.id == "B1" && m.character() == Some(CharacterId::O03))
    );
    // The server omits the field once it is cleared; the directory replaces the row.
    let cleared = Member {
        character: None,
        ..bot
    };
    s.apply_directory(Some(&[cleared]), None, None).unwrap();
    assert_eq!(s.member("B1").unwrap().unwrap().character(), None);
    // An id this build does not know (written by a newer build) reads as none.
    s.with_reader(|c| c.execute("UPDATE members SET character = 'zz9' WHERE id = 'B1'", []))
        .unwrap();
    assert_eq!(s.member("B1").unwrap().unwrap().character(), None);
}

#[test]
fn member_reports_to_and_weekly_budget_are_stored_and_cleared() {
    let (_d, s) = open();
    let bot = Member {
        kind: MemberKind::Bot,
        reports_to: Some(ALICE.into()),
        weekly_budget_cents: Some(5000),
        ..member("B2", "Forge")
    };
    s.apply_directory(Some(std::slice::from_ref(&bot)), None, None)
        .unwrap();
    let got = s.member("B2").unwrap().unwrap();
    assert_eq!(got.reports_to.as_deref(), Some(ALICE));
    assert_eq!(got.weekly_budget_cents, Some(5000));
    // The server omits both once they are cleared; the directory replaces the row.
    let cleared = Member {
        reports_to: None,
        weekly_budget_cents: None,
        ..bot
    };
    s.apply_directory(Some(&[cleared]), None, None).unwrap();
    let got = s.member("B2").unwrap().unwrap();
    assert_eq!((got.reports_to, got.weekly_budget_cents), (None, None));
}

#[test]
fn member_team_owner_is_stored_and_cleared() {
    let (_d, s) = open();
    let owner = Member {
        level: MemberLevel::Owner,
        team_owner: Some(true),
        ..member("O1", "Ana")
    };
    s.apply_directory(Some(std::slice::from_ref(&owner)), None, None)
        .unwrap();
    assert!(s.member("O1").unwrap().unwrap().is_team_owner());
    // Made an admin: the server stops sending the mark.
    let admin = Member {
        team_owner: None,
        ..owner
    };
    s.apply_directory(Some(&[admin]), None, None).unwrap();
    let got = s.member("O1").unwrap().unwrap();
    assert_eq!((got.level, got.team_owner), (MemberLevel::Owner, None));
}

#[test]
fn a_cache_from_before_roles_1_reads_owners_as_admins() {
    use chat_core::store::migrations::{MIGRATIONS, run};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.sqlite3");
    {
        let mut conn = rusqlite::Connection::open(&path).unwrap();
        let n = MIGRATIONS.iter().position(|m| m.name == "roles-1").unwrap();
        run(&mut conn, &MIGRATIONS[..n]).unwrap();
        conn.execute(
            "INSERT INTO members (id, kind, name, level) VALUES ('O1', 'human', 'old', 'owner')",
            [],
        )
        .unwrap();
    }
    let s = Store::open(&path).unwrap();
    let m = s.member("O1").unwrap().unwrap();
    assert_eq!((m.level, m.is_team_owner()), (MemberLevel::Owner, false));
}

#[test]
fn a_cache_from_before_characters_1_upgrades_and_keeps_its_members() {
    use chat_core::store::migrations::{MIGRATIONS, run};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.sqlite3");
    {
        let mut conn = rusqlite::Connection::open(&path).unwrap();
        run(&mut conn, &MIGRATIONS[..2]).unwrap();
        conn.execute(
            "INSERT INTO members (id, kind, name, role, level) VALUES ('B1', 'bot', 'Jojo', '主助手', 'owner')",
            [],
        )
        .unwrap();
    }
    let s = Store::open(&path).unwrap();
    let m = s.member("B1").unwrap().unwrap();
    assert_eq!(
        (m.name.as_str(), m.role(), m.character()),
        ("Jojo", Some("主助手"), None)
    );
}

#[test]
fn events_apply_in_seq_order_and_stop_at_gaps() {
    let (_d, s) = open();
    seed_topic(&s);
    // 4 and 5 arrive before 3: nothing past the gap is applied.
    let out = s
        .apply_events(
            G1,
            &[
                msg_created(5, message("M5", G1, "T1", ALICE, 5, "five")),
                msg_created(4, message("M4", G1, "T1", ALICE, 4, "four")),
            ],
        )
        .unwrap();
    assert_eq!(out.applied, 0);
    assert_eq!(out.gap_at, Some(4));
    assert_eq!(s.last_applied(G1).unwrap(), 2);
    assert!(s.message("M4").unwrap().is_none());

    // With 3 the whole run applies, in any input order.
    let out = s
        .apply_events(
            G1,
            &[
                msg_created(5, message("M5", G1, "T1", ALICE, 5, "five")),
                msg_created(3, message("M3", G1, "T1", ALICE, 3, "three")),
                msg_created(4, message("M4", G1, "T1", ALICE, 4, "four")),
            ],
        )
        .unwrap();
    assert_eq!(out.applied, 3);
    assert_eq!(out.gap_at, None);
    assert_eq!(s.last_applied(G1).unwrap(), 5);
    let bodies: Vec<String> = s
        .messages_before("T1", None, 10)
        .unwrap()
        .into_iter()
        .map(|m| m.body)
        .collect();
    assert_eq!(bodies, vec!["root", "three", "four", "five"]);
    assert_eq!(s.cursors().unwrap().get(G1), Some(&5));
}

#[test]
fn applying_twice_is_idempotent() {
    let (_d, s) = open();
    seed_topic(&s);
    let batch = vec![
        msg_created(3, message("M3", G1, "T1", ALICE, 3, "a")),
        msg_created(4, message("M4", G1, "T1", ALICE, 4, "b")),
    ];
    assert_eq!(s.apply_events(G1, &batch).unwrap().applied, 2);
    let topic_before = s.topic("T1").unwrap().unwrap();
    let group_before = s.group(G1).unwrap().unwrap();
    // Same events again, also duplicated inside one batch.
    let mut again = batch.clone();
    again.extend(batch.clone());
    let out = s.apply_events(G1, &again).unwrap();
    assert_eq!(out.applied, 0);
    assert_eq!(s.topic("T1").unwrap().unwrap(), topic_before);
    assert_eq!(s.group(G1).unwrap().unwrap(), group_before);
    assert_eq!(s.message_count_cached("T1").unwrap(), 3);
    assert_eq!(topic_before.topic.message_count, 3);
}

#[test]
fn edits_and_deletes_keep_rows() {
    let (_d, s) = open();
    seed_topic(&s);
    s.apply_events(
        G1,
        &[
            msg_created(3, message("M3", G1, "T1", ALICE, 3, "draft")),
            ev(
                4,
                G1,
                ALICE,
                EventData::MessageUpdated(MessageUpdatedData {
                    message_id: "M3".into(),
                    body: "final".into(),
                    edited_at: 9,
                    streaming: false,
                    ..Default::default()
                }),
            ),
            ev(
                5,
                G1,
                ALICE,
                EventData::MessageDeleted(MessageDeletedData {
                    message_id: "M1".into(),
                    deleted_at: 10,
                }),
            ),
            ev(
                6,
                G1,
                ME,
                EventData::TopicUpdated(TopicUpdatedData {
                    topic_id: "T1".into(),
                    title: Some("new".into()),
                    status: Some(TopicStatus::WaitingReview),
                    ..Default::default()
                }),
            ),
            ev(
                7,
                G1,
                ME,
                EventData::GroupUpdated(GroupUpdatedData {
                    purpose: None,
                    name: "renamed".into(),
                }),
            ),
            ev(
                8,
                G1,
                ME,
                EventData::GroupMemberRemoved(GroupMemberChangeData {
                    member_id: ALICE.into(),
                }),
            ),
        ],
    )
    .unwrap();
    let m3 = s.message("M3").unwrap().unwrap();
    assert_eq!((m3.body.as_str(), m3.edited_at), ("final", Some(Some(9))));
    let m1 = s.message("M1").unwrap().unwrap();
    assert_eq!((m1.body.as_str(), m1.deleted_at), ("", Some(Some(10))));
    // A late edit does not bring a deleted body back.
    s.apply_events(
        G1,
        &[ev(
            9,
            G1,
            ME,
            EventData::MessageUpdated(MessageUpdatedData {
                message_id: "M1".into(),
                body: "x".into(),
                edited_at: 11,
                streaming: false,
                ..Default::default()
            }),
        )],
    )
    .unwrap();
    assert_eq!(s.message("M1").unwrap().unwrap().body, "");
    let t = s.topic("T1").unwrap().unwrap().topic;
    assert_eq!(
        (t.title.as_str(), t.status),
        ("new", TopicStatus::WaitingReview)
    );
    let g = s.group(G1).unwrap().unwrap();
    assert_eq!(g.group.name, "renamed");
    assert_eq!(g.group.member_ids, vec![ME.to_string()]);
}

#[test]
fn unread_counts_follow_summary_events_and_cursors() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(&dir.path().join("c.sqlite3")).unwrap();
    // The server says 1 unread up to seq 2; seq 1..2 are not cached yet.
    s.apply_welcome(
        &member(ME, "me"),
        &[],
        &[summary(group(G1, &[ME, ALICE]), 2, 1, 1)],
    )
    .unwrap();
    s.apply_events(
        G1,
        &[
            topic_created(1, G1, topic("T1", G1, "M1", 1)),
            msg_created(2, message("M1", G1, "T1", ALICE, 2, "root")),
        ],
    )
    .unwrap();
    // Catch-up events covered by the summary do not count twice.
    assert_eq!(s.group(G1).unwrap().unwrap().unread, 1);
    let mut m = message("M3", G1, "T1", ALICE, 3, "hi @me");
    m.mentions = vec![ME.into()];
    s.apply_events(
        G1,
        &[
            msg_created(3, m),
            msg_created(4, message("M4", G1, "T1", ME, 4, "mine")),
        ],
    )
    .unwrap();
    let g = s.group(G1).unwrap().unwrap();
    assert_eq!((g.unread, g.mentions), (2, 1));
    assert_eq!(s.topics(G1, 10).unwrap()[0].unread, 2);
    // Reading the topic to seq 3 clears it; the group cursor stays.
    let sent = s.mark_read(G1, Some("T1"), 3).unwrap().unwrap();
    assert_eq!((sent.topic_id.as_deref(), sent.seq), (Some("T1"), 3));
    let g = s.group(G1).unwrap().unwrap();
    assert_eq!((g.unread, g.mentions, g.read_seq), (0, 0, 1));
    assert_eq!(s.unsynced_reads().unwrap(), vec![sent.clone()]);
    s.mark_read_synced(&sent).unwrap();
    assert!(s.unsynced_reads().unwrap().is_empty());
    // A cursor never moves back, and never past the newest seq.
    assert_eq!(s.mark_read(G1, Some("T1"), 2).unwrap(), None);
    let g_read = s.mark_read(G1, None, 99).unwrap().unwrap();
    assert_eq!(g_read.seq, 4);
    assert_eq!(s.group(G1).unwrap().unwrap().read_seq, 4);
}

#[test]
fn deleted_and_system_messages_are_not_unread_and_lose_content() {
    let (_d, s) = open();
    seed_topic(&s);
    let mut sys = message("MS", G1, "T1", ALICE, 3, "alice joined");
    sys.kind = Some(MessageKind::System);
    s.apply_events(
        G1,
        &[
            msg_created(3, sys),
            msg_created(4, message("M4", G1, "T1", ALICE, 4, "secret")),
            ev(
                5,
                G1,
                ALICE,
                EventData::MessageDeleted(MessageDeletedData {
                    message_id: "M4".into(),
                    deleted_at: 10,
                }),
            ),
        ],
    )
    .unwrap();
    let t = s.topic("T1").unwrap().unwrap();
    assert_eq!(t.unread, 0);
    let m4 = s.message("M4").unwrap().unwrap();
    assert!(m4.body.is_empty() && m4.card.is_none() && m4.is_deleted());
    let raw: String = s
        .with_reader(|c| c.query_row("SELECT raw FROM messages WHERE id = 'M4'", [], |r| r.get(0)))
        .unwrap();
    assert!(!raw.contains("secret"));
}

#[test]
fn welcome_topic_states_and_reads_frames_set_per_topic_positions() {
    let (_d, s) = open();
    s.apply_events(
        G1,
        &[
            topic_created(1, G1, topic("TA", G1, "MA", 1)),
            msg_created(2, message("MA", G1, "TA", ALICE, 2, "a")),
            topic_created(3, G1, topic("TB", G1, "MB", 3)),
            msg_created(4, message("MB", G1, "TB", ALICE, 4, "b")),
        ],
    )
    .unwrap();
    // The server lists only TB (unread, followed): TA is read up to lastSeq.
    let mut sum = summary(group(G1, &[ME, ALICE]), 4, 0, 1);
    sum.topics = Some(vec![TopicState {
        topic_id: "TB".into(),
        read_seq: 0,
        unread: 1,
        mentions: 0,
        following: Some(true),
        muted: Some(true),
    }]);
    s.apply_welcome(&member(ME, "me"), &[], &[sum]).unwrap();
    let rows = s.topics(G1, 10).unwrap();
    let get = |id: &str| rows.iter().find(|t| t.topic.id == id).unwrap().clone();
    assert_eq!(
        (get("TA").unread, get("TA").following, get("TA").muted),
        (0, false, false)
    );
    assert_eq!(
        (get("TB").unread, get("TB").following, get("TB").muted),
        (1, true, true)
    );
    // A reads frame from another device reads TB.
    s.apply_reads(
        &ReadsFrame {
            group_id: G1.into(),
            read_seq: 0,
            unread: 0,
            mentions: 0,
            topics: vec![TopicState {
                topic_id: "TB".into(),
                read_seq: 4,
                unread: 0,
                mentions: 0,
                following: Some(true),
                muted: None,
            }],
        },
        4,
    )
    .unwrap();
    assert_eq!(s.topic("TB").unwrap().unwrap().unread, 0);
    // Unmuted on another device: following stays.
    let tb = s.topic("TB").unwrap().unwrap();
    assert_eq!((tb.following, tb.muted), (true, false));
    // The ack of topic.mute mutes TA without touching its read position.
    s.apply_topic_state(
        G1,
        &TopicState {
            topic_id: "TA".into(),
            read_seq: 4,
            unread: 0,
            mentions: 0,
            following: None,
            muted: Some(true),
        },
    )
    .unwrap();
    let ta = s.topic("TA").unwrap().unwrap();
    assert_eq!((ta.following, ta.muted), (false, true));
    assert_eq!(s.group(G1).unwrap().unwrap().unread, 0);
    // A later message counts once.
    s.apply_events(
        G1,
        &[msg_created(5, message("M5", G1, "TA", ALICE, 5, "new"))],
    )
    .unwrap();
    assert_eq!(s.group(G1).unwrap().unwrap().unread, 1);
    assert_eq!(s.topic("TA").unwrap().unwrap().unread, 1);
}

#[test]
fn unknown_log_entries_keep_seq_order() {
    let (_d, s) = open();
    seed_topic(&s);
    let out = s
        .apply_log(
            G1,
            &[
                chat_core::LogEntry::Known(msg_created(
                    4,
                    message("M4", G1, "T1", ALICE, 4, "after"),
                )),
                chat_core::LogEntry::Unknown {
                    seq: 3,
                    group_id: G1.into(),
                    event_type: "future.thing".into(),
                },
            ],
        )
        .unwrap();
    assert_eq!((out.applied, out.last_applied), (2, 4));
    assert!(s.message("M4").unwrap().is_some());
}

#[test]
fn topic_updates_keep_v1_fields() {
    let (_d, s) = open();
    seed_topic(&s);
    s.apply_events(
        G1,
        &[ev(
            3,
            G1,
            ME,
            EventData::TopicUpdated(TopicUpdatedData {
                topic_id: "T1".into(),
                status: Some(TopicStatus::WaitingAnswer),
                waiting_on: Some(vec![ALICE.into()]),
                owner_id: Some(Some(ME.into())),
                ..Default::default()
            }),
        )],
    )
    .unwrap();
    let t = s.topic("T1").unwrap().unwrap().topic;
    assert_eq!(t.waiting_on, Some(vec![ALICE.to_string()]));
    assert_eq!(t.owner_id, Some(Some(ME.to_string())));
    assert_eq!(t.last_message_at, Some(1_002));
    s.apply_events(
        G1,
        &[ev(
            4,
            G1,
            ME,
            EventData::TopicUpdated(TopicUpdatedData {
                topic_id: "T1".into(),
                owner_id: Some(None),
                ..Default::default()
            }),
        )],
    )
    .unwrap();
    let t = s.topic("T1").unwrap().unwrap().topic;
    assert_eq!(t.owner_id, Some(None));
    assert_eq!(t.waiting_on, Some(vec![ALICE.to_string()]));
}

fn relay(state: &str, updated_at: u64) -> TopicRelay {
    TopicRelay {
        id: "R1".into(),
        started_by: ME.into(),
        goal: "加深色模式".into(),
        current: 1,
        state: state.into(),
        reason: None,
        stopped_by: None,
        updated_at,
        steps: vec![
            TopicRelayStep {
                member_id: "B1".into(),
                note: None,
                state: "done".into(),
                run_id: Some("RUN1".into()),
            },
            TopicRelayStep {
                member_id: "B2".into(),
                note: Some("审".into()),
                state: "working".into(),
                run_id: None,
            },
        ],
    }
}

#[test]
fn topic_updates_keep_participants_and_the_relay() {
    let (_d, s) = open();
    seed_topic(&s);
    let who = vec![TopicParticipant {
        member_id: "B1".into(),
        working: true,
        last_run_at: 5,
    }];
    let upd = |seq: u64, data: TopicUpdatedData| ev(seq, G1, ME, EventData::TopicUpdated(data));
    s.apply_events(
        G1,
        &[upd(
            3,
            TopicUpdatedData {
                topic_id: "T1".into(),
                participants: Some(who.clone()),
                relay: Some(Some(relay("running", 10))),
                ..Default::default()
            },
        )],
    )
    .unwrap();
    let t = s.topic("T1").unwrap().unwrap().topic;
    assert_eq!(t.participants, Some(who.clone()));
    assert_eq!(t.relay, Some(relay("running", 10)));
    // An event without the fields keeps them; a null relay removes it.
    s.apply_events(
        G1,
        &[upd(
            4,
            TopicUpdatedData {
                topic_id: "T1".into(),
                status: Some(TopicStatus::Done),
                ..Default::default()
            },
        )],
    )
    .unwrap();
    let t = s.topic("T1").unwrap().unwrap().topic;
    assert_eq!(t.participants, Some(who.clone()));
    assert!(t.relay.is_some());
    s.apply_events(
        G1,
        &[upd(
            5,
            TopicUpdatedData {
                topic_id: "T1".into(),
                relay: Some(None),
                ..Default::default()
            },
        )],
    )
    .unwrap();
    let t = s.topic("T1").unwrap().unwrap().topic;
    assert_eq!(t.relay, None);
    assert_eq!(t.participants, Some(who));
}

#[test]
fn a_route_answer_never_replaces_a_newer_relay() {
    let (_d, s) = open();
    seed_topic(&s);
    assert!(s.store_topic_relay("T1", &relay("stopped", 20)).unwrap());
    // An older answer of the same relay (the event came first) stays out.
    assert!(!s.store_topic_relay("T1", &relay("running", 10)).unwrap());
    let t = s.topic("T1").unwrap().unwrap().topic;
    assert_eq!(t.relay.map(|r| r.state), Some("stopped".to_string()));
    // Another relay replaces it.
    let mut other = relay("running", 5);
    other.id = "R2".into();
    assert!(s.store_topic_relay("T1", &other).unwrap());
    assert!(!s.store_topic_relay("T-missing", &other).unwrap());
}

#[test]
fn per_topic_reads_do_not_clear_other_topics() {
    let (_d, s) = open();
    s.apply_events(
        G1,
        &[
            topic_created(1, G1, topic("TA", G1, "MA", 1)),
            msg_created(2, message("MA", G1, "TA", ALICE, 2, "a")),
            topic_created(3, G1, topic("TB", G1, "MB", 3)),
            msg_created(4, message("MB", G1, "TB", ALICE, 4, "b")),
        ],
    )
    .unwrap();
    assert_eq!(s.group(G1).unwrap().unwrap().unread, 2);
    assert!(s.mark_read(G1, Some("TB"), 4).unwrap().is_some());
    let topics = s.topics(G1, 10).unwrap();
    assert_eq!(
        topics
            .iter()
            .map(|t| (t.topic.id.as_str(), t.unread))
            .collect::<Vec<_>>(),
        vec![("TB", 0), ("TA", 1)]
    );
    assert_eq!(s.group(G1).unwrap().unwrap().unread, 1);
}

#[test]
fn welcome_removes_groups_that_are_gone() {
    let (_d, s) = open();
    seed_topic(&s);
    s.apply_welcome(&member(ME, "me"), &[], &[]).unwrap();
    assert!(s.groups().unwrap().is_empty());
    assert!(s.message("M1").unwrap().is_none());
    assert_eq!(s.last_applied(G1).unwrap(), 0);
}

#[test]
fn perf_insert_10k_and_query_window_of_200() {
    let (_d, s) = open();
    seed_topic(&s);
    let events: Vec<Event> = (3..10_003)
        .map(|seq| {
            msg_created(
                seq,
                message(
                    &format!("M{seq:06}"),
                    G1,
                    "T1",
                    ALICE,
                    seq,
                    &format!("message number {seq} 你好"),
                ),
            )
        })
        .collect();
    let t0 = Instant::now();
    for chunk in events.chunks(500) {
        s.apply_events(G1, chunk).unwrap();
    }
    let insert = t0.elapsed();
    assert_eq!(s.message_count_cached("T1").unwrap(), 10_001);

    // Warm the statement cache once, then time newest, middle and older windows.
    s.messages_before("T1", None, 200).unwrap();
    let mut worst = std::time::Duration::ZERO;
    for before in [None, Some(5_000), Some(300)] {
        let t = Instant::now();
        let w = s.messages_before("T1", before, 200).unwrap();
        let e = t.elapsed();
        worst = worst.max(e);
        assert!(!w.is_empty());
        assert!(w.windows(2).all(|p| p[0].created_seq < p[1].created_seq));
    }
    let t = Instant::now();
    let topics = s.topics(G1, 50).unwrap();
    let topics_time = t.elapsed();
    println!(
        "perf: insert 10000 messages {:.1} ms; window of 200 worst {:.3} ms; topic list {:.3} ms",
        insert.as_secs_f64() * 1e3,
        worst.as_secs_f64() * 1e3,
        topics_time.as_secs_f64() * 1e3
    );
    assert_eq!(topics.len(), 1);
    assert!(worst.as_millis() < 10, "window query took {worst:?}");
}

#[test]
fn a_topic_read_in_a_long_group_takes_only_that_topic_off_the_totals() {
    let (_d, s) = open();
    s.apply_events(
        G1,
        &[
            topic_created(1, G1, topic("TA", G1, "MA", 1)),
            msg_created(2, message("MA", G1, "TA", ALICE, 2, "a")),
        ],
    )
    .unwrap();
    // 20,000 unread messages in topic TA, then a topic TB with two, one a mention of me.
    let events: Vec<Event> = (3..20_003)
        .map(|seq| {
            msg_created(
                seq,
                message(&format!("M{seq:06}"), G1, "TA", ALICE, seq, "x"),
            )
        })
        .collect();
    for chunk in events.chunks(500) {
        s.apply_events(G1, chunk).unwrap();
    }
    let mut mention = message("MB2", G1, "TB", ALICE, 20_005, "@me");
    mention.mentions = vec![ME.into()];
    s.apply_events(
        G1,
        &[
            topic_created(20_003, G1, topic("TB", G1, "MB1", 20_003)),
            msg_created(20_004, message("MB1", G1, "TB", ALICE, 20_004, "b")),
            msg_created(20_005, mention),
        ],
    )
    .unwrap();
    let g = s.group(G1).unwrap().unwrap();
    assert_eq!((g.unread, g.mentions), (20_003, 1));
    let t = Instant::now();
    assert!(s.mark_read(G1, Some("TB"), 20_004).unwrap().is_some());
    let first = t.elapsed();
    let g = s.group(G1).unwrap().unwrap();
    assert_eq!((g.unread, g.mentions), (20_002, 1));
    let t = Instant::now();
    assert!(s.mark_read(G1, Some("TB"), 20_005).unwrap().is_some());
    let second = t.elapsed();
    let g = s.group(G1).unwrap().unwrap();
    assert_eq!((g.unread, g.mentions), (20_001, 0));
    println!(
        "perf: topic read in a 20k group {:.3} ms, {:.3} ms",
        first.as_secs_f64() * 1e3,
        second.as_secs_f64() * 1e3
    );
    assert!(second.as_millis() < 10, "topic read took {second:?}");
}

#[test]
fn group_purpose_and_desk_notes_sync_from_the_log() {
    let (_d, s) = open();
    let purpose = |p: Option<&str>| GroupUpdatedData {
        name: format!("group {G1}"),
        purpose: p.map(str::to_string),
    };
    let note = |text: &str| EventData::DeskNoted(DeskNotedData { text: text.into() });
    s.apply_events(
        G1,
        &[
            ev(1, G1, ME, EventData::GroupUpdated(purpose(Some("开发")))),
            ev(2, G1, ALICE, note("先发 1.1")),
            ev(3, G1, ALICE, note("今天先把 1.1 发出去")),
        ],
    )
    .unwrap();
    assert_eq!(
        s.group(G1).unwrap().unwrap().group.purpose.as_deref(),
        Some("开发")
    );
    let notes = s.desk_notes().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(
        (
            notes[0].member_id.as_str(),
            notes[0].text.as_str(),
            notes[0].seq
        ),
        (ALICE, "今天先把 1.1 发出去", 3)
    );
    // A rename alone keeps the purpose; "" clears both.
    s.apply_events(
        G1,
        &[
            ev(4, G1, ME, EventData::GroupUpdated(purpose(None))),
            ev(5, G1, ALICE, note("")),
        ],
    )
    .unwrap();
    assert_eq!(
        s.group(G1).unwrap().unwrap().group.purpose.as_deref(),
        Some("开发")
    );
    assert!(s.desk_notes().unwrap().is_empty());
    s.apply_events(
        G1,
        &[ev(6, G1, ME, EventData::GroupUpdated(purpose(Some(""))))],
    )
    .unwrap();
    assert_eq!(s.group(G1).unwrap().unwrap().group.purpose, None);
}
