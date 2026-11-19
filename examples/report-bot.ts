#!/usr/bin/env bun
// Report bot: a Card v2.1 that changes in place. Mention it and it answers with one living card
// instead of a wall of text: a row of figures, a line chart, a question and a button. Every
// press comes back to the bot, and the bot answers by patching the same card:
//
//   a chart point      -> the note under the chart says what that week was
//   按渠道拆开 (button)  -> a table per channel appears under the chart
//   有问题 (live pick)   -> an input asks which figure is wrong, before the submit
//   发给 Ledger (submit) -> the stored answer: the card closes its question and says what is next
//
//   CHAT_URL=http://127.0.0.1:8789 \
//   CHAT_TOKEN_FILE=~/.config/team-chat/local/bot-report-<id>.token \
//   bun examples/report-bot.ts
//
// Optional: CHAT_STATE_FILE (default: <token file>.state.json), LOG_LEVEL (debug|info|warn|error).
// The numbers are sample data (`REPORT`); a real bot reads its own source there.
import { homedir } from "node:os";
import { blocks, Bot, type CardV2, composeCard, type LogLevel } from "../packages/bot-sdk/src/index.ts";

const WEEKS = ["10-06", "10-13", "10-20", "10-27"];
const REPORT = {
  month: "10 月",
  groups: { value: 38, delta: 4 },
  dau: { value: 2340, delta: 90 },
  reply: { value: 1.2, delta: -0.3 },
  series: [
    { id: "oct", name: "10 月", values: [2210, 2268, 2301, 2340] },
    { id: "sep", name: "9 月", values: [2150, 2190, 2236, 2250] },
  ],
  // 渠道, 新增群, 占比; the total is a normal last row.
  channels: [
    { key: "weibo", cells: ["微博", "15", "39%"] },
    { key: "invite", cells: ["邀请", "11", "29%"] },
    { key: "bili", cells: ["B 站", "7", "18%"] },
    { key: "douyin", cells: ["抖音", "5", "13%"] },
  ],
};

/** The submit action; `inputs` names the blocks whose values the answer carries. */
const send = (inputs: string[]) => [{ id: "send", label: "发给 Ledger", style: "primary" as const, kind: "submit" as const, inputs }];

/** The report as posted (rev 1). `from` is who may answer the question: the member who asked. */
export function reportCard(from: string): CardV2 {
  return composeCard({
    intent: "done",
    title: `${REPORT.month}数据月报`,
    summary: `新增群 ${REPORT.groups.value} 个，日活 ${REPORT.dau.value.toLocaleString("en-US")}。`,
    // A question on a report: it does not block the topic, and it says what waits for the answer.
    ask: { id: "verify", from: [from], blocking: false, promise: "你确认之前，我不会把月报发到全员群。" },
    blocks: [
      blocks.row("kpis", [
        blocks.metric({ id: "m_groups", label: "新增群", ...REPORT.groups, better: "up", note: "比 9 月" }),
        blocks.metric({ id: "m_dau", label: "日活", ...REPORT.dau, better: "up", note: "比 9 月" }),
        blocks.metric({ id: "m_reply", label: "平均回复", ...REPORT.reply, unit: "秒", better: "down", note: "比 9 月" }),
      ]),
      blocks.row(
        "main",
        [
          blocks.stack("chart_col", [
            blocks.chart({
              id: "trend",
              kind: "line",
              label: "日活 · 近 4 周",
              emphasis: "high",
              x: WEEKS,
              series: REPORT.series,
              summary: "10 月日活从 2,210 涨到 2,340，每周都比 9 月高。",
              events: ["chart.select"], // a point press reaches onCardEvent
            }),
            blocks.text("point", "点图上的一个点，看那一周的数。"),
            // `buttons` sends button.press; the builder declares it.
            blocks.buttons({ id: "more", buttons: [{ id: "channels", label: "按渠道拆开" }] }),
          ]),
          blocks.stack("check_col", [
            blocks.decision({
              id: "check",
              label: "这个数据对不对？",
              required: true,
              deliver: "onSubmit", // the answer goes with the submit action
              events: ["choice.change"], // and each pick reaches onCardEvent before that
              options: [
                { id: "ok", label: "对" },
                { id: "wrong", label: "有问题" },
              ],
            }),
          ]),
        ],
        { weights: [2, 1] },
      ),
    ],
    actions: send(["check"]),
  });
}

const has = (card: CardV2, id: string) => card.blocks.some((b) => b.id === id);

/** The handlers of the loop. Every value a handler reads comes from a person's client: check it. */
export function attach(bot: Bot): void {
  bot.onMention(async (ctx) => {
    await ctx.ack(); // 👀: seen, working on it
    await ctx.postCard(reportCard(ctx.message.authorId));
  });

  // A point of the chart: say what it is, in the card.
  bot.onCardEvent("chart.select", async (ctx) => {
    const p = ctx.point();
    const series = REPORT.series.find((s) => s.id === p?.series);
    const i = p?.index ?? (typeof p?.x === "string" ? WEEKS.indexOf(p.x) : -1);
    const value = series?.values[i];
    if (!series || value === undefined) return; // not a point of this chart: the SDK resolves "ignored"
    if (!ctx.isLatest()) return; // a newer point was picked meanwhile: latest wins
    await ctx.patch((d) => d.set<"text">("point", { text: `${WEEKS[i]} 这一周，${series.name}日活 ${value.toLocaleString("en-US")}。` }));
  });

  // The inline button: add the channel table once, under the chart.
  bot.onCardEvent("button.press", async (ctx) => {
    // `ctx.card` is the newest rev the SDK knows: a second press finds the table already there.
    if (ctx.button() !== "channels" || has(ctx.card, "drill")) return;
    await ctx.patch((d) => {
      d.insert(
        blocks.group({
          id: "drill",
          title: "按渠道拆开 · 新增群",
          collapsible: false,
          children: [
            blocks.table({
              id: "by_channel",
              columns: ["渠道", "新增群", "占比"],
              rows: [...REPORT.channels.map((c) => c.cells), ["合计", String(REPORT.groups.value), "100%"]],
              rowKeys: [...REPORT.channels.map((c) => c.key), "total"],
            }),
          ],
        }),
        { after: "main" },
      );
      d.remove("more"); // the button did its work
    });
  });

  // A live pick of the question, before the submit: "有问题" asks which figure, "对" takes that back.
  bot.onCardEvent({ name: "choice.change", block: "check" }, async (ctx) => {
    const wrong = ctx.choice() === "wrong";
    if (wrong === has(ctx.card, "what")) return; // nothing to change: the SDK resolves "ignored"
    await ctx.patch((d) => {
      if (wrong) {
        d.insert(blocks.input({ id: "what", kind: "textarea", label: "哪一项不对？后台看到的是多少？", maxLength: 300 }), { parent: "check_col" });
        d.actions(send(["check", "what"]));
      } else {
        d.remove("what");
        d.actions(send(["check"]));
      }
    });
  });

  // The submit: a stored answer, so it also arrives on catch-up after a restart.
  bot.onCardAnswer(async (ctx) => {
    if (ctx.askId !== "verify") return;
    const wrong = ctx.choice("check") === "wrong";
    const what = ctx.text("what")?.trim().slice(0, 300);
    await ctx.patch((d) => {
      d.ask(null); // the question is closed
      d.actions([]);
      d.cardProps({ summary: wrong ? "有一项数据要核对，核对完我改这张卡片。" : "数据已确认。" });
      d.replace("check", blocks.evidence({ id: "check", label: wrong ? `要核对：${what || "没写是哪一项"}` : "数据已确认", state: wrong ? "info" : "pass" }));
      if (d.block("what")) d.remove("what");
    });
    await ctx.updateTopic(wrong ? { status: "in_progress", waitingOn: [] } : { status: "done", waitingOn: [] });
  });
}

function expand(path: string): string {
  return path.startsWith("~/") ? `${homedir()}${path.slice(1)}` : path;
}

if (import.meta.main) {
  const url = process.env.CHAT_URL;
  const tokenFile = process.env.CHAT_TOKEN_FILE && expand(process.env.CHAT_TOKEN_FILE);
  if (!url || !tokenFile) {
    console.error("set CHAT_URL and CHAT_TOKEN_FILE");
    process.exit(2);
  }
  const bot = new Bot({
    url,
    tokenFile,
    stateFile: expand(process.env.CHAT_STATE_FILE ?? `${tokenFile}.state.json`),
    client: { name: "report-bot", version: "1.1.0" },
    logLevel: (process.env.LOG_LEVEL as LogLevel | undefined) ?? "info",
  });
  attach(bot);
  bot.onConnection((up) => bot.log.info(up ? "online" : "offline, reconnecting"));
  for (const signal of ["SIGINT", "SIGTERM"] as const) {
    process.on(signal, () => {
      void bot.stop().then(() => process.exit(0));
    });
  }
  await bot.start();
  bot.log.info("report bot ready", { member: bot.me.name });
}
