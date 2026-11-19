// What `chat-run card example` and `chat-run artifact example` print (apps/daemon/examples) is a
// set of fixtures: the Zod and the Rust fixture tests take every example, so an example can
// never teach a member a card, an op or an events file that the protocol refuses.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import type { InteractCtx } from "./fixtures-v11-interact.ts";

const examples = join(import.meta.dir, "../../../apps/daemon/examples");
const read = (path: string): unknown => JSON.parse(readFileSync(join(examples, path), "utf8"));

export function addExampleFixtures({ ok, ID }: Pick<InteractCtx, "ok" | "ID">) {
  for (const file of readdirSync(join(examples, "cards")).sort()) {
    if (!file.endsWith(".json")) continue;
    const kind = file.slice(0, -".json".length);
    const value = read(`cards/${file}`);
    if (kind === "patch") {
      // The ops of `chat-run card example patch`, in the frame `chat-run card patch` sends.
      ok(
        "client/card.patch-example.json",
        "ClientFrame",
        { t: "card.patch", rid: "r-example", groupId: ID.general, messageId: ID.msg, baseRev: 1, ops: value },
        "chat-run card example patch (apps/daemon/examples/cards/patch.json)",
      );
    } else {
      ok(`entities/card-v2-example-${kind}.json`, "CardV2", value, `chat-run card example ${kind} (apps/daemon/examples/cards/${file})`);
    }
  }
  // The starter page of `chat-run artifact example`: its events and first data, as the request
  // `chat-run artifact create` sends, with the preview that `--summary` and `--metrics` build.
  ok(
    "http/machine-create-artifact-request-example.json",
    "MachineCreateArtifactRequest",
    {
      title: "发布前清单",
      kind: "html",
      entry: "index.html",
      files: [{ path: "index.html", fileId: ID.msg }],
      events: read("page/events.json"),
      data: read("page/data.json"),
      preview: { summary: "发布前要做完的三件事", metrics: [{ label: "已完成", value: "1 / 3" }] },
    },
    "chat-run artifact example (apps/daemon/examples/page)",
  );
}
