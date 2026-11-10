// Local preview: builds once, rebuilds when src/ changes, and serves dist/ on 127.0.0.1.
import { watch } from "node:fs";
import { join } from "node:path";
import { build, dist } from "./build";

const port = Number(process.env.PORT ?? 4321);
await build();

let pending: ReturnType<typeof setTimeout> | undefined;
watch(join(import.meta.dir, "src"), { recursive: true }, () => {
  clearTimeout(pending);
  pending = setTimeout(() => build().then(() => console.log("rebuilt")), 100);
});

Bun.serve({
  hostname: "127.0.0.1",
  port,
  async fetch(req) {
    const path = decodeURIComponent(new URL(req.url).pathname);
    const file = Bun.file(join(dist, path.endsWith("/") ? `${path}index.html` : path));
    return (await file.exists()) ? new Response(file) : new Response("Not found", { status: 404 });
  },
});
console.log(`http://127.0.0.1:${port}/`);
