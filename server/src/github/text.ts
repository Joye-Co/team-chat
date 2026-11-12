// What an AI member says in the topic when a run needs a GitHub the person has not connected
// (design-github-identity 3.4), and what the CLI hears when it is resumed. The topic messages are
// interface text: Simplified Chinese with the product nouns only.

export const GITHUB_TEXT = {
  /** The run needs the person's GitHub and they have not connected it. */
  notConnected: (name: string) => `@${name} 这一步要用你的 GitHub。连接 GitHub 后我会自己接着做。`,
  /** GitHub refused the person's saved token. */
  invalid: (name: string) => `@${name} 你的 GitHub 连接失效了，重新连接后我会自己接着做。`,
  /** A private card's answerer stays unnamed: the same asks, to nobody in particular. */
  notConnectedUnnamed: "这一步要用 GitHub。连接 GitHub 后我会自己接着做。",
  invalidUnnamed: "GitHub 连接失效了，重新连接后我会自己接着做。",
  /** The follow-up a connection resumed (to the CLI). */
  resume: (name: string) => `[team-chat] ${name} has connected GitHub. Continue from where you stopped.`,
};

/** The topic message for a wait reason ("invalid", else not connected); `name` null: nobody named. */
export function waitText(name: string | null, reason: string): string {
  if (name === null) return reason === "invalid" ? GITHUB_TEXT.invalidUnnamed : GITHUB_TEXT.notConnectedUnnamed;
  return reason === "invalid" ? GITHUB_TEXT.invalid(name) : GITHUB_TEXT.notConnected(name);
}
