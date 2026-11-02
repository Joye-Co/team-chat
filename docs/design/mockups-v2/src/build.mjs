// Builds every v2 mockup as a self-contained HTML file and renders it to PNG with headless Chrome
// at device scale 1. Run: bun docs/design/mockups-v2/src/build.mjs [name ...] [--no-png] [--scale=2]
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { page } from "./kit.mjs";
import { MAIN_CSS } from "./main.mjs";
import { LOGIN_CSS, SETTINGS_CSS, SWITCHER_CSS, login, main, settings, short, switcher, worst } from "./screens.mjs";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..");
const CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

const both = (file, fn, extra = {}) => ["light", "dark"].map((mode) => ({ file: `${file}-${mode}`, mode, fn, ...extra }));
const screens = [
  ...both("main", main),
  ...both("short-topic", short),
  ...both("worst-case", worst, { width: 1100, height: 680 }),
  ...both("switcher", switcher, { css: SWITCHER_CSS }),
  ...both("settings-members", settings, { width: 900, height: 640, css: SETTINGS_CSS }),
  ...both("login", login, { width: 440, height: 520, css: LOGIN_CSS }),
];

const args = process.argv.slice(2);
const only = args.filter((a) => !a.startsWith("--"));
const noPng = args.includes("--no-png");
const scale = Number(args.find((a) => a.startsWith("--scale="))?.slice(8) ?? 1);

for (const s of screens) {
  if (only.length && !only.some((o) => s.file === o || s.file.startsWith(`${o}-`))) continue;
  const width = s.width ?? 1440;
  const height = s.height ?? 900;
  const html = page({ title: s.file, mode: s.mode, width, height, css: MAIN_CSS + (s.css ?? ""), body: s.fn(s.mode) });
  const htmlPath = join(OUT, `${s.file}.html`);
  writeFileSync(htmlPath, html);
  if (noPng) continue;
  const profile = mkdtempSync(join(tmpdir(), "mockups-v2-"));
  try {
    execFileSync(CHROME, [
      "--headless=new", "--hide-scrollbars", `--force-device-scale-factor=${scale}`,
      `--window-size=${width},${height}`, `--user-data-dir=${profile}`,
      `--screenshot=${join(OUT, `${s.file}${scale === 1 ? "" : `@${scale}x`}.png`)}`, `file://${htmlPath}`,
    ], { stdio: "ignore" });
  } finally {
    rmSync(profile, { recursive: true, force: true });
  }
  console.log(`${s.file}.png`);
}
