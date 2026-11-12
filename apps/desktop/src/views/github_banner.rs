//! GitHub in the main window (docs/plans/usable/design-github-identity.zh.md §3.7):
//!
//! - the banner at the top of a 话题 whose AI member waits for me to connect GitHub
//!   (`{成员}在等你连接 GitHub`, 连接 GitHub). Once I connect, the server resumes the run and
//!   the banner goes;
//! - the one-time 连接 GitHub prompt after sign-in (with 以后再说), once per account on this
//!   computer, only when the server can connect GitHub and I have not.
//!
//! The status is read when the window comes to the front, when another 话题 opens, when I
//! sign in, and on `github.status`. A server without GitHub shows nothing here.

use gpui::{
    App, AppContext as _, Context, Entity, Global, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Window, div,
};
use gpui_component::WindowExt as _;

use crate::{
    app::{Changes, Services},
    copy::{fill, isolate, settings as s},
    data::{Change, GithubState, GithubWait, SessionState},
    ui::{
        Sym,
        native::{Alert, AlertTone, PushSize, PushStyle, push_button, tokens},
    },
    views::{
        settings::{
            github::{Place, open_connect},
            prefs,
        },
        timeline::v2 as timeline_v2,
    },
};

/// The banner of the open main window.
struct Slot(Entity<GithubBanner>);

impl Global for Slot {}

pub struct GithubBanner {
    services: Services,
    /// The 话题 the status was last read for.
    read_for: Option<(String, String)>,
    _subscriptions: Vec<Subscription>,
}

/// Starts the banner and the prompt for the main window.
pub fn install(services: &Services, window: &mut Window, cx: &mut App) {
    let banner = cx.new(|cx| GithubBanner::new(services.clone(), window, cx));
    cx.set_global(Slot(banner));
}

/// The banner for the top of the 话题 page; it draws nothing while no member waits.
pub fn slot(cx: &App) -> Option<Entity<GithubBanner>> {
    cx.try_global::<Slot>().map(|s| s.0.clone())
}

/// `<server>#<member id>` of the signed-in account.
fn account_key(services: &Services) -> Option<String> {
    match services.provider.session() {
        SessionState::SignedIn { me, server_url } => Some(format!("{server_url}#{}", me.id)),
        _ => None,
    }
}

/// Whether the one-time prompt shows for `key`: the server can connect GitHub, I have not
/// connected, and this account has not seen it on this computer.
pub fn should_prompt(state: Option<&GithubState>, key: &str, prompted: &[String]) -> bool {
    state.is_some_and(|s| s.available && s.account.is_none()) && !prompted.iter().any(|k| k == key)
}

/// The wait the banner of a 话题 shows.
pub fn banner_wait<'a>(
    state: Option<&'a GithubState>,
    topic: Option<&(String, String)>,
) -> Option<&'a GithubWait> {
    let (g, t) = topic?;
    state?.wait_in(g, t)
}

impl GithubBanner {
    fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        services.provider.refresh_github();
        let subscriptions = vec![
            cx.subscribe_in(
                &services.hub,
                window,
                |this, _, changes: &Changes, window, cx| {
                    if changes.any(|c| matches!(c, Change::Session)) {
                        this.read_for = None;
                        this.services.provider.refresh_github();
                    }
                    if changes.any(|c| matches!(c, Change::Github | Change::Session)) {
                        this.maybe_prompt(window, cx);
                        cx.notify();
                    }
                },
            ),
            cx.observe(&services.nav, |_, _, cx| cx.notify()),
            // 应用回到前台时取.
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.services.provider.refresh_github();
                    this.maybe_prompt(window, cx);
                }
            }),
        ];
        Self {
            services,
            read_for: None,
            _subscriptions: subscriptions,
        }
    }

    /// Opens the one-time 连接 GitHub sheet when it is due. Self-tests never write prefs,
    /// so they never prompt by themselves (a scene opens it).
    fn maybe_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Unit tests and self-tests never prompt by themselves (nor write the prefs file).
        if cfg!(test)
            || !prefs::saving()
            || crate::selftest::clock::is_pinned()
            || window.has_active_dialog(cx)
        {
            return;
        }
        let Some(key) = account_key(&self.services) else {
            return;
        };
        let state = self.services.provider.github();
        if !should_prompt(state.as_ref(), &key, &prefs::get().github_prompted) {
            return;
        }
        prefs::update(|p| p.github_prompted.push(key));
        let services = self.services.clone();
        window.defer(cx, move |window, cx| {
            open_connect(services, Place::Main, true, window, cx);
        });
    }

    fn connect_button(&self, cx: &App) -> gpui_component::button::Button {
        let services = self.services.clone();
        push_button(
            "github-wait-connect",
            s::GITHUB_CONNECT_TITLE,
            PushStyle::Default,
            PushSize::Small,
            cx,
        )
        .on_click(move |_, window, cx| {
            open_connect(services.clone(), Place::Main, false, window, cx);
        })
    }
}

impl Render for GithubBanner {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let topic = self.services.nav.read(cx).topic.clone();
        // 打开话题时取: once per 话题 opened.
        if topic.is_some() && topic != self.read_for {
            self.read_for = topic.clone();
            self.services.provider.refresh_github();
        }
        let state = self.services.provider.github();
        let Some(wait) = banner_wait(state.as_ref(), topic.as_ref()) else {
            return div().into_any_element();
        };
        let name = isolate(crate::ui::avatar::name_beside_avatar(
            &crate::views::shell::member_name(&self.services, &wait.bot_id),
        ));
        let title = fill(
            if wait.reconnect {
                s::GITHUB_WAIT_RECONNECT
            } else {
                s::GITHUB_WAIT
            },
            &[("name", &name)],
        );
        // COMPLETENESS §2.16: an in-page banner is the Alert at the top of the message
        // column, on the same insets as the composer.
        div()
            .flex_none()
            .w_full()
            .pl(timeline_v2::LEADING)
            .pr(timeline_v2::SIDE)
            .pt(tokens::space::S8)
            .child(
                Alert::new(Sym::Branch, title)
                    .tone(AlertTone::Warning)
                    .description(fill(s::GITHUB_WAIT_HINT, &[("name", &name)]))
                    .action(self.connect_button(cx)),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::GithubAccount;

    fn wait(topic: &str, reconnect: bool) -> GithubWait {
        GithubWait {
            run_id: format!("r-{topic}"),
            group_id: "g".into(),
            topic_id: topic.into(),
            bot_id: "b".into(),
            reconnect,
        }
    }

    fn state(available: bool, connected: bool, waits: Vec<GithubWait>) -> GithubState {
        GithubState {
            available,
            unavailable_reason: None,
            account: connected.then(|| GithubAccount {
                login: "octo".into(),
                name: None,
                ok: true,
                connected_at: 1,
            }),
            waits,
        }
    }

    #[test]
    fn the_prompt_shows_once_per_account_and_only_when_it_can_help() {
        let key = "https://chat.example.com#m1";
        assert!(should_prompt(Some(&state(true, false, vec![])), key, &[]));
        // Seen on this computer.
        assert!(!should_prompt(
            Some(&state(true, false, vec![])),
            key,
            &[key.to_string()]
        ));
        // Another account on the same computer still sees it.
        assert!(should_prompt(
            Some(&state(true, false, vec![])),
            key,
            &["https://chat.example.com#m2".to_string()]
        ));
        // Connected already, a server without a client id, or not read yet.
        assert!(!should_prompt(Some(&state(true, true, vec![])), key, &[]));
        assert!(!should_prompt(Some(&state(false, false, vec![])), key, &[]));
        assert!(!should_prompt(None, key, &[]));
    }

    #[test]
    fn the_banner_shows_in_the_waiting_topic_only() {
        let st = state(true, false, vec![wait("t1", false), wait("t2", true)]);
        let topic = |t: &str| ("g".to_string(), t.to_string());
        assert_eq!(
            banner_wait(Some(&st), Some(&topic("t1"))).map(|w| w.reconnect),
            Some(false)
        );
        assert_eq!(
            banner_wait(Some(&st), Some(&topic("t2"))).map(|w| w.reconnect),
            Some(true)
        );
        assert!(banner_wait(Some(&st), Some(&topic("t3"))).is_none());
        assert!(banner_wait(Some(&st), None).is_none());
        assert!(banner_wait(None, Some(&topic("t1"))).is_none());
    }
}
