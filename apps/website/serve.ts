// Local preview: builds once, rebuilds when a file under src/ really changes, and serves dist/ on 127.0.0.1.
import { watch } from "node:fs";
import { readdir, stat } from "node:fs/promises";
import { join } from "node:path";
import { build, dist } from "./build";

const port = Number(process.env.PORT ?? 4321);
const src = join(import.meta.dir, "src");

// Size and mtime of every file under src/. Reading src/ during a build can fire watch events on
// macOS, so a rebuild runs only when this fingerprint differs from the last build's.
async function fingerprint(): Promise<string> {
  const entries = await readdir(src, { recursive: true, withFileTypes: true });
  const parts: string[] = [];
  for (const entry of entries) {
    if (!entry.isFile()) continue;
    const path = join(entry.parentPath, entry.name);
    const info = await stat(path);
    parts.push(`${path}:${info.size}:${info.mtimeMs}`);
  }
  return parts.sort().join("\n");
}

let last = await fingerprint();
await build();

// Throttle, not debounce: a steady stream of events (another process reading src/) must not
// postpone the check forever.
let pending: ReturnType<typeof setTimeout> | undefined;
watch(src, { recursive: true }, () => {
  if (pending) return;
  pending = setTimeout(async () => {
    pending = undefined;
    const now = await fingerprint();
    if (now === last) return;
    last = now;
    await build();
    console.log("rebuilt");
  }, 150);
});

Bun.serve({
  hostname: "127.0.0.1",
  port,
  async fetch(req) {
    const path = decodeURIComponent(new URL(req.url).pathname);
    let file = Bun.file(join(dist, path.endsWith("/") ? `${path}index.html` : path));
    // Like the static host: /en serves en.html.
    if (!(await file.exists()) && !path.includes(".")) file = Bun.file(join(dist, `${path}.html`));
    return (await file.exists()) ? new Response(file) : new Response("Not found", { status: 404 });
  },
});
console.log(`http://127.0.0.1:${port}/`);
