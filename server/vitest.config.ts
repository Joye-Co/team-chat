import { cloudflareTest, readD1Migrations } from "@cloudflare/vitest-plugin";
import { generateKeyPairSync } from "node:crypto";
import { defineConfig } from "vitest/config";

export default defineConfig(async () => {
  const migrations = await readD1Migrations(new URL("./migrations", import.meta.url).pathname);
  // A throwaway APNs key made for each run; pushes go to the fake gateway at apns.test.
  const apns = generateKeyPairSync("ec", { namedCurve: "P-256" });
  const apnsKey = apns.privateKey.export({ type: "pkcs8", format: "pem" }) as string;
  const apnsPublic = apns.publicKey.export({ type: "spki", format: "pem" }) as string;
  return {
    plugins: [
      cloudflareTest({
        wrangler: { configPath: "./wrangler.jsonc" },
        miniflare: {
          bindings: { ADMIN_SECRET: "test-admin-secret", HOOK_SECRET_KEY: "test-hook-secret-key-0123456789", GITHUB_CLIENT_ID: "test-github-client", AUTOTITLE_FAKE: "[autotitle]", TEST_MIGRATIONS: migrations, APNS_KEY: apnsKey, APNS_TEST_PUBLIC_KEY: apnsPublic, APNS_KEY_ID: "TESTKEY001", APNS_TEAM_ID: "TESTTEAM01", APNS_ORIGIN: "https://apns.test" },
        },
      }),
    ],
    test: {
      setupFiles: ["./test/setup.ts"],
      testTimeout: 20_000,
      fileParallelism: false,
    },
  };
});
