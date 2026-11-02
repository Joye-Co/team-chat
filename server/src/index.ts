import "./features";
import type { Env } from "./core/env";
import { handleFetch, handleScheduled } from "./core/worker";

export { GroupRoom } from "./core/room";
export { MemberGateway } from "./core/gateway";
export type { Env } from "./core/env";

export default {
  fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    return handleFetch(request, env, ctx);
  },
  scheduled(controller: ScheduledController, env: Env, ctx: ExecutionContext): Promise<void> {
    return handleScheduled(controller, env, ctx);
  },
} satisfies ExportedHandler<Env>;
