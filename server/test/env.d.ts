import type { Env as AppEnv } from "../src/core/env";

declare global {
  namespace Cloudflare {
    interface Env extends AppEnv {
      TEST_MIGRATIONS: import("cloudflare:test").D1Migration[];
    }
  }
}

/** Vite raw imports (test fixtures as text). */
declare module "*?raw" {
  const text: string;
  export default text;
}
