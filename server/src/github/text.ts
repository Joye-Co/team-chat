// What an AI member says in the topic when its run found no GitHub sign-in on the computer
// (design-github-identity 3.4). Interface text: Simplified Chinese with the product nouns only.

export const GITHUB_TEXT = {
  /** A team computer with no sign-in: the admins were told. */
  teamMissing: "这台电脑还没有登录 GitHub，推不了代码。已通知管理员，登录好后再 @我。",
  /** A team computer whose sign-in GitHub refused. */
  teamInvalid: "这台电脑的 GitHub 登录失效了。已通知管理员，重新登录后再 @我。",
  /** A personal computer: its owner signs in there. */
  personalMissing: (owner: string) => `这台电脑还没有登录 GitHub，推不了代码。请电脑主人 ${owner} 在电脑上登录后再 @我。`,
};

/** The topic message for a run's `github` event: `owner` is the personal computer's owner's name, null on a team computer. */
export function missingText(state: string, owner: string | null): string {
  if (owner !== null) return GITHUB_TEXT.personalMissing(owner);
  return state === "invalid" ? GITHUB_TEXT.teamInvalid : GITHUB_TEXT.teamMissing;
}
