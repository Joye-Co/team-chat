// BotDesk and a run's GitHub (design-github-identity 3.4, unit G3). The github module's machine
// routes call these two RPCs:
// - githubWait: the run needs a GitHub its person has not connected (or GitHub refused it). The AI
//   member @-mentions them once in the topic; the run then ends waiting, and its topic waits on
//   them instead of being done.
// - githubResume: they connected. A follow-up of the waiting run goes on from where it stopped,
//   for the same person; when the run has not ended yet, it starts once the run ends. A run that
//   someone already followed up is not resumed again.
import { isMainTopicId } from "@chat/protocol";
import { isGroupMember } from "../../core/db";
import { ulid } from "../../core/ids";
import { GITHUB_TEXT, waitText } from "../../github/text";
import { MAX_FOLLOW_UPS, type RunRow } from "./core";
import { DeskInteract } from "./interact";

export abstract class DeskGithub extends DeskInteract {
  /** The run waits for `memberId` to connect GitHub: says so once in the topic. */
  async githubWait(botId: string, runId: string, memberId: string, reason: string): Promise<void> {
    this.bind(botId);
    await this.serial(runId, async () => {
      const r = this.run(runId);
      if (!r || this.githubWaitingOn(runId) !== null) return;
      this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `github_wait:${runId}`, memberId);
      const name = (await this.names([memberId])).get(memberId) ?? memberId;
      // Mentions must be group members; a fallback admin outside the group still reads the name.
      const mentions = (await isGroupMember(this.env.DB, r.group_id, memberId)) ? [memberId] : [];
      try {
        await this.post(r, `run:${r.id}:github`, waitText(name, reason), { mentions });
      } catch (err) {
        this.log.warn("github wait note failed", { runId, err });
      }
    });
  }

  /** The person the run waited for connected GitHub. Returns whether a follow-up started or will start. */
  async githubResume(botId: string, runId: string, memberId: string): Promise<boolean> {
    this.bind(botId);
    return this.serial(runId, async () => {
      const r = this.run(runId);
      if (!r || this.superseded(r)) return false;
      this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)", `github_resume:${runId}`, memberId);
      // Still reporting: it goes on once it ended (events.ts `end`).
      if (r.ended_at === null) return true;
      return this.startResume(r);
    });
  }

  protected override async resumeAfterGithub(r: RunRow): Promise<void> {
    // Follow-ups the run never took start their own run, which carries on the same work.
    const unread = this.sql.exec<{ n: number }>("SELECT count(*) AS n FROM inputs WHERE run_id = ? AND accepted = 0", r.id).one().n;
    if (unread > 0 || this.superseded(r)) return;
    try {
      await this.startResume(r);
    } catch (err) {
      this.log.error("github resume failed", { runId: r.id, err });
    }
  }

  /**
   * Someone already went on with this work: a later run in its topic (a 话题 is one piece of work),
   * or in a chat area a follow-up or retry of it.
   */
  protected superseded(r: RunRow): boolean {
    const later = isMainTopicId(r.topic_id)
      ? this.sql.exec("SELECT 1 FROM runs WHERE follow_up_of = ? OR retry_of = ? LIMIT 1", r.id, r.id).toArray()
      : this.sql.exec("SELECT 1 FROM runs WHERE topic_id = ? AND id != ? AND created_at >= ? LIMIT 1", r.topic_id, r.id, r.created_at).toArray();
    return later.length > 0;
  }

  /** A follow-up of the ended run, for the run's own person, with the same session. */
  protected async startResume(r: RunRow): Promise<boolean> {
    const who = this.meta(`github_resume:${r.id}`) ?? r.requested_for;
    this.sql.exec("DELETE FROM meta WHERE key = ?", `github_resume:${r.id}`);
    if (this.startBlocked(r.topic_id) || this.followUpDepth(r) >= MAX_FOLLOW_UPS) return false;
    const key = `github:${ulid()}`;
    const name = who ? ((await this.names([who])).get(who) ?? "") : "";
    await this.start(
      {
        groupId: r.group_id,
        topicId: r.topic_id,
        messageId: key,
        authorId: r.requested_by,
        body: GITHUB_TEXT.resume(name || "The person"),
        mentioned: true,
        requestedFor: r.requested_for,
      },
      r,
    );
    const next = this.sql.exec<{ id: string }>("SELECT id FROM runs WHERE trigger_message_id = ?", key).toArray()[0];
    this.log.info("github resume", { runId: r.id, next: next?.id });
    return next !== undefined;
  }
}
