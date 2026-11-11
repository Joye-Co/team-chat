// Writes docs/plans/windows-parity/interface-scenes.zh.md from two runs of the screenshot
// self-test (`desktop --screenshot`): one on a Mac, one on Windows CI.
//
//   bun scripts/windows-scene-table.ts <mac screenshots.json> <windows screenshots.json> <windows run id>
//
// Every row comes from the two `screenshots.json` files, so the table cannot say more
// than the runs do. What a person saw when comparing a pair is kept beside it in
// docs/plans/windows-parity/interface-scenes.notes.json (scene -> differences); a scene
// without a note reads 一样.
import { readFileSync, writeFileSync } from "node:fs";

type Shot = { scene: string; theme: string; ok: boolean; error?: string };
type Run = { shots: Shot[] };
type Note = { first: string; now: string };
type Notes = { common: Note[]; scenes: Record<string, Note[]> };

const [macPath, winPath, runId] = process.argv.slice(2);
if (!macPath || !winPath || !runId) {
  console.error("usage: bun scripts/windows-scene-table.ts <mac screenshots.json> <windows screenshots.json> <windows run id>");
  process.exit(2);
}
const dir = "docs/plans/windows-parity";
const read = (path: string): Run => JSON.parse(readFileSync(path, "utf8"));
const notes: Notes = JSON.parse(readFileSync(`${dir}/interface-scenes.notes.json`, "utf8"));

/// scene -> its result: 通过, 失败 with the errors, or 没有跑.
function results(run: Run): Map<string, string> {
  const errors = new Map<string, Set<string>>();
  for (const shot of run.shots) {
    const seen = errors.get(shot.scene) ?? new Set<string>();
    if (!shot.ok) seen.add(shot.error ?? "failed");
    errors.set(shot.scene, seen);
  }
  return new Map([...errors].map(([scene, e]) => [scene, e.size ? `失败：${[...e].join("；")}` : "通过"]));
}

const mac = results(read(macPath));
const win = results(read(winPath));
const scenes = [...new Set([...mac.keys(), ...win.keys()])].sort();
const unique = (items: string[]) => [...new Set(items)].join("；");
const failing = scenes.filter((s) => win.get(s)?.startsWith("失败"));

const lines = [
  "# Windows 对齐清单：界面，逐场景对比",
  "",
  "这个文件由 `scripts/windows-scene-table.ts` 生成，不要手改；每个场景看到的差别写在",
  "`interface-scenes.notes.json` 里。主清单是 [interface.zh.md](interface.zh.md)。",
  "",
  `- Windows：CI 的 \`desktop-windows\` 任务，\`windows_scenes=all\`，**run ${runId}**，产物 \`windows-screenshots\`（\`<场景>-<light|dark>.png\`，内嵌页面另见 \`screen/\`）。`,
  "- Mac：一台 Mac 上 `desktop --screenshot` 的结果（2 倍，缩到 1 倍再比）。",
  `- 场景 ${scenes.length} 个；Windows 上通过 ${scenes.length - failing.length} 个${failing.length ? `，失败 ${failing.length} 个（${failing.map((s) => `\`${s}\``).join("、")}）` : "，没有失败的"}。`,
  "- 不算差别、所以不在表里的：标题条和窗口按钮、内容整体下移 32、字体和图标集本身、表情的画法、AI 成员头像（开源构建没有图包）。",
  "",
  "所有主窗口场景共有的差别（表里不逐行重复）：",
  "",
  ...notes.common.map((n) => `- ${n.first}：${n.now}`),
  "",
  "| 场景 | Mac | Windows | 比较时看到的差别 | 现在 |",
  "| --- | --- | --- | --- | --- |",
  ...scenes.map((scene) => {
    const seen = notes.scenes[scene] ?? [];
    const first = seen.length ? unique(seen.map((n) => n.first)) : "一样";
    const now = seen.length ? unique(seen.map((n) => n.now)) : "一样";
    return `| \`${scene}\` | ${mac.get(scene) ?? "没有跑"} | ${win.get(scene) ?? "没有跑"} | ${first} | ${now} |`;
  }),
  "",
];
writeFileSync(`${dir}/interface-scenes.zh.md`, lines.join("\n"));
console.log(`${scenes.length} scenes, ${failing.length} failing on Windows`);
