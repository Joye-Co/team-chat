// Measures the visible (painted) bounds of every character at small size (status lamp, bubble
// and the hidden shadow removed) with headless Chrome, and writes char-boxes.json. The bounds
// come from the rasterized pixels, not getBBox, so invisible layers (opacity 0) do not count.
// The avatar tile (DESIGN §7.9) uses them to fit each character by its larger side and center it.
// Run after the art changes: bun docs/design/mockups-v2/src/charbox.mjs
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
    try { svg = readFileSync(join(DIR, id, `${st}-24.svg`), "utf8"); } catch { continue; }
    const vb = svg.match(/viewBox="([^"]*)"/)[1].split(/\s+/).map(Number);
    svg = smallArt(svg).replace(/width="24" height="24"/, 'width="512" height="512"');
    items.push({ k: `${id}__${st}`, vb, src: `data:image/svg+xml;base64,${Buffer.from(svg).toString("base64")}` });
  }
}
const html = `<!doctype html><body><script>
const items=${JSON.stringify(items)};const out={};
Promise.all(items.map(it=>new Promise(res=>{const im=new Image();im.onload=()=>{const c=document.createElement('canvas');c.width=c.height=512;const g=c.getContext('2d');g.drawImage(im,0,0,512,512);const d=g.getImageData(0,0,512,512).data;let x0=512,y0=512,x1=-1,y1=-1;for(let y=0;y<512;y++)for(let x=0;x<512;x++){if(d[(y*512+x)*4+3]>24){if(x<x0)x0=x;if(x>x1)x1=x;if(y<y0)y0=y;if(y>y1)y1=y}}const s=it.vb[2]/512;out[it.k]=[it.vb[0]+x0*s,it.vb[1]+y0*s,(x1-x0+1)*s,(y1-y0+1)*s].map(v=>Math.round(v*10)/10);res()};im.src=it.src}))).then(()=>{document.body.innerHTML='<pre id=r>'+JSON.stringify(out)+'</pre>'});
</script></body>`;
const tmp = mkdtempSync(join(tmpdir(), "charbox-"));
try {
  const f = join(tmp, "b.html");
  writeFileSync(f, html);
  const dom = execFileSync("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", ["--headless=new", `--user-data-dir=${join(tmp, "p")}`, "--virtual-time-budget=20000", "--dump-dom", `file://${f}`], { encoding: "utf8", maxBuffer: 1 << 26, stdio: ["ignore", "pipe", "ignore"] });
  const json = dom.match(/<pre id="r">(.*?)<\/pre>/s)[1].replace(/&quot;/g, '"');
  writeFileSync(new URL("./char-boxes.json", import.meta.url), `${JSON.stringify(JSON.parse(json), null, 0)}\n`);
  console.log(Object.keys(JSON.parse(json)).length, "boxes");
} finally {
  rmSync(tmp, { recursive: true, force: true });
}
