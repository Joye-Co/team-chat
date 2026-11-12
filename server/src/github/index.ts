// GitHub sign-in is off (docs/protocol.md, "GitHub sign-in"; design
// docs/plans/usable/design-github-identity.zh.md 3.4): an AI member uses the GitHub sign-in of the
// computer it runs on, and the server keeps nobody's GitHub. These stubs keep older apps working:
// status says unavailable ("off"), a sign-in is refused, disconnecting has nothing to do. The
// machine routes (`/v1/machine/runs/:id/github...`) are gone. Registered from src/features.ts.
import type { GithubStatusResponse } from "@chat/protocol";
import { ApiError } from "../core/errors";
import { registerRoute } from "../core/registry";

/** `GithubStatusResponse.unavailableReason` now that the server keeps nobody's GitHub. */
export const GITHUB_OFF = "off";

let registered = false;

export function registerGithub(): void {
  if (registered) return;
  registered = true;
  registerRoute({
    method: "GET",
    path: "/v1/me/github",
    auth: "member",
    async handler(): Promise<GithubStatusResponse> {
      return { available: false, unavailableReason: GITHUB_OFF, waits: [] };
    },
  });
  for (const path of ["/v1/me/github/device", "/v1/me/github/device/:id/poll"]) {
    registerRoute({
      method: "POST",
      path,
      auth: "member",
      async handler() {
        throw new ApiError("conflict", "GitHub sign-in is off: AI members use the computer's own sign-in", `github.${GITHUB_OFF}`);
      },
    });
  }
  registerRoute({
    method: "DELETE",
    path: "/v1/me/github",
    auth: "member",
    async handler() {
      return { ok: true };
    },
  });
}

registerGithub();
