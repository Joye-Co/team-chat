// Reaction fixtures. Called by gen-fixtures.ts.
type Obj = Record<string, unknown>;
export type ReactionsCtx = {
  ok: (path: string, kind: string, value: unknown, description?: string) => void;
  bad: (path: string, kind: string, value: unknown, description: string) => void;
  ID: Record<string, string>;
  base: Obj;
  message: Obj;
};

export function addReactionsFixtures({ ok, bad, ID, base, message }: ReactionsCtx) {
  const react = { t: "react", rid: "r60", groupId: ID.general, messageId: ID.msg, emoji: "👍", on: true };
  ok("client/react.json", "ClientFrame", react);
  ok("client/react-off.json", "ClientFrame", { ...react, rid: "r61", emoji: "👍🏽", on: false }, "remove, skin tone modifier");
  ok("client/react-zwj.json", "ClientFrame", { ...react, rid: "r62", emoji: "👩‍👩‍👧‍👦" }, "a joined emoji is one reaction");
  bad("client/react-empty-emoji.json", "ClientFrame", { ...react, emoji: "" }, "empty emoji");
  bad("client/react-emoji-too-long.json", "ClientFrame", { ...react, emoji: "👍".repeat(17) }, "more than 16 code points");
  bad("client/react-emoji-space.json", "ClientFrame", { ...react, emoji: "👍 👍" }, "whitespace in emoji");
  bad("client/react-missing-on.json", "ClientFrame", { t: "react", rid: "r63", groupId: ID.general, messageId: ID.msg, emoji: "👍" }, "missing on");

  ok("events/reaction.changed.json", "Event", { ...base, actorId: ID.alice, type: "reaction.changed", data: { messageId: ID.msg, emoji: "✅", on: true } });
  ok("events/reaction.changed-off.json", "Event", { ...base, actorId: ID.alice, type: "reaction.changed", data: { messageId: ID.msg, emoji: "✅", on: false } }, "removed");
  bad("events/reaction.changed-missing-emoji.json", "Event", { ...base, type: "reaction.changed", data: { messageId: ID.msg, on: true } }, "missing emoji");

  ok("entities/message-reactions.json", "Message", {
    ...message,
    reactions: [
      { emoji: "👍", memberIds: [ID.owner, ID.alice] },
      { emoji: "👀", memberIds: [ID.bot] },
    ],
  });
}
