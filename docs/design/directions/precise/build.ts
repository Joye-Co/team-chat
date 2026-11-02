// Builds the self-contained mockups: inlines shared.css, partials and the Lucide
// symbols each page uses. Run: bun docs/design/directions/precise/build.ts
import { readFileSync, writeFileSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";

const here = import.meta.dir;
const src = join(here, "src");
const registry = join(homedir(), ".cargo/registry/src");
const kit = readdirSync(registry)
  .map((d) => join(registry, d, "gpui-kit-assets-0.8.1/assets/icons"))
  .find((p) => existsSync(p));
if (!kit) throw new Error("gpui-kit-assets-0.8.1 not found: run cargo fetch");

const css = readFileSync(join(src, "shared.css"), "utf8");

function include(html: string): string {
  return html.replace(/<!--@include (\S+)-->/g, (_, name) =>
    include(readFileSync(join(src, "partials", `${name}.html`), "utf8")),
  );
}

function sprite(html: string): string {
  const names = new Set([...html.matchAll(/href="#i-([a-z0-9-]+)"/g)].map((m) => m[1]));
  const symbols = [...names].sort().map((name) => {
    const svg = readFileSync(join(kit!, `${name}.svg`), "utf8");
    const body = svg.replace(/^[\s\S]*?<svg[^>]*>/, "").replace(/<\/svg>\s*$/, "").trim();
    return `<symbol id="i-${name}" viewBox="0 0 24 24">${body}</symbol>`;
  });
  return `<svg width="0" height="0" style="position:absolute" aria-hidden="true">${symbols.join("")}</svg>`;
}

for (const file of readdirSync(src).filter((f) => f.endsWith(".html"))) {
  let html = include(readFileSync(join(src, file), "utf8"));
  html = html.replace("<!--@css-->", `<style>\n${css}\n</style>`);
  html = html.replace("<!--@icons-->", sprite(html));
  writeFileSync(join(here, file), html);
  console.log("built", file);
}
