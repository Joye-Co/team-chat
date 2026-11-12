import { Database } from "bun:sqlite"
import { beforeEach, describe, expect, test } from "bun:test"
import {
  buildMatchQuery,
  makeSnippet,
  MessageSearchIndex,
  searchStatement,
  tokenizeForIndex,
} from "../src/index.ts"

function openIndex() {
  const db = new Database(":memory:")
  const index = new MessageSearchIndex((sql, params) => db.query(sql).all(...params) as Record<string, unknown>[])
  index.migrate()
  return { db, index }
}

let db: Database
let index: MessageSearchIndex
let seq = 0
function add(id: string, text: string, topicId = "t1") {
  index.upsert({ messageId: id, topicId, sortKey: ++seq, text })
}
function ids(query: string, extra: { topicId?: string; prefixLast?: boolean } = {}) {
  return index
    .search({ query, ...extra })
    .map((r) => r.messageId)
    .sort()
}

beforeEach(() => {
  ;({ db, index } = openIndex())
  seq = 0
})

describe("tokenizeForIndex", () => {
  test("CJK bigrams, single character unigram, tail unigram at the end", () => {
    expect(tokenizeForIndex("部署")).toBe("部署 署")
    expect(tokenizeForIndex("部署上线")).toBe("部署 署上 上线 线")
    expect(tokenizeForIndex("好")).toBe("好")
  })
  test("Latin lowercased, identifier parts at the end", () => {
    expect(tokenizeForIndex("Deploy parseJSONData now")).toBe("deploy parsejsondata now parse json data")
    expect(tokenizeForIndex("max_retry_count")).toBe("max_retry_count max retry count")
    expect(tokenizeForIndex("HTTPServer v2 getV2Api")).toBe("httpserver v2 getv2api http server get v2 api")
  })
  test("punctuation and emoji dropped", () => {
    expect(tokenizeForIndex("🎉🚀!!! ，。？")).toBe("")
    expect(tokenizeForIndex("好！👍ok…")).toBe("好 ok")
    expect(tokenizeForIndex("___ --- ***")).toBe("")
  })
  test("full-width letters are normalized", () => {
    expect(tokenizeForIndex("ＡＢＣ１２３")).toBe("abc123")
  })
  test("mixed Chinese and English", () => {
    expect(tokenizeForIndex("把API部署到prod环境")).toBe("把 api 部署 署到 prod 环境 到 境")
  })
})

describe("buildMatchQuery", () => {
  test("shapes", () => {
    expect(buildMatchQuery("部署")).toBe('"部署"')
    expect(buildMatchQuery("部署上线")).toBe('"部署 署上 上线"')
    expect(buildMatchQuery("部")).toBe('"部"*')
    expect(buildMatchQuery("foo.bar baz")).toBe('"foo bar" AND "baz"')
    expect(buildMatchQuery("deplo", { prefixLast: true })).toBe('"deplo"*')
    expect(buildMatchQuery("   ")).toBeNull()
    expect(buildMatchQuery("🎉 ！")).toBeNull()
  })
  test("term cap", () => {
    const q = buildMatchQuery(Array.from({ length: 100 }, (_, i) => `w${i}`).join(" "))
    expect(q?.split(" AND ").length).toBe(32)
  })
})

describe("search", () => {
  test("Chinese two-character and longer words", () => {
    add("m1", "今天下午部署新版本")
    add("m2", "部门会议改到明天")
    add("m3", "生产环境部署上线了")
    add("m4", "全部完成")
    expect(ids("部署")).toEqual(["m1", "m3"])
    expect(ids("部署上线")).toEqual(["m3"])
    expect(ids("新版本")).toEqual(["m1"])
    expect(ids("会议")).toEqual(["m2"])
    expect(ids("上部")).toEqual([])
    // single character finds it at the start, middle and end of runs
    expect(ids("部")).toEqual(["m1", "m2", "m3", "m4"])
    expect(ids("本")).toEqual(["m1"])
    // AND of words
    expect(ids("部署 版本")).toEqual(["m1"])
  })

  test("mixed Chinese and English", () => {
    add("m1", "把API部署到prod环境")
    add("m2", "API文档更新")
    add("m3", "prod 数据库迁移")
    expect(ids("api")).toEqual(["m1", "m2"])
    expect(ids("API部署")).toEqual(["m1"])
    expect(ids("prod环境")).toEqual(["m1"])
    expect(ids("Prod")).toEqual(["m1", "m3"])
  })

  test("code identifiers", () => {
    add("m1", "call parseJSONData() in utils.ts")
    add("m2", "set max_retry_count = 3")
    add("m3", "the json parser is slow")
    add("m4", "version 3.14 released, see github.com/org/repo")
    expect(ids("parseJSONData")).toEqual(["m1"])
    expect(ids("parsejsondata")).toEqual(["m1"])
    expect(ids("json")).toEqual(["m1", "m3"])
    expect(ids("max_retry_count")).toEqual(["m2"])
    expect(ids("retry")).toEqual(["m2"])
    expect(ids("utils.ts")).toEqual(["m1"])
    expect(ids("3.14")).toEqual(["m4"])
    expect(ids("14.3")).toEqual([])
    expect(ids("github.com/org")).toEqual(["m4"])
    expect(ids("pars", { prefixLast: true })).toEqual(["m1", "m3"])
  })

  test("punctuation and emoji", () => {
    add("m1", "上线啦🎉🎉！！")
    add("m2", "👍")
    expect(ids("上线！")).toEqual(["m1"])
    expect(ids("🎉")).toEqual([])
    expect(ids("“上线”")).toEqual(["m1"])
    // a message with nothing searchable is not indexed
    expect(db.query("SELECT count(*) AS n FROM search_docs").get()).toEqual({ n: 1 })
  })

  test("injection attempts do not break the query", () => {
    add("m1", 'he said "hello" OR NOT world')
    add("m2", "tokens: secret")
    const attacks = [
      '"',
      '""',
      '"hello',
      "hello\"' OR 1=1 --",
      "tokens:secret",
      "NEAR(hello world)",
      "hello OR world",
      "NOT hello",
      "* ^ ( ) : + - { }",
      "'); DROP TABLE search_docs; --",
      "hello*",
      "\u0000hello",
    ]
    for (const q of attacks) expect(() => index.search({ query: q })).not.toThrow()
    expect(ids("tokens:secret")).toEqual(["m2"])
    // OR and NOT are plain words, combined with AND
    expect(ids("hello OR world")).toEqual(["m1"])
    expect(ids("NOT hello")).toEqual(["m1"])
    expect(ids("hello OR nothing")).toEqual([])
    expect(db.query("SELECT count(*) AS n FROM search_docs").get()).toEqual({ n: 2 })
  })

  test("edits and deletes", () => {
    add("m1", "部署失败")
    add("m2", "部署成功")
    expect(ids("部署")).toEqual(["m1", "m2"])
    index.upsert({ messageId: "m1", topicId: "t1", sortKey: 1, text: "回滚完成" })
    expect(ids("部署")).toEqual(["m2"])
    expect(ids("失败")).toEqual([])
    expect(ids("回滚")).toEqual(["m1"])
    index.delete("m2")
    expect(ids("部署")).toEqual([])
    index.delete("m2")
    index.delete("never-indexed")
    // editing to nothing searchable removes the message
    index.upsert({ messageId: "m1", topicId: "t1", sortKey: 1, text: "👍" })
    expect(ids("回滚")).toEqual([])
    const counts = db.query("SELECT (SELECT count(*) FROM search_docs) AS d, (SELECT count(*) FROM search_fts) AS f").get()
    expect(counts).toEqual({ d: 0, f: 0 })
  })

  test("topic filter, delete topic, order and paging", () => {
    add("a1", "发布 one", "ta")
    add("a2", "发布 two", "ta")
    add("b1", "发布 three", "tb")
    expect(ids("发布", { topicId: "ta" })).toEqual(["a1", "a2"])
    expect(index.search({ query: "发布" }).map((r) => r.messageId)).toEqual(["b1", "a2", "a1"])
    expect(index.search({ query: "发布", limit: 1, beforeSortKey: 3 }).map((r) => r.messageId)).toEqual(["a2"])
    expect(index.search({ query: "发布", order: "relevance" })).toHaveLength(3)
    index.deleteTopic("ta")
    expect(ids("发布")).toEqual(["b1"])
  })

  test("statement params are bound, never inlined", () => {
    const s = searchStatement({ query: "x'; DROP", topicId: "t'1", limit: 1e9 })
    expect(s?.sql).not.toContain("DROP")
    expect(s?.params.at(-1)).toBe(200)
  })
})

describe("makeSnippet", () => {
  test("finds and highlights the match", () => {
    const text = `${"前面很长的一段话。".repeat(20)}今天下午部署新版本，请注意。${"后面也很长。".repeat(20)}`
    const s = makeSnippet(text, "部署", { maxLength: 40 })
    expect(s.text.startsWith("…")).toBe(true)
    expect(s.text.endsWith("…")).toBe(true)
    expect(s.highlights).toHaveLength(1)
    const [a, b] = s.highlights[0] as [number, number]
    expect(s.text.slice(a, b)).toBe("部署")
  })
  test("case-insensitive Latin, several terms, merged ranges", () => {
    const s = makeSnippet("Call parseJSONData()\nthen JSON again", "json parsejsondata")
    expect(s.text).toBe("Call parseJSONData() then JSON again")
    expect(s.highlights.map(([a, b]) => s.text.slice(a, b))).toEqual(["parseJSONData", "JSON"])
  })
  test("the start does not cut a word", () => {
    const text = `这是一段比较长的产品说明（草稿）新功能的话题会在这里讨论，请大家看话题列表${"后面还有内容".repeat(10)}`
    const s = makeSnippet(text, "列表", { maxLength: 40, before: 24 })
    // NFKC folds the full-width bracket.
    expect(s.text.startsWith("…草稿)")).toBe(true)
    const [a, b] = s.highlights[0] as [number, number]
    expect(s.text.slice(a, b)).toBe("列表")
    const latin = makeSnippet(`${"word ".repeat(10)} release notes for the parser`, "parser", { maxLength: 40, before: 24 })
    expect(latin.text).toMatch(/^…(word|release) /)
  })
  test("no match returns the start; emoji are not cut", () => {
    expect(makeSnippet("short text", "zzz")).toEqual({ text: "short text", highlights: [] })
    const s = makeSnippet(`${"🎉".repeat(30)}部署`, "部署", { maxLength: 20, before: 5 })
    expect(s.text).not.toContain("�")
    expect(s.text.normalize()).toBe(s.text)
    expect(Array.from(s.text).every((c) => c.length === 1 || c === "🎉")).toBe(true)
  })
})
