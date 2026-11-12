import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { checks } from "./check-results.mjs";

const workflow = Bun.YAML.parse(readFileSync(".github/workflows/ci.yml", "utf8")) as any;
const filters = Bun.YAML.parse(readFileSync(".github/ci-paths.yml", "utf8")) as Record<string, any[]>;

function selected(path: string) {
  // These filters use only literal paths and * / ** globs (no extglobs or negations), whose
  // semantics are shared by Bun.Glob and paths-filter's picomatch with dot:true.
  const matched = Object.fromEntries(Object.entries(filters).map(([key, patterns]) => [
    key, patterns.flat(Infinity).some((pattern: string) => new Bun.Glob(pattern).match(path)),
  ]));
  return Object.entries(checks).filter(([, keys]) => matched.ci || keys.some((key) => matched[key]))
    .map(([job]) => job).sort();
}

const rust = ["rust", "rust-lint", "windows-daemon"];
const desktop = ["desktop-macos", "rust-lint", "windows-desktop"];
const allRust = [...new Set([...rust, ...desktop])];
const js = ["packages", "server", "ops"];
const cases: [string, string[]][] = [
  ["docs/PRODUCT.md", []],
  ["README.md", []],
  ["apps/daemon/src/run.rs", rust],
  ["apps/update-helper/src/main.rs", rust],
  ["tools/e2e/src/main.rs", rust],
  ["apps/desktop/src/main.rs", desktop],
  ["crates/artifact-view/src/lib.rs", desktop],
  ["third_party/gpui-component/src/lib.rs", desktop],
  ["crates/chat-core/src/lib.rs", allRust],
  ["Cargo.lock", allRust],
  ["Cargo.toml", [...allRust, "ops"]],
  [".cargo/config.toml", allRust],
  ["rust-toolchain.toml", allRust],
  ["protocol/fixtures/event.json", [...allRust, "packages"]],
  ["packages/protocol/src/index.ts", js],
  ["packages/search/src/index.ts", js],
  ["server/src/core/gateway.ts", ["server"]],
  ["server/wrangler.jsonc", ["server", "ops"]],
  ["scripts/sandbox/seed.ts", ["server"]],
  ["ops/monitor/src/index.ts", ["ops"]],
  ["scripts/servers.ts", js],
  ["scripts/check-release-digest.sh", ["ops"]],
  ["docs/releases/1.2.6.md", [...desktop, "ops"]],
  [".github/workflows/release.yml", ["workflows", "ops"]],
  [".github/workflows/release-jobs.yml", ["workflows", "ops"]],
  [".bun-version", js],
  ["bun.lock", js],
  ["product.json", [...js, ...desktop]],
  [".github/workflows/ci.yml", Object.keys(checks)],
  [".github/ci-paths.yml", Object.keys(checks)],
  [".github/actions/setup-bun/action.yml", Object.keys(checks)],
  ["scripts/ci/check-results.mjs", Object.keys(checks)],
  ["scripts/ci-run.sh", Object.keys(checks)],
];
for (const [path, jobs] of cases) {
  test(`checks selected for ${path}`, () => expect(selected(path)).toEqual([...jobs].sort()));
}

test("every job is covered by the result gate and agrees with its selection rules", () => {
  expect(Object.keys(workflow.jobs).sort()).toEqual(["changes", "result", ...Object.keys(checks)].sort());
  expect(workflow.jobs.result.needs.sort()).toEqual(["changes", ...Object.keys(checks)].sort());
  expect(workflow.jobs.result.if).toBe("always()");
  for (const [job, keys] of Object.entries(checks)) {
    const condition = workflow.jobs[job].if as string;
    const actual = [...condition.matchAll(/needs\.changes\.outputs\.(\w+) == 'true'/g)].map((m) => m[1]);
    expect(actual.sort()).toEqual(["all", ...keys].sort());
  }
});

test("manual runs and merge queue candidates run every check", () => {
  expect(workflow.on).toHaveProperty("merge_group");
  for (const event of ["workflow_dispatch", "merge_group"]) {
    expect(workflow.jobs.changes.outputs.all).toContain(`github.event_name == '${event}'`);
    expect(workflow.jobs.changes.steps.find((s: any) => s.id === "filter").if)
      .toContain(`github.event_name != '${event}'`);
  }
});

test("filter syntax stays within the tested glob subset", () => {
  for (const patterns of Object.values(filters)) {
    for (const pattern of patterns.flat(Infinity)) {
      expect(typeof pattern).toBe("string");
      expect(pattern).toMatch(/^[\w./*-]+$/);
    }
  }
});
