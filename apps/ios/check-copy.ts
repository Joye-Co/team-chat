// Checks the iPhone client's interface text, with the same rules as the desktop catalog test
// (apps/desktop/src/copy/mod.rs):
//   1. no developer word (the list in docs/PRODUCT.md) in the catalog, apps/ios/Sources/Copy.swift
//   2. the product name is never typed
//   3. no Chinese text in a string of any other Swift file: it must come from the catalog
// Run: bun apps/ios/check-copy.ts
import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";

const root = join(import.meta.dir, "../..");
const ios = join(root, "apps/ios");
const catalog = join(ios, "Sources/Copy.swift");

function developerWords(): { ascii: string[]; cjk: string[] } {
  const product = readFileSync(join(root, "docs/PRODUCT.md"), "utf8");
  const start = product.indexOf("Developer words appear only");
  if (start < 0) throw new Error("docs/PRODUCT.md lists no developer words");
  let rest = product.slice(start);
  rest = rest.slice(rest.indexOf(":") + 1);
  const end = rest.indexOf("\n- ");
  const list = (end < 0 ? rest : rest.slice(0, end)).replaceAll("\n", " ").trim().replace(/\.$/, "");
  const ascii: string[] = [];
  const cjk: string[] = [];
  for (const raw of list.split(/[,，]/)) {
    const item = raw.trim().replace(/^and their Chinese forms/, "").trim().replace(/^and /, "").trim();
    if (!item) continue;
    if (item === "model names") ascii.push("model", "claude", "codex", "gpt", "gemini", "opus", "sonnet", "haiku");
    else if (/^[\x00-\x7f]+$/.test(item)) ascii.push(item.toLowerCase());
    else cjk.push(item);
  }
  return { ascii, cjk };
}

/// The string literals of a Swift file, with comments removed and `\(...)` left in place.
function strings(source: string): { line: number; text: string }[] {
  const out: { line: number; text: string }[] = [];
  source.split("\n").forEach((raw, i) => {
    let inString = false;
    let text = "";
    for (let j = 0; j < raw.length; j++) {
      const c = raw[j];
      if (inString) {
        if (c === "\\") {
          text += c + (raw[j + 1] ?? "");
          j++;
        } else if (c === '"') {
          out.push({ line: i + 1, text });
          inString = false;
          text = "";
        } else text += c;
      } else if (c === '"') inString = true;
      else if (c === "/" && raw[j + 1] === "/") break;
    }
  });
  return out;
}

function swiftFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? swiftFiles(join(dir, e.name)) : e.name.endsWith(".swift") ? [join(dir, e.name)] : [],
  );
}

const { ascii, cjk } = developerWords();
for (const word of ["session", "token", "bot", "attachment"]) if (!ascii.includes(word)) throw new Error(`word list lost ${word}`);
for (const word of ["会话", "机器人", "任务", "令牌"]) if (!cjk.includes(word)) throw new Error(`word list lost ${word}`);

function found(text: string): string[] {
  const words = new Set(text.toLowerCase().split(/[^a-z0-9]+/).filter(Boolean));
  return [...ascii.filter((w) => words.has(w) || words.has(`${w}s`)), ...cjk.filter((w) => text.includes(w))];
}

const productName: string = JSON.parse(readFileSync(join(root, "product.json"), "utf8")).name;
const han = /[㐀-鿿]/;
const bad: string[] = [];
for (const file of swiftFiles(ios)) {
  const name = relative(root, file);
  for (const { line, text } of strings(readFileSync(file, "utf8"))) {
    if (text.toLowerCase().includes(productName.toLowerCase())) bad.push(`${name}:${line} types the product name`);
    if (file === catalog) {
      const words = found(text.replace(/\\\([^)]*\)/g, " "));
      if (words.length) bad.push(`${name}:${line} "${text}" uses ${words.join(", ")}`);
    } else if (han.test(text)) bad.push(`${name}:${line} "${text}" belongs in the catalog (Sources/Copy.swift)`);
  }
}
if (bad.length) {
  console.error(`interface text problems:\n${bad.join("\n")}`);
  process.exit(1);
}
console.log("iOS interface text: ok");
