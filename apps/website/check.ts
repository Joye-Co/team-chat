// Checks the built homepage in dist/ (run `bun run build` first):
//   1. no {{PLACEHOLDER}} is left in a page
//   2. every local file a page names exists: src, href, poster, and both the -light.webp and the
//      -dark.webp of every data-img (main.js builds those two names when the page runs)
//   3. every #id link has its target on the same page
//   4. the Chinese and the English page have the same sections, and every screenshot tour has
//      the same number of steps in both
// The film is optional (see build.ts): a page built without it has no film block, so nothing
// here needs the media folder.
// Run: bun check.ts
import { existsSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const dist = join(import.meta.dir, "dist");
const PAGES = ["index.html", "en.html"];

interface Page {
  ids: Set<string>;
  /** Local files the page names, as written. */
  files: string[];
  /** Targets of the #id links. */
  anchors: string[];
  /** The id of each <section>, in order ("" when it has none). */
  sections: string[];
  /** The number of steps of each tour, in order. */
  tours: number[];
}

async function read(html: string): Promise<Page> {
  const page: Page = { ids: new Set(), files: [], anchors: [], sections: [], tours: [] };
  const link = (value: string | null) => {
    if (!value) return;
    if (value.startsWith("#")) page.anchors.push(value.slice(1));
    else if (!/^([a-z][a-z0-9+.-]*:|\/\/)/i.test(value)) page.files.push(value);
  };
  let tour = -1;
  const rewriter = new HTMLRewriter()
    .on("*", {
      element(el) {
        const id = el.getAttribute("id");
        if (id) page.ids.add(id);
        for (const name of ["src", "href", "poster"]) link(el.getAttribute(name));
        const img = el.getAttribute("data-img");
        if (img) page.files.push(`${img}-light.webp`, `${img}-dark.webp`);
      },
    })
    .on("section", {
      element(el) {
        page.sections.push(el.getAttribute("id") ?? "");
      },
    })
    .on(".player", {
      element(el) {
        tour = page.tours.push(0) - 1;
        el.onEndTag(() => {
          tour = -1;
        });
      },
    })
    .on(".ps", {
      element() {
        if (tour >= 0) page.tours[tour] += 1;
      },
    });
  await rewriter.transform(new Response(html)).text();
  return page;
}

/** The file a local link is served from: itself, `<name>.html` (as `/en`), or a folder's index.html. */
function served(ref: string): boolean {
  const path = join(dist, decodeURIComponent(ref.split(/[?#]/)[0]));
  if (existsSync(path)) return statSync(path).isFile() || existsSync(join(path, "index.html"));
  return existsSync(`${path}.html`);
}

const problems: string[] = [];
const pages: Record<string, Page> = {};
for (const file of PAGES) {
  const path = join(dist, file);
  if (!existsSync(path)) {
    problems.push(`${file}: not built (run bun run build first)`);
    continue;
  }
  const html = readFileSync(path, "utf8");
  for (const m of new Set(html.match(/\{\{[^}]*\}\}/g) ?? [])) problems.push(`${file}: ${m} was not filled in`);
  const page = await read(html);
  pages[file] = page;
  for (const ref of new Set(page.files)) if (!served(ref)) problems.push(`${file}: ${ref} does not exist`);
  for (const id of new Set(page.anchors)) if (!page.ids.has(id)) problems.push(`${file}: the link #${id} has no target`);
  if (page.tours.length === 0) problems.push(`${file}: no screenshot tour found (.player)`);
}

const [zh, en] = PAGES.map((f) => pages[f]);
if (zh && en) {
  const list = (p: Page) => p.sections.map((id) => id || "(no id)").join(", ");
  if (list(zh) !== list(en)) problems.push(`sections differ: ${PAGES[0]} has ${list(zh)}; ${PAGES[1]} has ${list(en)}`);
  if (zh.tours.join(",") !== en.tours.join(",")) {
    problems.push(`steps per tour differ: ${PAGES[0]} has ${zh.tours.join(", ")}; ${PAGES[1]} has ${en.tours.join(", ")}`);
  }
}

if (problems.length > 0) {
  console.error(problems.map((p) => `website: ${p}`).join("\n"));
  process.exit(1);
}
console.log(`website: ok (${PAGES.join(", ")})`);
