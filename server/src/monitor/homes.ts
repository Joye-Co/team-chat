// AI members' home folders on a VM host (usable, feature "homes"; docs/protocol.md "Machine
// homes"). The daemon reports each home's size and use with its heartbeat (MachineMetrics.homes,
// shown in the monitor); whoever manages the machine can empty one, relayed as `home.clear`.

import { linkStub } from "../bots/env";
import { requireMachineManager } from "../bots/store";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import { ApiError } from "../core/errors";
import { registerRoute } from "../core/registry";

/** Member ids the daemon takes as folder names (ULIDs). */
const MEMBER_ID = /^[A-Za-z0-9]{1,64}$/;

registerRoute({
  method: "POST",
  path: "/v1/machines/:id/homes/:memberId/clear",
  auth: "member",
  async handler({ env, params, auth }) {
    if (!MEMBER_ID.test(params.memberId)) throw new ApiError("not_found", "member not found");
    await requireMachineManager(env.DB, auth as AuthContext, params.id);
    const r = await linkStub(env, params.id).clearHome(params.memberId);
    if (r === "offline") throw new ApiError("conflict", "the machine is offline", "machine.offline");
    if (r === "unsupported") throw new ApiError("conflict", "this machine keeps no home folders, or its daemon is too old", "machine.too_old");
    await env.DB.batch([
      auditStatement(env.DB, auth as AuthContext, { action: "machine.home.clear", target: params.id, details: { memberId: params.memberId } }),
    ]);
    return { ok: true };
  },
});
