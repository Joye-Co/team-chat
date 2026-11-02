// Writes the app icon masters to docs/design/brand/ and renders PNGs with headless Chrome.
// icon.svg is the 1024 master; icon-dark.svg the dark appearance; icon-small.svg the optical
// variant for 16 and 32 px (thicker strokes, larger shapes). Run: bun docs/design/mockups/src/brand.mjs
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { appIconSvg } from "./screens.mjs";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "../../brand");
const CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

const masters = {
  "icon.svg": appIconSvg(1024, { variant: "light", id: "m" }),
  "icon-dark.svg": appIconSvg(1024, { variant: "dark", id: "m" }),
  "icon-small.svg": appIconSvg(32, { variant: "light", id: "m" }).replace(/width="32" height="32"/, 'width="1024" height="1024"'),
  "icon-small-dark.svg": appIconSvg(32, { variant: "dark", id: "m" }).replace(/width="32" height="32"/, 'width="1024" height="1024"'),
};
for (const [name, svg] of Object.entries(masters)) writeFileSync(join(OUT, name), `${svg}\n`);

const renders = [
  ...[1024, 512, 256, 128, 64].map((s) => ({ src: "icon.svg", out: `icon-${s}.png`, s })),
  ...[32, 16].map((s) => ({ src: "icon-small.svg", out: `icon-${s}.png`, s })),
  ...[1024, 128, 64].map((s) => ({ src: "icon-dark.svg", out: `icon-dark-${s}.png`, s })),
  ...[32, 16].map((s) => ({ src: "icon-small-dark.svg", out: `icon-dark-${s}.png`, s })),
];
for (const r of renders) {
  const html = join(OUT, `.render-${r.out}.html`);
  writeFileSync(html, `<!doctype html><html><body style="margin:0;background:transparent"><img src="${r.src}" width="${r.s}" height="${r.s}" style="display:block"></body></html>`);
  const profile = mkdtempSync(join(tmpdir(), "brand-chrome-"));
  try {
    execFileSync(CHROME, ["--headless=new", "--hide-scrollbars", "--force-device-scale-factor=1", `--window-size=${r.s},${r.s}`,
      `--user-data-dir=${profile}`, "--default-background-color=00000000", `--screenshot=${join(OUT, r.out)}`, `file://${html}`], { stdio: "ignore" });
  } finally {
    rmSync(profile, { recursive: true, force: true });
    rmSync(html, { force: true });
  }
  console.log(r.out);
}
