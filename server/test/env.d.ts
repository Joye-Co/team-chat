import type { Env as AppEnv } from "../src/core/env";

declare global {
  namespace Cloudflare {
    interface Env extends AppEnv {
      TEST_MIGRATIONS: import("cloudflare:test").D1Migration[];
      APNS_TEST_PUBLIC_KEY: string;
    }
  }
}
