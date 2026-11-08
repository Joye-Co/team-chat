//! 通用 › AI 服务 (owners only): the provider, model and API Key the server
//! uses for its own model calls (AI 话题标题). The key goes to the server
//! once and never comes back: the field shows "已设置 ····abcd" after it is
//! saved. 测试连接 tries the values in the form without saving them; 保存
//! stores them. Loading, failed and empty states per COMPLETENESS.md §2.15.

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, SharedString, Styled,
    Subscription, Window, div,
};
use gpui_component::{
    input::{InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
};

use super::widgets::{self, Status, answer};
use crate::{
    app::Services,
    copy::{fill, settings as s},
    data::{
        AiDraftProblem, AiProviderKind, AiServiceDraft, AiServiceSettings, AiTestOutcome, Change,
    },
    ui::native::{
        FormRow, GroupedForm, PushSize, PushStyle, SectionTitle, disabled_push, push_button,
        tokens::{size, space},
    },
};

/// What the last 测试连接 or 保存 did.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum AiAction {
    #[default]
    Idle,
    Testing,
    Saving,
    TestOk(u64),
    /// The provider's error text.
    TestFailed(SharedString),
    Saved,
    /// A field that is not right yet, or the server's refusal.
    Problem(SharedString),
    SaveFailed(SharedString),
}

impl AiAction {
    fn busy(&self) -> bool {
        matches!(self, AiAction::Testing | AiAction::Saving)
    }
}

pub fn provider_label(p: AiProviderKind) -> &'static str {
    match p {
        AiProviderKind::Deepseek => s::AI_PROVIDER_DEEPSEEK,
        AiProviderKind::Anthropic => s::AI_PROVIDER_ANTHROPIC,
        AiProviderKind::OpenaiCompatible => s::AI_PROVIDER_COMPATIBLE,
    }
}

fn problem_text(p: AiDraftProblem, switched: bool) -> &'static str {
    match p {
        AiDraftProblem::ModelRequired => s::AI_MODEL_REQUIRED,
        AiDraftProblem::ModelInvalid => s::AI_MODEL_INVALID,
        AiDraftProblem::BaseUrlInvalid => s::AI_BASE_URL_INVALID,
        AiDraftProblem::KeyRequired if switched => s::AI_KEY_REQUIRED_SWITCH,
        AiDraftProblem::KeyRequired => s::AI_KEY_REQUIRED,
        AiDraftProblem::KeyInvalid => s::AI_KEY_INVALID,
    }
}

pub struct AiServicePanel {
    services: Services,
    /// Whether I am an owner, read when the session or the members change
    /// rather than on every frame (`session()` reads the store).
    owner: bool,
    /// What the server keeps; `None` until it is read.
    saved: Option<AiServiceSettings>,
    load: Status,
    /// The provider picked in the form.
    provider: Option<AiProviderKind>,
    model: Entity<InputState>,
    base_url: Entity<InputState>,
    key: Entity<InputState>,
    action: AiAction,
    /// Which field the last problem is about (its error ring).
    problem: Option<AiDraftProblem>,
    _subscriptions: Vec<Subscription>,
}

impl AiServicePanel {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let model = cx.new(|cx| InputState::new(window, cx).placeholder(s::AI_MODEL_PLACEHOLDER));
        let base_url =
            cx.new(|cx| InputState::new(window, cx).placeholder(s::AI_BASE_URL_PLACEHOLDER));
        let key = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(s::AI_KEY_PLACEHOLDER)
        });
        let mut subscriptions: Vec<Subscription> = [&model, &base_url, &key]
            .into_iter()
            .map(|field| {
                cx.subscribe(field, |this: &mut Self, _, e: &InputEvent, cx| {
                    // Typing clears the answer of the last try.
                    if matches!(e, InputEvent::Change) && !this.action.busy() {
                        this.action = AiAction::Idle;
                        this.problem = None;
                        cx.notify();
                    }
                })
            })
            .collect();
        subscriptions.push(cx.subscribe_in(
            &services.hub,
            window,
            |this, _, changes: &crate::app::Changes, window, cx| {
                if changes.any(|c| matches!(c, Change::Session)) {
                    this.owner = super::i_am_owner(&this.services);
                    this.saved = None;
                    this.load = Status::Idle;
                    this.refresh(window, cx);
                } else if changes.any(|c| matches!(c, Change::Members)) {
                    let owner = super::i_am_owner(&this.services);
                    if owner != this.owner {
                        this.owner = owner;
                        cx.notify();
                    }
                }
            },
        ));
        Self {
            owner: super::i_am_owner(&services),
            services,
            saved: None,
            load: Status::Idle,
            provider: None,
            model,
            base_url,
            key,
            action: AiAction::Idle,
            problem: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn saved(&self) -> Option<&AiServiceSettings> {
        self.saved.as_ref()
    }

    pub fn action(&self) -> &AiAction {
        &self.action
    }

    /// Reads the team's AI 服务 (once per window, and after a failure on 重试).
    pub fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.load.busy() || !self.owner {
            return;
        }
        self.load = Status::Busy(s::AI_LOADING.into());
        let reply = self.services.provider.ai_service();
        cx.spawn_in(window, async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(saved) => {
                        this.saved = Some(saved.clone());
                        this.show(&saved, window, cx);
                        this.load = Status::Idle;
                    }
                    Err(e) => this.load = Status::from_error(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Puts the saved values in the form (call after `saved` is set); the key
    /// field stays empty.
    fn show(&mut self, saved: &AiServiceSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.provider = saved.provider;
        let model = saved.model.clone();
        let base = saved.base_url.clone().unwrap_or_default();
        self.model
            .update(cx, |st, cx| st.set_value(model, window, cx));
        self.base_url
            .update(cx, |st, cx| st.set_value(base, window, cx));
        self.key
            .update(cx, |st, cx| st.set_value(String::new(), window, cx));
        self.update_key_placeholder(window, cx);
    }

    /// "已设置 ····abcd" while the saved key still applies, else 粘贴 API Key.
    fn update_key_placeholder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text: SharedString = match self.kept_hint() {
            Some(hint) => fill(s::AI_KEY_SET, &[("hint", &hint)]).into(),
            None => s::AI_KEY_PLACEHOLDER.into(),
        };
        self.key
            .update(cx, |st, cx| st.set_placeholder(text, window, cx));
    }

    /// The saved key's hint when the picked provider is the saved one.
    fn kept_hint(&self) -> Option<String> {
        let saved = self.saved.as_ref()?;
        (saved.provider.is_some() && saved.provider == self.provider)
            .then(|| saved.key_hint.clone())
            .flatten()
    }

    /// Self-tests: types into the model, 接口地址 and key fields.
    pub fn fill_for_test(
        &mut self,
        model: &str,
        base_url: &str,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (state, text) in [
            (&self.model, model),
            (&self.base_url, base_url),
            (&self.key, key),
        ] {
            let text = text.to_string();
            state.update(cx, |st, cx| st.set_value(text, window, cx));
        }
    }

    pub fn pick(&mut self, p: AiProviderKind, window: &mut Window, cx: &mut Context<Self>) {
        if self.provider == Some(p) {
            return;
        }
        // The suggested model of the new provider, unless it is the saved one.
        let model = match &self.saved {
            Some(saved) if saved.provider == Some(p) => saved.model.clone(),
            _ => p.default_model().to_string(),
        };
        self.provider = Some(p);
        self.model
            .update(cx, |st, cx| st.set_value(model, window, cx));
        self.action = AiAction::Idle;
        self.problem = None;
        self.update_key_placeholder(window, cx);
        cx.notify();
    }

    /// The form as a draft, or the first problem in it.
    fn draft(&self, cx: &Context<Self>) -> Result<AiServiceDraft, (AiDraftProblem, bool)> {
        let provider = self.provider.unwrap_or(AiProviderKind::Deepseek);
        let key = self.key.read(cx).value().trim().to_string();
        let base = self.base_url.read(cx).value().trim().to_string();
        let draft = AiServiceDraft {
            provider,
            model: self.model.read(cx).value().trim().to_string(),
            base_url: (!base.is_empty()).then_some(base),
            api_key: (!key.is_empty()).then_some(key),
        };
        let saved = self.saved.clone().unwrap_or_default();
        let switched = saved.key_hint.is_some() && saved.provider != Some(provider);
        draft
            .check(&saved)
            .map(|()| draft)
            .map_err(|p| (p, switched))
    }

    fn checked(&mut self, cx: &mut Context<Self>) -> Option<AiServiceDraft> {
        match self.draft(cx) {
            Ok(d) => Some(d),
            Err((p, switched)) => {
                self.problem = Some(p);
                self.action = AiAction::Problem(problem_text(p, switched).into());
                cx.notify();
                None
            }
        }
    }

    pub fn test(&mut self, cx: &mut Context<Self>) {
        if self.action.busy() || self.provider.is_none() {
            return;
        }
        let Some(draft) = self.checked(cx) else {
            return;
        };
        self.action = AiAction::Testing;
        cx.notify();
        let reply = self.services.provider.test_ai_service(draft);
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.action = match result {
                    Ok(AiTestOutcome::Ok { latency_ms }) => AiAction::TestOk(latency_ms),
                    Ok(AiTestOutcome::Failed(e)) => AiAction::TestFailed(e.into()),
                    Err(e) => AiAction::TestFailed(widgets::error_text(e).into()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.action.busy() || self.provider.is_none() {
            return;
        }
        let Some(draft) = self.checked(cx) else {
            return;
        };
        self.action = AiAction::Saving;
        cx.notify();
        let reply = self.services.provider.set_ai_service(draft);
        cx.spawn_in(window, async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(saved) => {
                        this.saved = Some(saved.clone());
                        this.show(&saved, window, cx);
                        this.action = AiAction::Saved;
                    }
                    Err(e) => this.action = AiAction::SaveFailed(widgets::error_text(e).into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The line or Alert under the group: the answer of the last try.
    fn answer_row(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let quiet = |t: SharedString| Some(widgets::quiet(t, cx).into_any_element());
        match &self.action {
            AiAction::Idle => None,
            AiAction::Testing => quiet(s::AI_TESTING.into()),
            AiAction::Saving => quiet(s::AI_SAVING.into()),
            AiAction::TestOk(ms) => quiet(fill(s::AI_TEST_OK, &[("ms", &ms.to_string())]).into()),
            AiAction::Saved => quiet(s::AI_SAVED.into()),
            AiAction::Problem(t) => {
                Some(crate::ui::native::sheet_status_error(t.clone(), cx).into_any_element())
            }
            AiAction::TestFailed(t) => Some(widgets::failed_alert(
                "ai-test-failed",
                s::AI_TEST_FAILED,
                Some(t.clone()),
                None,
                cx,
            )),
            AiAction::SaveFailed(t) => Some(widgets::failed_alert(
                "ai-save-failed",
                s::AI_SAVE_FAILED,
                Some(t.clone()),
                None,
                cx,
            )),
        }
    }

    /// The section; `None` for a member who is not an owner.
    pub fn section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<gpui::Div> {
        if !self.owner {
            return None;
        }
        if self.saved.is_none() && matches!(self.load, Status::Idle) {
            self.refresh(window, cx);
        }
        let header = SectionTitle::new(s::SECTION_AI).description(s::SECTION_AI_HINT);
        let Some(saved) = self.saved.clone() else {
            let body = match &self.load {
                Status::Failed(t) => widgets::failed_alert(
                    "ai-load-failed",
                    s::AI_LOAD_FAILED,
                    Some(t.clone()),
                    Some(
                        widgets::retry_button("ai-retry", cx)
                            .on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx))),
                    ),
                    cx,
                ),
                _ => GroupedForm::new()
                    .row(widgets::list_loading("ai-loading", 3, false, cx))
                    .into_any_element(),
            };
            return Some(widgets::section(header, body));
        };
        let busy = self.action.busy();
        let panel = cx.entity();
        let current = self.provider;
        let select = disabled_push(
            widgets::popup_fixed(
                "ai-provider",
                current.map_or(s::AI_PROVIDER_PICK, provider_label),
                size::SELECT_WIDTH,
                window,
                cx,
            ),
            busy,
            cx,
        )
        .dropdown_menu(move |menu, _, _| {
            AiProviderKind::ALL.into_iter().fold(menu, |menu, p| {
                let panel = panel.clone();
                menu.item(
                    PopupMenuItem::new(provider_label(p))
                        .checked(current == Some(p))
                        .on_click(move |_, window, cx| {
                            panel.update(cx, |this, cx| this.pick(p, window, cx))
                        }),
                )
            })
        });
        let bad = |p: AiDraftProblem| self.problem == Some(p);
        let field = |id: &'static str, state: &Entity<InputState>, error: bool| {
            crate::ui::native::NativeTextField::new(id, state)
                .width(size::SETTINGS_FIELD_WIDTH)
                .disabled(busy)
                .error(error)
        };
        let mut group = GroupedForm::new().row(FormRow::new(s::AI_PROVIDER).control(select));
        if let Some(p) = current {
            group = group.row(FormRow::new(s::AI_MODEL).control(field(
                "ai-model",
                &self.model,
                bad(AiDraftProblem::ModelRequired) || bad(AiDraftProblem::ModelInvalid),
            )));
            if p.needs_base_url() {
                group = group.row(FormRow::new(s::AI_BASE_URL).control(field(
                    "ai-base-url",
                    &self.base_url,
                    bad(AiDraftProblem::BaseUrlInvalid),
                )));
            }
            let key_detail = if self.kept_hint().is_some() {
                s::AI_KEY_KEEP_HINT
            } else {
                s::AI_KEY_NEW_HINT
            };
            group = group.row(
                FormRow::new(s::AI_KEY)
                    .detail(key_detail)
                    .min_height(crate::ui::native::tokens::height::FORM_ROW_TWO)
                    .control(field(
                        "ai-key",
                        &self.key,
                        bad(AiDraftProblem::KeyRequired) || bad(AiDraftProblem::KeyInvalid),
                    )),
            );
        }
        let testing = self.action == AiAction::Testing;
        let saving = self.action == AiAction::Saving;
        let no_provider = current.is_none();
        let buttons = div()
            .w_full()
            .flex()
            .justify_end()
            .gap(space::S8)
            .child(
                disabled_push(
                    push_button(
                        "ai-test",
                        s::AI_TEST,
                        PushStyle::Outline,
                        PushSize::Regular,
                        cx,
                    ),
                    no_provider || busy,
                    cx,
                )
                .loading(testing)
                .on_click(cx.listener(|this, _, _, cx| this.test(cx))),
            )
            .child(
                disabled_push(
                    push_button(
                        "ai-save",
                        s::AI_SAVE,
                        PushStyle::Default,
                        PushSize::Regular,
                        cx,
                    ),
                    no_provider || busy,
                    cx,
                )
                .loading(saving)
                .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            );
        let empty = (saved.provider.is_none() && self.action == AiAction::Idle)
            .then(|| widgets::quiet(s::AI_EMPTY, cx));
        Some(widgets::section(
            header,
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(space::S12)
                .child(group)
                .children(empty)
                .children(self.answer_row(cx))
                .child(buttons),
        ))
    }
}

impl gpui::Render for AiServicePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.section(window, cx).unwrap_or_else(div)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_problem_has_words() {
        for p in [
            AiDraftProblem::ModelRequired,
            AiDraftProblem::ModelInvalid,
            AiDraftProblem::BaseUrlInvalid,
            AiDraftProblem::KeyRequired,
            AiDraftProblem::KeyInvalid,
        ] {
            assert!(!problem_text(p, false).is_empty());
        }
        assert_eq!(
            problem_text(AiDraftProblem::KeyRequired, true),
            s::AI_KEY_REQUIRED_SWITCH
        );
        for p in AiProviderKind::ALL {
            assert!(!provider_label(p).is_empty());
        }
    }
}
