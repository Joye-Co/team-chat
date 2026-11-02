// Loads Lucide icons from the gpui-kit-assets crate (the same SVGs the app ships) and caches
// them in icons.json, so the mockups use exactly the glyphs GPUI will draw.
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const NAMES = [
  "search", "inbox", "at-sign", "bell", "bell-off", "panel-right", "ellipsis", "square-pen",
  "paperclip", "image", "arrow-up", "text-quote", "reply", "copy", "check", "file-text",
  "file-code", "globe", "file", "settings", "chevron-down", "chevron-right", "chevron-up-down",
  "users", "user-plus", "bot", "server", "activity", "code", "user", "loader", "circle-alert",
  "wifi-off", "x", "command", "corner-down-left", "circle-check", "clock", "external-link",
  "plus", "list-filter", "terminal", "triangle-alert", "eye", "keyboard", "git-branch",
  "log-out", "chart-column", "circle-dashed", "circle", "smile-plus", "arrow-down",
  "chevrons-up-down", "shield-check", "key-round", "message-square-plus", "pin", "sparkles",
  "lock", "refresh-cw", "circle-dot", "monitor", "panel-left", "mail",
];

const cache = new URL("./icons.json", import.meta.url);

function registryDir() {
  const root = join(homedir(), ".cargo/registry/src");
  if (!existsSync(root)) return null;
  for (const index of readdirSync(root)) {
    const dir = join(root, index, "gpui-kit-assets-0.8.1/assets/icons");
    if (existsSync(dir)) return dir;
  }
  return null;
}

export function loadIcons() {
  const dir = registryDir();
  if (!dir) return JSON.parse(readFileSync(cache, "utf8"));
  const icons = {};
  for (const name of NAMES) {
    const file = join(dir, `${name}.svg`);
    if (!existsSync(file)) continue;
    const svg = readFileSync(file, "utf8");
    icons[name] = svg.replace(/^[\s\S]*?>\s*/, "").replace(/<\/svg>\s*$/, "").replace(/\s+/g, " ").trim();
  }
  writeFileSync(cache, `${JSON.stringify(icons, null, 1)}\n`);
  return icons;
}
