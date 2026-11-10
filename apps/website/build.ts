// Builds the static homepage into dist/: copies src/ and fills {{PRODUCT_NAME}} from product.json,
// the only place the product name lives (AGENTS.md).
import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";

const here = import.meta.dir;
const src = join(here, "src");
export const dist = join(here, "dist");

export async function build(): Promise<void> {
  const product = JSON.parse(await readFile(join(here, "../../product.json"), "utf8")) as { name: string };
  await rm(dist, { recursive: true, force: true });
  await mkdir(dist, { recursive: true });
  await cp(src, dist, { recursive: true });
  for (const file of ["index.html"]) {
    const path = join(dist, file);
    const text = await readFile(path, "utf8");
    await writeFile(path, text.replaceAll("{{PRODUCT_NAME}}", product.name));
  }
}

if (import.meta.main) {
  await build();
  console.log(`built ${dist}`);
}
