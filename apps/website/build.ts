// Builds the static homepage (Chinese index.html, English en.html) into dist/: copies src/ and fills {{PRODUCT_NAME}} from product.json,
// the only place the product name lives (AGENTS.md).
//
// The promo film is optional and is not in this repository: its frames carry the operator's character art.
// Put film-zh.mp4, film-zh.jpg, film-en.mp4 and film-en.jpg in WEBSITE_MEDIA (default
// ~/.config/team-chat/website/media) and the build copies them to dist/media/; a page whose film is missing
// is built without its film block (<!-- film --> … <!-- /film -->).
import { access, cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";

const here = import.meta.dir;
const src = join(here, "src");
export const dist = join(here, "dist");
const media = process.env.WEBSITE_MEDIA ?? join(homedir(), ".config/team-chat/website/media");
const PAGES: Record<string, string> = { "index.html": "zh", "en.html": "en" };

const exists = (path: string) => access(path).then(() => true, () => false);

export async function build(): Promise<void> {
  const product = JSON.parse(await readFile(join(here, "../../product.json"), "utf8")) as { name: string };
  await rm(dist, { recursive: true, force: true });
  await mkdir(dist, { recursive: true });
  await cp(src, dist, { recursive: true });
  for (const [file, lang] of Object.entries(PAGES)) {
    const path = join(dist, file);
    let text = (await readFile(path, "utf8")).replaceAll("{{PRODUCT_NAME}}", product.name);
    const film = [`film-${lang}.mp4`, `film-${lang}.jpg`];
    if ((await Promise.all(film.map((f) => exists(join(media, f))))).every(Boolean)) {
      await mkdir(join(dist, "media"), { recursive: true });
      for (const f of film) await cp(join(media, f), join(dist, "media", f));
    } else {
      text = text.replace(/[ \t]*<!-- film -->[\s\S]*?<!-- \/film -->\n?/, "");
    }
    await writeFile(path, text);
  }
}

if (import.meta.main) {
  await build();
  console.log(`built ${dist}`);
}
