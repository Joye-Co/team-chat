// Builds the static homepage (Chinese index.html, English en.html) into dist/: copies src/ and fills {{PRODUCT_NAME}} from product.json,
// the only place the product name lives (AGENTS.md).
//
// The promo film is optional and is not in this repository: its frames carry the operator's character art.
// Put film-zh.mp4, film-zh.jpg, film-en.mp4 and film-en.jpg in WEBSITE_MEDIA (default
// ~/.config/team-chat/website/media) and the build copies them to dist/media/; a page whose film is missing
// is built without its film block (<!-- film --> … <!-- /film -->).
//
// The site's address is an operator's deployment detail, so it is not in this repository either: set
// WEBSITE_ORIGIN (for example https://chat.example.com) and the build fills {{ORIGIN}} in the tags that need
// an absolute URL (share image, og:url, canonical, hreflang), kept between <!-- origin --> and <!-- /origin -->.
// Without it those tags are left out and the relative ones between <!-- no-origin --> and <!-- /no-origin --> stay.
import { access, cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";

const here = import.meta.dir;
const src = join(here, "src");
export const dist = join(here, "dist");
const media = process.env.WEBSITE_MEDIA ?? join(homedir(), ".config/team-chat/website/media");
const PAGES: Record<string, string> = { "index.html": "zh", "en.html": "en" };


// The origin as scheme://host[:port], without a path or a trailing slash; undefined when the variable is not set.
function siteOrigin(): string | undefined {
  const raw = process.env.WEBSITE_ORIGIN?.trim();
  if (!raw) return undefined;
  const url = new URL(raw);
  if ((url.protocol !== "https:" && url.protocol !== "http:") || url.pathname !== "/" || url.search || url.hash) {
    throw new Error("WEBSITE_ORIGIN must be an origin such as https://chat.example.com, with no path");
  }
  return url.origin;
}

const block = (name: string) => new RegExp(`[ \\t]*<!-- ${name} -->\\n?([\\s\\S]*?)[ \\t]*<!-- /${name} -->\\n?`, "g");

const exists = (path: string) => access(path).then(() => true, () => false);

export async function build(): Promise<void> {
  const origin = siteOrigin();
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
    // Keep the tags of the case that applies, drop the other block.
    text = text.replace(block(origin ? "origin" : "no-origin"), "$1").replace(block(origin ? "no-origin" : "origin"), "");
    if (origin) text = text.replaceAll("{{ORIGIN}}", origin);
    await writeFile(path, text);
  }
}

if (import.meta.main) {
  await build();
  console.log(`built ${dist}`);
}
