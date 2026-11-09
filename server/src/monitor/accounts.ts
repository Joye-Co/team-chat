// Sign in and API keys from the app (feature "accounts"; docs/protocol.md "Machine accounts").
// Owner-only routes that relay to the machine's daemon through MachineLink. A key reaches the
// server only sealed to the machine's public key, so the server stores and logs no value; the
// machine reports names and last 4 characters back with its heartbeat.
import { LoginCodeRequest, SecretName, SetSecretRequest, StartLoginRequest } from "@chat/protocol";
import { linkStub } from "../bots/env";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import { ApiError } from "../core/errors";
import { ulid } from "../core/ids";
import { registerRoute } from "../core/registry";
import type { Env } from "../core/env";

async function machine(env: Env, id: string) {
  const row = await env.DB.prepare("SELECT id FROM machines WHERE id = ? AND revoked_at IS NULL").bind(id).first<{ id: string }>();
  if (!row) throw new ApiError("not_found", "machine not found");
  return linkStub(env, id);
}

function sent(r: "sent" | "offline" | "unsupported"): void {
  if (r === "offline") throw new ApiError("conflict", "the machine is offline", "machine.offline");
  if (r === "unsupported") throw new ApiError("conflict", "the machine's daemon is too old for this; update it to 1.2.7 or later", "machine.too_old");
}

const LOGIN_ID = /^[A-Za-z0-9_-]{1,64}$/;

registerRoute({
  method: "POST",
  path: "/v1/machines/:id/logins",
  auth: "owner",
  async handler({ env, params, body, auth }) {
    const req = await body(StartLoginRequest);
    const link = await machine(env, params.id);
    const loginId = ulid(Date.now());
    sent(await link.accountsFrame({ t: "account.login", loginId, runtime: req.runtime, op: "start" }));
    await env.DB.batch([
      auditStatement(env.DB, auth as AuthContext, { action: "machine.login.start", target: params.id, details: { runtime: req.runtime } }),
    ]);
    return { loginId };
  },
});

registerRoute({
  method: "GET",
  path: "/v1/machines/:id/logins/:loginId",
  auth: "owner",
  async handler({ env, params }) {
    if (!LOGIN_ID.test(params.loginId)) throw new ApiError("not_found", "sign-in not found");
    const login = await (await machine(env, params.id)).login(params.loginId);
    if (!login) throw new ApiError("not_found", "sign-in not found");
    return login;
  },
});

registerRoute({
  method: "POST",
  path: "/v1/machines/:id/logins/:loginId/code",
  auth: "owner",
  async handler({ env, params, body }) {
    const req = await body(LoginCodeRequest);
    if (!LOGIN_ID.test(params.loginId)) throw new ApiError("not_found", "sign-in not found");
    const link = await machine(env, params.id);
    const login = await link.login(params.loginId);
    if (!login) throw new ApiError("not_found", "sign-in not found");
    // The code is a one-time sign-in code, relayed and never stored or logged.
    sent(await link.accountsFrame({ t: "account.login", loginId: params.loginId, runtime: login.runtime, op: "code", code: req.code }));
    return { ok: true };
  },
});

registerRoute({
  method: "DELETE",
  path: "/v1/machines/:id/logins/:loginId",
  auth: "owner",
  async handler({ env, params }) {
    if (!LOGIN_ID.test(params.loginId)) throw new ApiError("not_found", "sign-in not found");
    const link = await machine(env, params.id);
    const login = await link.login(params.loginId);
    if (!login) throw new ApiError("not_found", "sign-in not found");
    sent(await link.accountsFrame({ t: "account.login", loginId: params.loginId, runtime: login.runtime, op: "cancel" }));
    return { ok: true };
  },
});

registerRoute({
  method: "GET",
  path: "/v1/machines/:id/secrets",
  auth: "owner",
  async handler({ env, params }) {
    return (await machine(env, params.id)).secrets();
  },
});

registerRoute({
  method: "PUT",
  path: "/v1/machines/:id/secrets/:name",
  auth: "owner",
  async handler({ env, params, body, auth }) {
    if (!SecretName.safeParse(params.name).success) throw new ApiError("invalid", "a key's name is an environment variable name (A-Z, 0-9, _)");
    const req = await body(SetSecretRequest);
    const link = await machine(env, params.id);
    sent(await link.accountsFrame({ t: "secret.set", name: params.name, sealed: req.sealed, members: req.members }));
    await env.DB.batch([
      auditStatement(env.DB, auth as AuthContext, {
        action: "machine.secret.set",
        target: params.id,
        details: { name: params.name, members: req.members },
      }),
    ]);
    return { ok: true };
  },
});

registerRoute({
  method: "DELETE",
  path: "/v1/machines/:id/secrets/:name",
  auth: "owner",
  async handler({ env, params, auth }) {
    if (!SecretName.safeParse(params.name).success) throw new ApiError("not_found", "key not found");
    const link = await machine(env, params.id);
    sent(await link.accountsFrame({ t: "secret.delete", name: params.name }));
    await env.DB.batch([
      auditStatement(env.DB, auth as AuthContext, { action: "machine.secret.delete", target: params.id, details: { name: params.name } }),
    ]);
    return { ok: true };
  },
});
