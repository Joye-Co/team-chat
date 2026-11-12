import { afterAll, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

mkdirSync(".tmp/ci", { recursive: true });
const root = mkdtempSync(resolve(".tmp/ci/runner-"));
afterAll(() => rmSync(root, { recursive: true, force: true }));

function pkg(name: string, scripts: Record<string, string>) {
  const dir = join(root, name);
  mkdirSync(dir);
  writeFileSync(join(dir, "package.json"), JSON.stringify({ scripts }));
  return dir;
}
const passing = pkg("passing with spaces", { test: "echo passed-marker" });
const failing = pkg("failing", { test: "exit 7" });
const empty = pkg("empty", {});

function run(script: string, ...dirs: string[]) {
  const result = Bun.spawnSync(["bash", "scripts/ci-run.sh", script, ...dirs], {
    env: { ...process.env, GITHUB_ACTIONS: "true" },
  });
  return { code: result.exitCode, text: result.stdout.toString() + result.stderr.toString() };
}

test("runs packages whose paths contain spaces and skips packages without the script", () => {
  const result = run("test", empty, passing);
  expect(result.code).toBe(0);
  expect(result.text).toContain("passed-marker");
  expect(result.text).toContain("ran 'test' in 1 package(s)");
});

test("a failed package does not hide later test results", () => {
  const result = run("test", failing, passing);
  expect(result.code).not.toBe(0);
  expect(result.text).toContain("passed-marker");
  expect(result.text).toContain("failed in 1 package(s)");
  expect(result.text.match(/::group::/g)?.length).toBe(2);
  expect(result.text.match(/::endgroup::/g)?.length).toBe(2);
});

test("a misspelled script or unmatched directory cannot produce green CI", () => {
  expect(run("typo", passing).code).not.toBe(0);
  expect(run("test", join(root, "missing")).code).not.toBe(0);
  expect(run("test", empty).code).not.toBe(0);
});

test("a broken manifest fails even when another package passes", () => {
  const broken = pkg("broken", {});
  writeFileSync(join(broken, "package.json"), "{");
  const result = run("test", broken, passing);
  expect(result.code).not.toBe(0);
  expect(result.text).toContain("passed-marker");
  expect(result.text).toContain("invalid package.json");
});
