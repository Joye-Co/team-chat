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
    if (!(await file.exists())) return new Response("Not found", { status: 404 });
    // Byte ranges, like the static host: a video is fetched in parts (Safari asks for a range before it plays).
    const range = req.headers.get("range")?.match(/^bytes=(\d*)-(\d*)$/);
    if (!range || (!range[1] && !range[2])) return new Response(file, { headers: { "Accept-Ranges": "bytes" } });
    const size = file.size;
    const start = range[1] ? Number(range[1]) : Math.max(0, size - Number(range[2]));
    const end = range[1] && range[2] ? Math.min(size - 1, Number(range[2])) : size - 1;
    if (start > end || start >= size) return new Response(null, { status: 416, headers: { "Content-Range": `bytes */${size}` } });
    return new Response(file.slice(start, end + 1), {
      status: 206,
      headers: { "Accept-Ranges": "bytes", "Content-Range": `bytes ${start}-${end}/${size}`, "Content-Type": file.type },
    });
  },
});
console.log(`http://127.0.0.1:${port}/`);
