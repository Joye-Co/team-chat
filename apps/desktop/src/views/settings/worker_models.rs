//! 通用 › 模型 (owners only): one place for the models every AI member uses. The front desk's
//! model is the AI 服务 (shown here, changed in its own section above); the executors' model and
//! 思考强度 are chosen per CLI runtime from what the team's computers report, and a pick is saved
//! at once. A member's own runner model wins over these. Loading, failed and empty states per
//! COMPLETENESS.md §2.15.

use gpui::{Context, IntoElement, ParentElement, SharedString, Styled, Subscription, Window, div};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};

use super::{
    ai_service::provider_label,
    members::{cli_choices, default_model_text, effort_options, effort_text, model_text},
    widgets::{self, Status, answer},
};
use crate::{
    app::Services,
    copy::{fill, settings as s},
    data::{AiServiceSettings, MachineInfo, WorkerModelSetting},
    ui::native::{
        FormRow, GroupedForm, SectionTitle, disabled_push,
        tokens::{NativeType as _, size, space},
    },
};

#[derive(Clone, Debug, Default, PartialEq)]
pub enum ModelsAction {
    #[default]
    Idle,
    Saving,
    Saved,
    SaveFailed(SharedString),
}

#[derive(Clone)]
struct Loaded {
    workers: Vec<WorkerModelSetting>,
    machines: Vec<MachineInfo>,
    front: Option<AiServiceSettings>,
}

pub struct WorkerModelsPanel {
    services: Services,
    owner: bool,
    loaded: Option<Loaded>,
    load: Status,
    action: ModelsAction,
    _subscriptions: Vec<Subscription>,
}

/// The runtime names the computers report, `claude` and `codex` first.
/// The 前台 row of 模型 names only the provider and model; what the front desk does,
/// and what is off without an AI service, is said once, in the AI 服务 list above.
fn front_value(front: Option<&AiServiceSettings>) -> String {
    match front.and_then(|f| f.provider.map(|p| (p, f.model.clone()))) {
        Some((p, model)) => fill(
            s::MODELS_FRONT_VALUE,
            &[("provider", provider_label(p)), ("name", &model)],
        ),
        None => s::MODELS_FRONT_UNSET.into(),
    }
}

fn runtimes(machines: &[MachineInfo]) -> Vec<String> {
    let mut all: Vec<String> = machines
        .iter()
        .flat_map(|m| m.clis.iter().map(|c| c.runtime.clone()))
        .collect();
    all.sort_by_key(|r| {
        (
            !matches!(r.as_str(), "claude"),
            !matches!(r.as_str(), "codex"),
            r.clone(),
        )
    });
    all.dedup();
    all
}

impl WorkerModelsPanel {
    pub fn new(services: Services, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sub = cx.subscribe(
            &services.hub,
            |this: &mut Self, _, changes: &crate::app::Changes, cx| {
                if changes.any(|c| {
                    matches!(
                        c,
                        crate::data::Change::Session | crate::data::Change::Members
                    )
                }) {
                    let owner = super::i_am_owner(&this.services);
                    if owner != this.owner {
                        this.owner = owner;
                        this.loaded = None;
                        this.load = Status::Idle;
                        cx.notify();
                    }
                }
            },
        );
        Self {
            owner: super::i_am_owner(&services),
            services,
            loaded: None,
            load: Status::Idle,
            action: ModelsAction::Idle,
            _subscriptions: vec![sub],
        }
    }

    pub fn action(&self) -> &ModelsAction {
        &self.action
    }

    pub fn workers(&self) -> Vec<WorkerModelSetting> {
        self.loaded
            .as_ref()
            .map(|l| l.workers.clone())
            .unwrap_or_default()
    }

    /// Reads the team's choices, the computers' models and the AI 服务 (for the 前台 row).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.load.busy() || !self.owner {
            return;
        }
        self.load = Status::Busy(s::MODELS_LOADING.into());
        let workers = self.services.provider.worker_models();
        let machines = self.services.provider.machines();
        let front = self.services.provider.ai_service();
        cx.spawn(async move |this, cx| {
            let workers = answer(workers).await;
            let machines = answer(machines).await;
            let front = answer(front).await.ok();
            let _ = this.update(cx, |this, cx| {
                match (workers, machines) {
                    (Ok(workers), Ok(machines)) => {
                        this.loaded = Some(Loaded {
                            workers,
                            machines,
                            front,
                        });
                        this.load = Status::Idle;
                    }
                    (Err(e), _) | (_, Err(e)) => this.load = Status::from_error(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Saves one runtime's model and effort (both fields: `None` clears).
    pub fn choose(&mut self, setting: WorkerModelSetting, cx: &mut Context<Self>) {
        if self.action == ModelsAction::Saving {
            return;
        }
        let Some(loaded) = self.loaded.as_mut() else {
            return;
        };
        // Shown at once; a failure puts the old value back.
        let before = loaded.workers.clone();
        loaded.workers.retain(|w| w.runtime != setting.runtime);
        loaded.workers.push(setting.clone());
        self.action = ModelsAction::Saving;
        cx.notify();
        let reply = self.services.provider.set_worker_model(setting);
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(saved) => {
                        if let Some(l) = this.loaded.as_mut() {
                            l.workers.retain(|w| w.runtime != saved.runtime);
                            l.workers.push(saved);
                        }
                        this.action = ModelsAction::Saved;
                    }
                    Err(e) => {
                        if let Some(l) = this.loaded.as_mut() {
                            l.workers = before;
                        }
                        this.action = ModelsAction::SaveFailed(widgets::error_text(e).into());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn front_row(&self, front: Option<&AiServiceSettings>, cx: &mut Context<Self>) -> FormRow {
        let k = crate::ui::native::native_colors(cx);
        let value = front_value(front);
        FormRow::new(s::MODELS_FRONT).control(
            div()
                .max_w(size::SETTINGS_FIELD_WIDTH)
                .text_right()
                .n_subheadline()
                .text_color(k.secondary)
                .child(value),
        )
    }

    fn runtime_row(
        &self,
        runtime: &str,
        l: &Loaded,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> FormRow {
        let busy = self.action == ModelsAction::Saving;
        let (models, efforts) = cli_choices(&l.machines, runtime, None);
        let current = l
            .workers
            .iter()
            .find(|w| w.runtime == runtime)
            .cloned()
            .unwrap_or(WorkerModelSetting {
                runtime: runtime.into(),
                ..Default::default()
            });
        let panel = cx.entity();

        let mut options: Vec<(Option<String>, String)> = vec![(None, default_model_text(&models))];
        if let Some(p) = current
            .model
            .as_ref()
            .filter(|p| !models.iter().any(|m| &m.id == *p))
        {
            options.push((Some(p.clone()), p.clone()));
        }
        options.extend(models.iter().map(|m| (Some(m.id.clone()), model_text(m))));
        let shown = options
            .iter()
            .find(|(id, _)| *id == current.model)
            .map(|(_, t)| t.clone())
            .unwrap_or_default();
        let model = {
            let (panel, current) = (panel.clone(), current.clone());
            disabled_push(
                widgets::popup_fixed(
                    SharedString::from(format!("models-{runtime}-model")),
                    shown,
                    size::MODEL_SELECT_WIDTH,
                    window,
                    cx,
                ),
                busy,
                cx,
            )
            .dropdown_menu(move |menu, _, _| {
                options.iter().fold(menu, |menu, (id, text)| {
                    let (panel, mut next) = (panel.clone(), current.clone());
                    next.model = id.clone();
                    menu.item(
                        PopupMenuItem::new(text.clone())
                            .checked(*id == current.model)
                            .on_click(move |_, _, cx| {
                                panel.update(cx, |this, cx| this.choose(next.clone(), cx))
                            }),
                    )
                })
            })
        };

        let levels = effort_options(&models, &efforts, current.model.as_deref());
        let effort = (!levels.is_empty() || current.effort.is_some()).then(|| {
            let mut options: Vec<(Option<String>, String)> =
                vec![(None, s::MODELS_EFFORT_DEFAULT.to_string())];
            if let Some(p) = current.effort.as_ref().filter(|p| !levels.contains(p)) {
                options.push((Some(p.clone()), effort_text(p)));
            }
            options.extend(levels.iter().map(|e| (Some(e.clone()), effort_text(e))));
            let shown = options
                .iter()
                .find(|(id, _)| *id == current.effort)
                .map(|(_, t)| t.clone())
                .unwrap_or_default();
            let (panel, current) = (panel.clone(), current.clone());
            disabled_push(
                widgets::popup_fixed(
                    SharedString::from(format!("models-{runtime}-effort")),
                    shown,
                    size::EFFORT_SELECT_WIDTH,
                    window,
                    cx,
                ),
                busy,
                cx,
            )
            .dropdown_menu(move |menu, _, _| {
                options.iter().fold(menu, |menu, (id, text)| {
                    let (panel, mut next) = (panel.clone(), current.clone());
                    next.effort = id.clone();
                    menu.item(
                        PopupMenuItem::new(text.clone())
                            .checked(*id == current.effort)
                            .on_click(move |_, _, cx| {
                                panel.update(cx, |this, cx| this.choose(next.clone(), cx))
                            }),
                    )
                })
            })
        });
        FormRow::new(runtime.to_string())
            .detail(s::MODELS_WORKER_DETAIL)
            .min_height(crate::ui::native::tokens::height::FORM_ROW_TWO)
            .control(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(model)
                    .children(effort),
            )
    }

    pub fn section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<gpui::Div> {
        if !self.owner {
            return None;
        }
        if self.loaded.is_none() && matches!(self.load, Status::Idle) {
            self.refresh(cx);
        }
        let header = SectionTitle::new(s::SECTION_MODELS).description(s::SECTION_MODELS_HINT);
        let Some(l) = self.loaded.clone() else {
            let body = match &self.load {
                Status::Failed(t) => widgets::failed_alert(
                    "models-load-failed",
                    s::MODELS_LOAD_FAILED,
                    Some(t.clone()),
                    Some(
                        widgets::retry_button("models-retry", cx)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    ),
                    cx,
                ),
                _ => GroupedForm::new()
                    .row(widgets::list_loading("models-loading", 3, false, cx))
                    .into_any_element(),
            };
            return Some(widgets::section(header, body));
        };
        let mut group = GroupedForm::new().row(self.front_row(l.front.as_ref(), cx));
        let names = runtimes(&l.machines);
        for rt in &names {
            group = group.row(self.runtime_row(rt, &l, window, cx));
        }
        let empty = names
            .is_empty()
            .then(|| widgets::quiet(s::MODELS_NO_MACHINE, cx));
        let answer_row: Option<gpui::AnyElement> = match &self.action {
            ModelsAction::Idle => None,
            ModelsAction::Saving => Some(widgets::quiet(s::MODELS_SAVING, cx).into_any_element()),
            ModelsAction::Saved => Some(widgets::quiet(s::MODELS_SAVED, cx).into_any_element()),
            ModelsAction::SaveFailed(t) => Some(widgets::failed_alert(
                "models-save-failed",
                s::MODELS_SAVE_FAILED,
                Some(t.clone()),
                None,
                cx,
            )),
        };
        Some(widgets::section(
            header,
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(space::S12)
                .child(group)
                .children(empty)
                .children(answer_row),
        ))
    }
}

impl gpui::Render for WorkerModelsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.section(window, cx).unwrap_or_else(div)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::CliChoice;

    #[test]
    fn claude_code_and_codex_come_first() {
        let m = |rts: &[&str]| MachineInfo {
            clis: rts
                .iter()
                .map(|r| CliChoice {
                    runtime: (*r).into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        assert_eq!(
            runtimes(&[m(&["acp", "codex"]), m(&["claude", "codex"])]),
            vec!["claude", "codex", "acp"]
        );
    }

    /// The 前台 row repeats none of the AI 服务 list's words: it names the provider and
    /// model, or only that the AI service is not set up.
    #[test]
    fn the_front_row_names_only_the_model() {
        assert_eq!(front_value(None), s::MODELS_FRONT_UNSET);
        assert_ne!(s::MODELS_FRONT_UNSET, s::AI_USE_FRONT_OFF);
        assert!(!s::MODELS_FRONT_UNSET.contains(s::AI_USE_FRONT_OFF));
        let unset = AiServiceSettings::default();
        assert_eq!(front_value(Some(&unset)), s::MODELS_FRONT_UNSET);
    }
}
