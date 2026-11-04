// The output contract of a run (docs/protocol.md, "Bot runs"): when a run succeeds the bot posts
// one Card v2 with intent "done" and its proof: the result, pull requests it names, the test
// counts it reports, its 验收标准 and the 产出 versions it made. Pure rules here; the group room
// side (`bots.runProof`) reads what the room knows about the run.
import { type AcceptanceItem, type Card, CardV2, type CardV2Block, cardV2ToV1 } from "@chat/protocol";
import { ApiError } from "../core/errors";
import { registerRoomRpc } from "../core/registry";
import { firstLine } from "./dispatch";

/** Product words only (PRODUCT.md). */
export const PROOF_TEXT = {
  doneTitle: "做完了",
  tests: (passed: number, failed: number) => (failed > 0 ? `测试：${passed} 个通过，${failed} 个没通过` : `测试：${passed} 个通过`),
  artifact: (title: string, version: number) => cutUnits(`${title}（第 ${version} 版）`, 200),
} as const;

/** The emoji a bot puts on the message that started a run, instead of a text acknowledgement. */
export const ACK_EMOJI = "👀";

/** Longest result text in the card's text block (v1 text blocks hold 8000 UTF-16 units). */
const CARD_TEXT_UNITS = 7000;
const MAX_PRS = 5;
const MAX_ARTIFACTS = 10;

/** Cuts text to at most `max` UTF-16 units (Zod's string length) without splitting a character. */
export function cutUnits(text: string, max: number): string {
  if (text.length <= max) return text;
  let out = "";
  for (const ch of text) {
    if (out.length + ch.length > max - 1) break;
    out += ch;
  }
  return `${out}…`;
}

export interface PrRef {
  url: string;
  repo: string;
  number: number;
  state?: string;
}

export interface ArtifactProof {
  artifactId: string;
  version: number;
  title: string;
}

export interface TestCounts {
  passed: number;
  failed: number;
  skipped?: number;
}

/** GitHub pull request links in `text`, first seen first, at most 5. */
export function prRefs(text: string): PrRef[] {
  const out: PrRef[] = [];
  const seen = new Set<string>();
  for (const m of text.matchAll(/https:\/\/github\.com\/([A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+)\/pull\/(\d{1,9})\b/g)) {
    const url = `https://github.com/${m[1]}/pull/${m[2]}`;
    if (seen.has(url)) continue;
    seen.add(url);
    out.push({ url, repo: m[1], number: Number(m[2]) });
    if (out.length === MAX_PRS) break;
  }
  return out;
}

/**
 * Test counts a result reports, summed over every summary line it has: `12 passed`, `3 failed`
 * (cargo, vitest, jest, pytest, bun) and `12 pass` / `0 fail` (bun). Null when it names none.
 */
export function testCounts(text: string): TestCounts | null {
  let passed = 0;
  let failed = 0;
  let skipped = 0;
  let found = false;
  for (const line of text.split(/\r?\n/)) {
    const p = line.match(/\b(\d{1,7})\s+(?:tests?\s+)?(?:passed|pass(?:ing)?)\b/i);
    const f = line.match(/\b(\d{1,7})\s+(?:tests?\s+)?(?:failed|fail(?:ing|ures?)?)\b/i);
    const k = line.match(/\b(\d{1,7})\s+(?:tests?\s+)?(?:skipped|skip|ignored|pending|todo)\b/i);
    if (!p && !f) continue;
    found = true;
    if (p) passed += Number(p[1]);
    if (f) failed += Number(f[1]);
    if (k) skipped += Number(k[1]);
  }
  if (!found) return null;
  return skipped > 0 ? { passed, failed, skipped } : { passed, failed };
}

/** The done card of a run, or null when it would not be a valid Card v2. */
export function doneCard(opts: {
  runId: string;
  result: string;
  prs: PrRef[];
  artifacts: ArtifactProof[];
  acceptance: AcceptanceItem[];
}): CardV2 | null {
  const blocks: CardV2Block[] = [];
  const text = opts.result.trim();
  if (text !== "") blocks.push({ type: "text", id: "result", text: cutUnits(text, CARD_TEXT_UNITS) });
  const tests = testCounts(text);
  if (tests) {
    blocks.push({
      type: "evidence",
      id: "tests",
      label: PROOF_TEXT.tests(tests.passed, tests.failed),
      state: tests.failed > 0 ? "fail" : "pass",
      // Readers take the counts from here, not from the label.
      tests,
    });
  }
  opts.acceptance.slice(0, 20).forEach((a, i) => {
    const b: Extract<CardV2Block, { type: "evidence" }> = {
      type: "evidence",
      id: `acceptance-${i + 1}`,
      label: cutUnits(a.text, 200),
      state: a.state === "pending" ? "info" : a.state,
    };
    if (a.evidence?.text) b.detail = cutUnits(a.evidence.text, 1000);
    if (a.evidence?.fileId) b.fileId = a.evidence.fileId;
    if (a.evidence?.url) b.url = a.evidence.url;
    if (a.evidence?.artifact) b.artifact = a.evidence.artifact;
    blocks.push(b);
  });
  opts.prs.slice(0, MAX_PRS).forEach((pr, i) => {
    blocks.push({
      type: "ref",
      id: `pr-${i + 1}`,
      label: cutUnits(`${pr.repo}#${pr.number}`, 200),
      target: { kind: "pr", url: pr.url, repo: pr.repo, number: pr.number, ...(pr.state ? { state: cutUnits(pr.state, 32) } : {}) },
    });
  });
  opts.artifacts.slice(0, MAX_ARTIFACTS).forEach((a, i) => {
    blocks.push({
      type: "ref",
      id: `artifact-${i + 1}`,
      label: PROOF_TEXT.artifact(a.title, a.version),
      target: { kind: "artifact", artifactId: a.artifactId, version: a.version },
    });
  });
  const card = {
    v: 2 as const,
    rev: 1,
    intent: "done" as const,
    title: cutUnits(firstLine(text, 200), 200) || PROOF_TEXT.doneTitle,
    fallback: cutUnits(text, 4000),
    blocks,
    data: { runId: opts.runId },
  };
  const parsed = CardV2.safeParse(card);
  return parsed.success ? parsed.data : null;
}

/**
 * The v1 card 1.0.x clients get for a done card: the derived one without the fallback text
 * and the `result` block. The message body holds the result, and 1.0.x draws it above the card.
 */
export function doneCardV1(card: CardV2): Card {
  return cardV2ToV1({ ...card, fallback: "", blocks: card.blocks.filter((b) => b.id !== "result") });
}

/** What `bots.runProof` returns. */
export interface RunProof {
  prs: PrRef[];
  artifacts: ArtifactProof[];
}

/**
 * Proof the room holds for a run of the calling bot in a topic: 产出 versions made by the run
 * (their `runId`), or by the bot in the topic since the run started; and the topic's pull
 * request links (`topic.linked`).
 */
registerRoomRpc("bots.runProof", (room, actor, args): RunProof => {
  const { topicId, runId, since } = args as { topicId: string; runId: string; since: number };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const artifacts: ArtifactProof[] = [];
  try {
    const rows = room.sql
      .exec<{ artifact_id: string; version: number; title: string }>(
        `SELECT v.artifact_id, v.version, a.title FROM artifact_versions v JOIN artifacts a ON a.id = v.artifact_id
         WHERE a.topic_id = ? AND (v.run_id = ? OR (v.run_id IS NULL AND v.created_by = ? AND v.created_at >= ?))
         ORDER BY v.created_at, v.version LIMIT 50`,
        topicId,
        runId,
        actor.id,
        since,
      )
      .toArray();
    // The newest version of each 产出.
    const newest = new Map<string, ArtifactProof>();
    for (const r of rows) newest.set(r.artifact_id, { artifactId: r.artifact_id, version: r.version, title: r.title });
    artifacts.push(...newest.values());
  } catch {
    // A room without 产出 tables (never written to) has no proof.
  }
  const prs: PrRef[] = [];
  const links = room.sql
    .exec<{ data: string }>("SELECT data FROM events WHERE topic_id = ? AND type = 'topic.linked' ORDER BY seq DESC LIMIT 50", topicId)
    .toArray();
  const seen = new Set<string>();
  for (const l of links) {
    const link = (JSON.parse(l.data) as { link?: { kind?: string; url?: string; repo?: string; number?: number; state?: string } }).link;
    if (link?.kind !== "pr" || !link.url || !link.repo || !link.number || seen.has(link.url)) continue;
    seen.add(link.url);
    prs.push({ url: link.url, repo: link.repo, number: link.number, ...(link.state ? { state: link.state } : {}) });
  }
  return { prs: prs.slice(0, MAX_PRS), artifacts: artifacts.slice(0, MAX_ARTIFACTS) };
});

/** Pull requests of the topic first (they carry a state), then the ones the result names. */
export function mergePrs(fromTopic: PrRef[], fromText: PrRef[]): PrRef[] {
  const out = [...fromTopic];
  for (const p of fromText) if (!out.some((x) => x.url === p.url)) out.push(p);
  return out.slice(0, MAX_PRS);
}
