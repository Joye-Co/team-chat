import { appendFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

// Keep these in step with the job conditions. config.test.ts checks the actual workflow.
export const checks = {
  workflows: ["workflows"],
  packages: ["packages"],
  server: ["server"],
  "rust-lint": ["core", "daemon", "desktop"],
  rust: ["core", "daemon"],
  "desktop-macos": ["core", "desktop"],
  "windows-desktop": ["core", "desktop"],
  "windows-daemon": ["core", "daemon"],
  ops: ["ops"],
};

export function evaluate(needs) {
  const failures = [];
  const rows = [];
  const outputs = needs.changes?.outputs ?? {};
  if (needs.changes?.result !== "success") failures.push("change detection did not succeed");
  for (const key of new Set(["all", ...Object.values(checks).flat()])) {
    if (!["true", "false"].includes(outputs[key])) failures.push(`missing or invalid change output: ${key}`);
  }
  for (const [job, filters] of Object.entries(checks)) {
    const required = outputs.all === "true" || filters.some((key) => outputs[key] === "true");
    const result = needs[job]?.result ?? "missing";
    rows.push(`| ${job} | ${required ? "required" : "not selected"} | ${result} |`);
    if (result !== "success" && !(result === "skipped" && !required)) {
      failures.push(`${job}: ${result}${required ? " (required)" : ""}`);
    }
  }
  // A newly added job must be classified instead of silently bypassing the gate.
  for (const job of Object.keys(needs)) {
    if (job !== "changes" && !Object.hasOwn(checks, job)) failures.push(`unclassified job: ${job}`);
  }
  return {
    failures,
    summary: ["## CI results", "", "| Job | Selection | Result |", "| --- | --- | --- |", ...rows, ""].join("\n"),
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const { failures, summary } = evaluate(JSON.parse(process.env.CI_NEEDS ?? "{}"));
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
  console.log(summary);
  for (const failure of failures) console.error(`::error::${failure}`);
  process.exitCode = failures.length ? 1 : 0;
}
