//! Run plans (docs/protocol.md, "Run plans"): a bot starts a run with a plan, a member sees it
//! in its cache, an update replaces the whole plan, an empty plan clears it, only the bot may
//! set it, and a member who starts later catches up to the final plan of the ended run.

use super::{Ctx, Run, Scenario};
use crate::check;
use anyhow::{Context as _, Result};
use chat_core::ApiError;
use chat_core::proto::{
    EndRunRequest, EndRunStatus, RunPlanItem, RunPlanState, RunStatus, SetRunPlanRequest,
    StartRunRequest,
};
use std::time::{Duration, Instant};

pub const SCENARIO: Scenario = Scenario {
    name: "plan",
    about: "a bot sets a run plan; a member sees it; an update replaces it; a late member catches up",
    in_all: true,
    timeout_secs: 90,
    run: start,
};

fn start(c: &mut Ctx) -> Run<'_> {
    Box::pin(run(c))
}

fn item(text: &str, state: RunPlanState) -> RunPlanItem {
    RunPlanItem {
        text: text.into(),
        state,
    }
}

const WAIT: Duration = Duration::from_secs(20);

async fn run(c: &mut Ctx) -> Result<()> {
    use RunPlanState::*;
    let h = c.h.clone();
    let a = h.human("plan-a").await?;
    let b = h.human("plan-b").await?;
    let k = h.bot("plan-bot").await?;
    let g = h.group(&a, "plan", &[&b, &k]).await?;
    let ca = h.client(&a, "a", None)?;
    ca.ready(&[&g]).await?;
    ca.c.start_topic(&g, Some("周四小红书"), "@bot 写一条小红书", vec![k.id.clone()])?;
    let topic = ca
        .wait(WAIT, "the topic", |cl| {
            cl.topics(&g, 10)
                .ok()?
                .into_iter()
                .next()
                .filter(|t| t.topic.last_seq > 0)
                .map(|t| t.topic.id)
        })
        .await?;
    let bot = h.http(&k);

    let plan1 = vec![
        item("写标题和正文", Current),
        item("生成 3 张配图", Pending),
        item("周四 20:00 发出", Pending),
    ];
    let plan2 = vec![
        item("写标题和正文", Done),
        item("生成 3 张配图", Done),
        item("等 Joye 确认", Waiting),
        item("周四 20:00 发出", Pending),
    ];
    let seen = |want: Option<Vec<RunPlanItem>>, what: &'static str| {
        let topic = topic.clone();
        let ca = &ca;
        async move {
            let t0 = Instant::now();
            ca.wait(WAIT, what, |cl| {
                let run = cl.runs(&topic).ok()?.into_iter().next()?;
                (run.plan == want).then_some(run)
            })
            .await
            .map(|run| (run, t0.elapsed().as_secs_f64() * 1000.0))
        }
    };

    // The bot starts a run with a plan; the member sees it.
    let started = bot
        .start_run(&StartRunRequest {
            group_id: g.clone(),
            topic_id: topic.clone(),
            plan: Some(plan1.clone()),
        })
        .await
        .context("start run")?;
    check!(started.status == RunStatus::Running, "new run is {:?}", started.status);
    let (run, ms1) = seen(Some(plan1.clone()), "the first plan").await?;
    check!(run.id == started.id, "member sees run {} not {}", run.id, started.id);
    check!(run.bot_id == k.id, "the run's bot is {}", run.bot_id);

    // Starting again returns the same run.
    let again = bot
        .start_run(&StartRunRequest {
            group_id: g.clone(),
            topic_id: topic.clone(),
            plan: None,
        })
        .await?;
    check!(again.id == started.id, "a second start made run {}", again.id);

    // An update replaces the whole plan.
    let t = Instant::now();
    bot.set_run_plan(&started.id, &SetRunPlanRequest { plan: plan2.clone() })
        .await
        .context("set plan")?;
    let (_, _) = seen(Some(plan2.clone()), "the replaced plan").await?;
    let ms2 = t.elapsed().as_secs_f64() * 1000.0;

    // Only the run's bot sets its plan.
    match h
        .http(&a)
        .set_run_plan(&started.id, &SetRunPlanRequest { plan: plan1.clone() })
        .await
    {
        Err(ApiError::Server { status: 403, .. }) => {}
        other => anyhow::bail!("a member set the bot's plan: {other:?}"),
    }
    // Invalid plans never leave the client; the raw API refuses them too.
    let (status, _) = h
        .api(
            reqwest::Method::PUT,
            &format!("/v1/runs/{}/plan", started.id),
            Some(k.token()),
            Some(serde_json::json!({ "plan": [{ "text": "字".repeat(201), "state": "current" }] })),
        )
        .await?;
    check!(status == 400, "a 201-character item got {status}");

    // An empty plan clears it; then the bot sets it again and ends the run.
    bot.set_run_plan(&started.id, &SetRunPlanRequest { plan: vec![] })
        .await?;
    seen(None, "the cleared plan").await?;
    bot.set_run_plan(&started.id, &SetRunPlanRequest { plan: plan2.clone() })
        .await?;
    seen(Some(plan2.clone()), "the plan again").await?;
    let ended = bot
        .end_run(
            &started.id,
            &EndRunRequest {
                status: EndRunStatus::Done,
                summary: Some("已排期".into()),
            },
        )
        .await?;
    check!(ended.plan.as_ref() == Some(&plan2), "the ended run lost its plan");
    ca.wait(WAIT, "the ended run", |cl| {
        let run = cl.runs(&topic).ok()?.into_iter().next()?;
        (run.status == RunStatus::Done).then_some(())
    })
    .await?;
    match bot
        .set_run_plan(&started.id, &SetRunPlanRequest { plan: plan1.clone() })
        .await
    {
        Err(ApiError::Server { status: 409, .. }) => {}
        other => anyhow::bail!("a plan update after the end: {other:?}"),
    }

    // A member who starts now catches up to the final plan from the group log.
    let cb = h.client(&b, "b", None)?;
    cb.ready(&[&g]).await?;
    cb.wait(WAIT, "B: the final run and plan", |cl| {
        let run = cl.runs(&topic).ok()?.into_iter().next()?;
        (run.status == RunStatus::Done && run.plan.as_ref() == Some(&plan2)).then_some(())
    })
    .await?;

    c.metric("first_plan_seen_ms", ms1);
    c.metric("update_seen_ms", ms2);
    c.note(format!(
        "plan of {} then {} items seen by the member; cleared, set again, ended; late member caught up",
        plan1.len(),
        plan2.len()
    ));
    ca.shutdown().await;
    cb.shutdown().await;
    Ok(())
}
