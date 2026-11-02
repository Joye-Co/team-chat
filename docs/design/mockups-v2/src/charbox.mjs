// Measures the visible bounds of every character at small size (status lamp and bubble removed)
// with headless Chrome, and writes char-boxes.json. The avatar tile (DESIGN §7.9) uses these
// bounds to fit each character by its larger side and center it. Run after the art changes:
// bun docs/design/mockups-v2/src/charbox.mjs
import { execFileSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { REPO, smallArt } from "./kit.mjs";

const DIR = join(REPO, "apps/desktop/assets/characters");
const STATES = ["idle", "working", "needs-input", "success", "error", "offline"];
const items = [];
for (const id of readdirSync(DIR)) {
  for (const st of STATES) {
    let svg;
    try { svg = readFileSync(join(DIR, id, `${st}-128.svg`), "utf8"); } catch { continue; }
    // Unique ids per inline copy so clip paths do not collide.
    const k = `${id}__${st}`;
    svg = smallArt(svg).replace(/id="([^"]+)"/g, `id="${k}-$1"`).replace(/url\(#([^)]+)\)/g, `url(#${k}-$1)`);
    items.push(`<div data-k="${k}">${svg}</div>`);
  }
}
const html = `<!doctype html><body>${items.join("")}<script>
const out={};for(const d of document.querySelectorAll('[data-k]')){const s=d.querySelector('svg');const b=s.getBBox();out[d.dataset.k]=[b.x,b.y,b.width,b.height].map(v=>Math.round(v*10)/10)}
document.body.innerHTML='<pre id=r>'+JSON.stringify(out)+'</pre>';</script></body>`;
const tmp = mkdtempSync(join(tmpdir(), "charbox-"));
try {
  const f = join(tmp, "b.html");
  writeFileSync(f, html);
  const dom = execFileSync("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", ["--headless=new", `--user-data-dir=${join(tmp, "p")}`, "--dump-dom", `file://${f}`], { encoding: "utf8", maxBuffer: 1 << 26, stdio: ["ignore", "pipe", "ignore"] });
  const json = dom.match(/<pre id="r">(.*?)<\/pre>/s)[1].replace(/&quot;/g, '"');
  writeFileSync(new URL("./char-boxes.json", import.meta.url), `${JSON.stringify(JSON.parse(json), null, 0)}\n`);
  console.log(Object.keys(JSON.parse(json)).length, "boxes");
} finally {
  rmSync(tmp, { recursive: true, force: true });
}
