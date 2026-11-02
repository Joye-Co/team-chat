// Builds every mockup as a self-contained HTML file and renders it to PNG with headless Chrome.
// Run: bun docs/design/directions/native/src/build.mjs [--no-png]
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { page } from "./base.mjs";
import { mainData, worstData } from "./data.mjs";
import { MAIN_CSS, mainWindow } from "./main.mjs";
import {
  ICON_CSS, LOGIN_CSS, SETTINGS_CSS, SWITCHER_CSS, WINDOW_CSS, appIconSvg, iconSheet, login, settings, switcherOverlay,
} from "./screens.mjs";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..");
const CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

const screens = [
  { file: "main-light", theme: "light", body: () => mainWindow(mainData("light"), "light") },
  { file: "main-dark", theme: "dark", body: () => mainWindow({ ...mainData("dark"), scrolledUp: true, draft: `<span class="ph">在「登录页改版」里回复</span>`, draftEmpty: true }, "dark", { panelTab: "process", mentionPopup: false }) },
  { file: "settings-members", theme: "light", css: WINDOW_CSS + SETTINGS_CSS, body: () => settings("light") },
  { file: "settings-members-dark", theme: "dark", css: WINDOW_CSS + SETTINGS_CSS, body: () => settings("dark") },
  { file: "login", theme: "light", css: WINDOW_CSS + LOGIN_CSS, body: () => login("light") },
  { file: "login-dark", theme: "dark", css: WINDOW_CSS + LOGIN_CSS, body: () => login("dark") },
  {
    file: "switcher", theme: "light", css: SWITCHER_CSS,
    body: () => mainWindow({ ...mainData("light"), draft: `<span class="ph">在「登录页改版」里回复</span>`, draftEmpty: true, typing: "", noHover: true, composerIdle: true }, "light", { mentionPopup: false, overlay: switcherOverlay("light") }),
  },
  { file: "worst-case", theme: "light", body: () => mainWindow(worstData("light"), "light") },
  { file: "icon", theme: "light", css: ICON_CSS, body: () => iconSheet() },
  { file: "icon-1024", theme: "light", css: "body{background:transparent}", width: 1024, height: 1024, scale: 1, body: () => appIconSvg(1024, { id: "x" }) },
];

const only = process.argv.slice(2).filter((a) => !a.startsWith("--"));
const noPng = process.argv.includes("--no-png");

for (const s of screens) {
  if (only.length && !only.includes(s.file)) continue;
  const width = s.width ?? 1440;
  const height = s.height ?? 900;
  const html = page({ title: s.file, theme: s.theme, body: s.body(), width, height, extraCss: MAIN_CSS + (s.css ?? "") });
  const htmlPath = join(OUT, `${s.file}.html`);
  writeFileSync(htmlPath, html);
  if (noPng) continue;
  const profile = mkdtempSync(join(tmpdir(), "native-chrome-"));
  try {
    execFileSync(CHROME, [
      "--headless=new", "--hide-scrollbars", `--force-device-scale-factor=${s.scale ?? 2}`,
      `--window-size=${width},${height}`, `--user-data-dir=${profile}`,
      ...(s.file === "icon-1024" ? ["--default-background-color=00000000"] : []),
      `--screenshot=${join(OUT, `${s.file}.png`)}`, `file://${htmlPath}`,
    ], { stdio: "ignore" });
  } finally {
    rmSync(profile, { recursive: true, force: true });
  }
  console.log(`${s.file}.png`);
}
