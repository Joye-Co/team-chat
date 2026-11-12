import { expect, test } from "bun:test";
import { checks, evaluate } from "./check-results.mjs";

function results(selected: string[] = []) {
  const outputs = Object.fromEntries(["all", ...Object.values(checks).flat()].map((key) => [key, "false"]));
  for (const key of selected) outputs[key] = "true";
  return {
    changes: { result: "success", outputs },
    ...Object.fromEntries(Object.entries(checks).map(([job, keys]) => [job, {
      result: outputs.all === "true" || keys.some((key) => outputs[key] === "true") ? "success" : "skipped",
    }])),
  } as any;
}

test("documentation-only changes may skip all build jobs", () => {
  expect(evaluate(results()).failures).toEqual([]);
});

for (const filter of new Set(["all", ...Object.values(checks).flat()])) {
  test(`selected ${filter} checks must pass`, () => expect(evaluate(results([filter])).failures).toEqual([]));
}

for (const result of ["failure", "cancelled", "skipped", "missing"]) {
  test(`a required job that is ${result} fails the gate`, () => {
    const needs = results(["server"]);
    if (result === "missing") delete needs.server;
    else needs.server.result = result;
    expect(evaluate(needs).failures).toContain(`server: ${result} (required)`);
  });
  test(`change detection that is ${result} fails the gate`, () => {
    const needs = results();
    needs.changes.result = result;
    expect(evaluate(needs).failures).toContain("change detection did not succeed");
  });
}

test("a failed optional job is not silently forgiven", () => {
  const needs = results();
  needs.server.result = "failure";
  expect(evaluate(needs).failures).toContain("server: failure");
});

test("missing outputs cannot masquerade as documentation-only changes", () => {
  const needs = results();
  delete needs.changes.outputs.server;
  expect(evaluate(needs).failures).toContain("missing or invalid change output: server");
  expect(evaluate({}).failures.length).toBeGreaterThan(0);
});

test("a new job must be included in the gate policy", () => {
  const needs = results();
  needs.new_job = { result: "success" };
  expect(evaluate(needs).failures).toContain("unclassified job: new_job");
});
