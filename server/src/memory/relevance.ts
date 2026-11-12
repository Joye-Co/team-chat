// Relevance of memories to a request (盯 4.5): the same tokens as message search (@chat/search:
// lowercased words, identifier parts, overlapping Chinese two-character words), scored in the
// Worker over the memories the reader may see. The candidates are already filtered by audience,
// so ranking never widens who reads what. D1 holds no FTS table for memories: the allowed set
// is small (at most a few thousand short items) and D1 cannot export a database with a virtual table.
import { tokenizeForIndex } from "@chat/search";

/** Words that say nothing about the subject of a request. */
const STOP = new Set([
  "the", "and", "for", "with", "this", "that", "from", "into", "are", "was", "but", "not", "you", "your",
  "can", "please", "pls", "what", "how", "why", "when", "who", "use", "all", "any", "out", "get", "let",
  "一下", "我们", "你们", "他们", "这个", "那个", "什么", "怎么", "可以", "一个", "帮我", "看看", "一些", "现在", "需要", "没有", "就是", "还是",
]);

/** The distinct search terms of a text: words of 2+ characters and Chinese two-character words. */
export function terms(text: string): Set<string> {
  const out = new Set<string>();
  for (const t of tokenizeForIndex(text).split(" ")) {
    if ([...t].length < 2 || STOP.has(t)) continue;
    out.add(t);
  }
  return out;
}

export interface Scored<T> {
  item: T;
  score: number;
}

/**
 * Each item's relevance to `query`: the sum of the rarity (inverse document frequency among
 * `items`) of the query's terms it contains. 0 when it shares none, or the query has none.
 */
export function score<T>(items: T[], text: (item: T) => string, query: string): Scored<T>[] {
  const q = terms(query);
  if (q.size === 0) return items.map((item) => ({ item, score: 0 }));
  const docs = items.map((item) => {
    const all = terms(text(item));
    return [...q].filter((t) => all.has(t));
  });
  const df = new Map<string, number>();
  for (const d of docs) for (const t of d) df.set(t, (df.get(t) ?? 0) + 1);
  const n = items.length;
  return items.map((item, i) => ({
    item,
    score: (docs[i] as string[]).reduce((s, t) => s + Math.log(1 + (n + 1) / ((df.get(t) ?? 0) + 0.5)), 0),
  }));
}
