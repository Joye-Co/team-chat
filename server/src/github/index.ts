// Each person's GitHub connection (docs/protocol.md, "GitHub sign-in"; design
// docs/plans/usable/design-github-identity.zh.md). Registered from src/features.ts.
//
// G2 (this module's start): the device code sign-in, `/v1/me/github` status and disconnect,
// encrypted storage and the member-deactivated cleanup. The run side (G3) adds its machine routes
// to `machineRoutes` below, fills `waits` with `registerGithubWaits` and resumes waiting runs
// with `registerGithubConnected` (both in ./hooks).
import { type RouteDef, registerMemberDeactivated, registerRoute } from "../core/registry";
import { registerGithubRoutes } from "./routes";
import { forgetMember } from "./store";

export { registerGithubConnected, registerGithubWaits } from "./hooks";
export { loadToken, markInvalid, unavailableReason } from "./store";

/**
 * Machine-token routes of the GitHub module (`/v1/machine/runs/:id/github...`). G3 lists its
 * route definitions in this array (imported from its own file into this one), so they are
 * registered with the rest below.
 */
export const machineRoutes: RouteDef[] = [];

let registered = false;

export function registerGithub(): void {
  if (registered) return;
  registered = true;
  registerGithubRoutes();
  for (const r of machineRoutes) registerRoute(r);
  registerMemberDeactivated(forgetMember);
}

registerGithub();
