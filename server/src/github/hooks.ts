// Extension points of the GitHub module that the run side (G3, server/src/bots/desk) fills in.
import type { GithubWait } from "@chat/protocol";
import type { Env } from "../core/env";

/** The runs waiting for this person to connect GitHub (`GET /v1/me/github` `waits`). */
export type WaitsLoader = (env: Env, memberId: string) => Promise<GithubWait[]>;
/** Runs after a person connected GitHub (for example: resume the runs that waited). */
export type ConnectedHook = (env: Env, memberId: string) => Promise<void> | void;

let waitsLoader: WaitsLoader | null = null;
export const connectedHooks: ConnectedHook[] = [];

/** One loader; without it `waits` is empty. */
export function registerGithubWaits(loader: WaitsLoader): void {
  waitsLoader = loader;
}

export function registerGithubConnected(hook: ConnectedHook): void {
  connectedHooks.push(hook);
}

export async function loadWaits(env: Env, memberId: string): Promise<GithubWait[]> {
  return waitsLoader ? (await waitsLoader(env, memberId)).slice(0, 100) : [];
}
