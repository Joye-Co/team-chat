// Member characters: the look of a member's avatar, one of a fixed set of 26 ids.
// See docs/protocol.md, "Entities" (Member.character).
import { z } from "zod";

/** The 26 known character ids. A client without art for an id draws the initial avatar. */
export const CHARACTER_IDS = [
  "jojo",
  "h02",
  "h03",
  "h04",
  "h05",
  "h06",
  "o01",
  "o02",
  "o03",
  "o04",
  "o05",
  "o06",
  "o07",
  "o08",
  "o09",
  "o10",
  "o11",
  "o12",
  "o13",
  "o14",
  "o15",
  "o16",
  "o17",
  "o18",
  "o19",
  "o20",
] as const;

export const CharacterId = z.enum(CHARACTER_IDS);
export type CharacterId = z.infer<typeof CharacterId>;
