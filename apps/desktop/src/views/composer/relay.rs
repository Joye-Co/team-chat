//! The composer's 按顺序做 lines (docs/plans/usable/design-relay.zh.md §3.6): what an order
//! written as `@A … → @B …` will do, a hint when a message names two or more AI members, a
//! note that the order works only in a 话题, and why the server refused one. The composer
//! never rewrites the message: the server reads the order.

use crate::{
    copy::{fill, relay as c},
    data::{Member, RelayRefusal},
};

/// The separators of an order on a message's first line (the server reads the same).
pub const ARROWS: [&str; 3] = ["→", "->", "=>"];

/// One line above the field, and whether it is an error (red) or a hint (muted).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelayLine {
    pub text: String,
    pub error: bool,
}

/// The member a step names: after its `@`, the longest member name it starts with, or the
/// word up to the first space.
fn step_name(segment: &str, members: &[Member]) -> Option<String> {
    let rest = segment.trim().strip_prefix('@')?;
    let known = members
        .iter()
        .filter(|m| !m.name.is_empty() && rest.starts_with(m.name.as_str()))
        .max_by_key(|m| m.name.len())
        .map(|m| m.name.clone());
    known.or_else(|| {
        let word: String = rest.chars().take_while(|c| !c.is_whitespace()).collect();
        (!word.is_empty()).then_some(word)
    })
}

/// The names of the steps when the first line of `text` is an order (`@A → @B …`, 2 or
/// more steps, each starting with an @); `None` otherwise.
pub fn order_names(text: &str, members: &[Member]) -> Option<Vec<String>> {
    let first = text.lines().next()?.trim();
    if !first.starts_with('@') {
        return None;
    }
    let mut segments = vec![first.to_string()];
    for arrow in ARROWS {
        segments = segments
            .iter()
            .flat_map(|s| s.split(arrow).map(str::to_string).collect::<Vec<_>>())
            .collect();
    }
    if segments.len() < 2 {
        return None;
    }
    segments.iter().map(|s| step_name(s, members)).collect()
}

/// The distinct AI members a draft names: picked from the @ list or typed as `@name`.
pub fn ai_members_named(text: &str, picked: &[String], members: &[Member]) -> usize {
    members
        .iter()
        .filter(|m| m.is_bot() && !m.name.is_empty())
        .filter(|m| picked.contains(&m.id) || text.contains(&format!("@{}", m.name)))
        .count()
}

/// Why the server refused my last message as 按顺序做; `None` for a reason this version
/// does not know (the failed message says it did not go out).
pub fn refusal_text(r: &RelayRefusal, name: impl Fn(&str) -> String) -> Option<String> {
    match r.reason.as_str() {
        "relay.member_unavailable" => {
            let names: Vec<String> = r
                .member_ids
                .iter()
                .map(|id| name(id))
                .filter(|n| !n.is_empty())
                .collect();
            Some(fill(
                c::RELAY_WHY_UNAVAILABLE,
                &[("name", &names.join(c::RELAY_NAMES_SEP))],
            ))
        }
        "relay.running" => Some(c::RELAY_HINT_RUNNING.to_string()),
        "relay.bad_steps" => Some(c::RELAY_REFUSED_BAD_STEPS.to_string()),
        _ => None,
    }
}

/// The 按顺序做 line of a draft. `in_chat`: the 群聊, where an order does not work;
/// `relay_live`: the 话题 already runs (or paused) one.
pub fn relay_line(
    text: &str,
    picked: &[String],
    members: &[Member],
    in_chat: bool,
    relay_live: bool,
) -> Option<RelayLine> {
    let hint = |text: String| Some(RelayLine { text, error: false });
    if let Some(names) = order_names(text, members) {
        if in_chat {
            return hint(c::RELAY_HINT_IN_TOPIC.to_string());
        }
        if relay_live {
            return Some(RelayLine {
                text: c::RELAY_HINT_RUNNING.to_string(),
                error: true,
            });
        }
        let sep = format!(" {} ", c::RELAY_ARROW);
        return hint(fill(c::RELAY_HINT_ORDER, &[("names", &names.join(&sep))]));
    }
    if !in_chat && ai_members_named(text, picked, members) >= 2 {
        return hint(c::RELAY_HINT_DISCOVER.to_string());
    }
    None
}

impl super::ComposerView {
    /// The 按顺序做 line above the field now: why the server refused my last order here,
    /// else what the draft's order will do (or a hint), none while editing.
    pub(super) fn relay_line_now(&self) -> Option<RelayLine> {
        if self.editing.is_some() {
            return None;
        }
        let key = self.key.as_ref()?;
        let p = &self.services.provider;
        let topic = key.topic_id.as_deref();
        if let Some(t) = topic
            && let Some(r) = p.relay_refusal(&key.group_id, t)
            && let Some(text) = refusal_text(&r, |id| {
                crate::copy::isolate(crate::ui::avatar::name_beside_avatar(
                    &crate::views::shell::member_name(&self.services, id),
                ))
            })
        {
            return Some(RelayLine { text, error: true });
        }
        if self.last_text.trim().is_empty() {
            return None;
        }
        let in_chat = topic.is_some_and(crate::data::is_main_topic);
        let relay_live = topic
            .and_then(|t| p.topic(&key.group_id, t))
            .and_then(|t| t.relay)
            .is_some_and(|r| matches!(r.state.as_str(), "running" | "paused"));
        let members = crate::views::topic_menu::group_members(&self.services, &key.group_id);
        relay_line(&self.last_text, &self.picked, &members, in_chat, relay_live)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{MemberKind, MemberLevel};

    fn member(id: &str, name: &str, bot: bool) -> Member {
        Member {
            id: id.into(),
            kind: if bot {
                MemberKind::Bot
            } else {
                MemberKind::Human
            },
            name: name.into(),
            role: None,
            level: MemberLevel::Member,
            avatar_url: None,
            deactivated: false,
            online: true,
            character: None,
            team_owner: false,
            deleted: false,
        }
    }

    fn team() -> Vec<Member> {
        vec![
            member("a", "小码", true),
            member("b", "小审", true),
            member("c", "Code Bot", true),
            member("me", "Joye", false),
        ]
    }

    #[test]
    fn an_order_on_the_first_line_names_its_steps() {
        let m = team();
        assert_eq!(
            order_names(
                "@小码 写 → @小审 审 → @小码 按意见改\n给设置页加深色模式",
                &m
            ),
            Some(vec!["小码".into(), "小审".into(), "小码".into()])
        );
        // Every separator the server reads, and a name with a space.
        assert_eq!(
            order_names("@Code Bot -> @小审 => @小码 收尾", &m),
            Some(vec!["Code Bot".into(), "小审".into(), "小码".into()])
        );
        // An unknown name is still a step (the server says who cannot take it).
        assert_eq!(
            order_names("@小码 → @路人 看看", &m),
            Some(vec!["小码".into(), "路人".into()])
        );
        // Not an order: one step, no @ first, a step without @, an arrow on line two.
        assert_eq!(order_names("@小码 写一下", &m), None);
        assert_eq!(order_names("先 @小码 → @小审", &m), None);
        assert_eq!(order_names("@小码 → 小审", &m), None);
        assert_eq!(order_names("@小码 写\n→ @小审", &m), None);
    }

    #[test]
    fn the_line_says_what_the_order_does_or_why_not() {
        let m = team();
        let order = "@小码 → @小审 → @小码 加深色模式";
        assert_eq!(
            relay_line(order, &[], &m, false, false),
            Some(RelayLine {
                text: "按顺序做：小码 → 小审 → 小码".into(),
                error: false
            })
        );
        assert_eq!(
            relay_line(order, &[], &m, true, false).map(|l| l.text),
            Some("按顺序做要在话题里用".into())
        );
        assert_eq!(
            relay_line(order, &[], &m, false, true),
            Some(RelayLine {
                text: "这个话题已经在按顺序做，先停止它".into(),
                error: true
            })
        );
    }

    #[test]
    fn two_ai_members_in_one_message_hint_at_the_order() {
        let m = team();
        assert_eq!(
            relay_line("@小码 和 @小审 一起看看", &[], &m, false, false).map(|l| l.text),
            Some("想让他们按顺序做？用 → 连起来".into())
        );
        // Picked from the @ list counts too; a person does not.
        assert!(relay_line("看看", &["a".into(), "b".into()], &m, false, false).is_some());
        assert!(relay_line("@小码 @Joye 看看", &[], &m, false, false).is_none());
        // Not in the 群聊.
        assert!(relay_line("@小码 和 @小审", &[], &m, true, false).is_none());
    }

    #[test]
    fn a_refusal_names_who_cannot_take_it() {
        let name = |id: &str| match id {
            "a" => "小码".to_string(),
            "b" => "小审".to_string(),
            _ => String::new(),
        };
        let r = |reason: &str, ids: &[&str]| RelayRefusal {
            reason: reason.into(),
            member_ids: ids.iter().map(|s| s.to_string()).collect(),
        };
        assert_eq!(
            refusal_text(&r("relay.member_unavailable", &["a", "b"]), name).as_deref(),
            Some("小码、小审 现在接不了这件事")
        );
        assert_eq!(
            refusal_text(&r("relay.running", &[]), name).as_deref(),
            Some("这个话题已经在按顺序做，先停止它")
        );
        assert!(refusal_text(&r("relay.bad_steps", &[]), name).is_some());
        assert!(refusal_text(&r("relay.something_new", &[]), name).is_none());
    }
}
