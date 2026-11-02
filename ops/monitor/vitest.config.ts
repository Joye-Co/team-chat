import { cloudflareTest } from "@cloudflare/vitest-plugin";
import { defineConfig } from "vitest/config";

const TARGETS = JSON.stringify([
  { name: "production", baseUrl: "https://prod.test", opsGroup: "ops", tokenSecret: "MONITOR_TOKEN_PRODUCTION" },
  { name: "staging", baseUrl: "https://staging.test", opsGroup: "ops", tokenSecret: "MONITOR_TOKEN_STAGING" },
]);

export default defineConfig({
  plugins: [
    cloudflareTest({
      wrangler: { configPath: "./wrangler.jsonc" },
      miniflare: {
        bindings: {
          TARGETS,
          FAIL_THRESHOLD: "2",
          SUMMARY_HOUR_UTC: "1",
          MONITOR_TOKEN_PRODUCTION: "tc_test_production_token_value_000000",
          MONITOR_TOKEN_STAGING: "tc_test_staging_token_value_0000000000",
        },
      },
    }),
  ],
  test: { testTimeout: 20_000, fileParallelism: false },
});
