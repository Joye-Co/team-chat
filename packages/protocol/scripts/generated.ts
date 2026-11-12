// Shared by the generators: each one owns a whole folder (protocol/fixtures, protocol/schema).
// `writeTree` makes the folder hold exactly the generated files; `diffTree` lists how the folder
// differs from them, for `--check` and for test/generated.test.ts.
import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";

/** Relative path (with `/`) to file text. */
export type Tree = Map<string, string>;

function walk(dir: string): string[] {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return [];
  }
  return entries.flatMap((e) => (e.isDirectory() ? walk(join(dir, e.name)) : [join(dir, e.name)]));
}

function onDisk(root: string): string[] {
  return walk(root).map((f) => relative(root, f).split(sep).join("/"));
}

export function diffTree(root: string, tree: Tree): string[] {
  const problems: string[] = [];
  const disk = new Set(onDisk(root));
  for (const [path, text] of tree) {
    if (!disk.has(path)) problems.push(`missing ${path}`);
    else if (readFileSync(join(root, path), "utf8") !== text) problems.push(`changed ${path}`);
  }
  for (const path of disk) {
    if (!tree.has(path)) problems.push(`not generated ${path} (add it to packages/protocol/scripts or delete it)`);
  }
  return problems.sort();
}

export function writeTree(root: string, tree: Tree) {
  for (const path of onDisk(root)) if (!tree.has(path)) rmSync(join(root, path));
  for (const [path, text] of tree) {
    const file = join(root, path);
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, text);
  }
}

/** Write the tree, or with `--check` exit 1 when the folder differs from it. */
export function run(name: string, root: string, tree: Tree) {
  if (process.argv.includes("--check")) {
    const problems = diffTree(root, tree);
    if (problems.length > 0) {
      console.error(`${name} differ from the generator; run \`bun run gen:protocol\`:\n  ${problems.join("\n  ")}`);
      process.exitCode = 1;
      return;
    }
    console.log(`${name}: ${tree.size} files match`);
    return;
  }
  writeTree(root, tree);
  console.log(`wrote ${tree.size} ${name} to ${root}`);
}
