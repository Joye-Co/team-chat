// Builds every final mockup as a self-contained HTML file and renders it to PNG with headless
// Chrome at device scale 1 (the brand icon renders are written by brand.mjs).
// Run: bun docs/design/mockups/src/build.mjs [scene ...] [--no-png] [--scale=2]
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { page } from "./base.mjs";
import { mainData, worstData } from "./data.mjs";
import { MAIN_CSS, mainWindow } from "./main.mjs";
import {
  ICON_CSS, LOGIN_CSS, SETTINGS_CSS, SWITCHER_CSS, WINDOW_CSS, iconSheet, login, settings, switcherOverlay,
} from "./screens.mjs";
import {
  ARTWIN_CSS, CARDS_CSS, EMPTY_CSS, PANEL_SHEET_CSS, STATES_CSS, artifactWindow, cardsSheet, emptySheet, panelSheet, statesSheet,
} from "./sheets.mjs";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..");
const CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

const quietDraft = { draft: `<span class="ph">在「登录页改版」里回复</span>`, draftEmpty: true };
const both = (file, fn, extra = {}) => [
  { file: `${file}-light`, theme: "light", body: () => fn("light"), ...extra },
  { file: `${file}-dark`, theme: "dark", body: () => fn("dark"), ...extra },
];

const screens = [
  ...both("main", (m) => mainWindow(mainData(m), m)),
  ...both("main-top", (m) => mainWindow({ ...mainData(m), ...quietDraft, scrolledUp: true, noHover: true }, m, { panelTab: "process", mentionPopup: false })),
  ...both("switcher", (m) => mainWindow({ ...mainData(m), ...quietDraft, typing: "", noHover: true, composerIdle: true }, m, { mentionPopup: false, overlay: switcherOverlay(m) }), { css: SWITCHER_CSS }),
  ...both("worst-case", (m) => mainWindow(worstData(m), m)),
  ...both("settings-members", (m) => settings(m), { css: WINDOW_CSS + SETTINGS_CSS }),
  ...both("login", (m) => login(m), { css: WINDOW_CSS + LOGIN_CSS }),
  ...both("side-panel", (m) => panelSheet(m), { css: PANEL_SHEET_CSS }),
  ...both("cards", (m) => cardsSheet(m), { css: CARDS_CSS }),
  ...both("empty-states", (m) => emptySheet(m), { css: EMPTY_CSS }),
  ...both("states", (m) => statesSheet(m), { css: STATES_CSS + SETTINGS_CSS }),
  ...both("artifact-window", (m) => artifactWindow(m), { css: WINDOW_CSS + ARTWIN_CSS }),
  { file: "icon", theme: "light", css: ICON_CSS, body: () => iconSheet() },
];

const args = process.argv.slice(2);
const only = args.filter((a) => !a.startsWith("--"));
const noPng = args.includes("--no-png");
const scale = Number(args.find((a) => a.startsWith("--scale="))?.slice(8) ?? 1);

for (const s of screens) {
  if (only.length && !only.some((o) => s.file === o || s.file.startsWith(`${o}-`))) continue;
  const width = s.width ?? 1440;
  const height = s.height ?? 900;
  const html = page({ title: s.file, theme: s.theme, body: s.body(), width, height, extraCss: MAIN_CSS + (s.css ?? "") });
  const htmlPath = join(OUT, `${s.file}.html`);
  writeFileSync(htmlPath, html);
  if (noPng) continue;
  const profile = mkdtempSync(join(tmpdir(), "final-chrome-"));
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
