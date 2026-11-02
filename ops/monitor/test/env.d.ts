import type { Env as MonitorEnv } from "../src/config";

declare global {
  namespace Cloudflare {
    interface Env extends MonitorEnv {}
  }
}
