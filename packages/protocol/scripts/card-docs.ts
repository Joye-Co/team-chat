// The descriptions of protocol/schema/CardV2.schema.json and PatchOp.schema.json: what a card
// author (an AI member that reads `chat-run card schema`) must know and cannot read from a field
// name. gen-schema.ts puts each one on its field and fails when a block type has no entry or an
// entry names a field the schema lacks, so this file cannot fall behind src/card2.ts. A path is
// a field name, `a.b` for a field of an object, and goes through arrays by itself
// (`options.recommended` is the field of each option).

/** Fields of the card itself. */
export const CARD_DOCS: Record<string, string> = {
  catalog: 'Set "tc.card/2.1" when the card has a 2.1 block (rank, rating, scale, confirm, imageChoice, matrix, slots, poll, buttons, diagram, embed, stack, row, compare, tabs, questionnaire). Absent = tc.card/2.0.',
  rev: "1 on post (the server sets it). Every patch raises it; a patch names the rev it changes.",
  intent: "ask: people must answer. info: something to read (numbers, a comparison), no status band. working: work people follow while it runs. done: a result.",
  status: "The band of a working card: executing or failed. Absent: the band follows intent.",
  summary: "One plain sentence under the title.",
  fallback: "The whole card as plain text, for notifications, search and old clients.",
  ask: "Who is asked. Without it nobody is asked and the topic does not wait, whatever the intent.",
  "ask.id": 'The ask round, default "a1". A new id opens a new round of answers.',
  "ask.from": 'Who may answer: member ids, "requester" (the person your run works for) or "owners".',
  "ask.blocking": "The topic waits for the answer. Default: true when intent is ask.",
  "ask.expiresAt": "Milliseconds since 1970; after it the ask takes no answer.",
  "ask.promise": "What you promise until the answer, shown with the question.",
  "ask.record": "true: the answer is kept as a decision of the group.",
  "ask.irreversible": "For what cannot be undone: the card names the action, its target and the consequence before the answer is sent.",
  "ask.irreversible.typeToConfirm": "The answerer types this text before the submit action is enabled.",
  blocks: "The content, top to bottom. Containers (stack, row, tabs, compare, questionnaire, group with children) name other blocks of this list by id.",
  actions: "The buttons under the card, at most 10 and at most one with style primary.",
  "actions.kind": "submit (default), reject (the answer no) and cancel are answers; submit and reject carry the values of `inputs`. link opens `target`. press is a live event, never an answer.",
  "actions.target": "Where a link action goes; only link actions have one.",
  "actions.confirm": "The client asks this before it sends the answer.",
  "actions.inputs": "submit and reject: the ids of the input blocks whose values go with the answer.",
  "actions.role": '"approve" marks the action that approves a reviewed change (a diff, a pull request).',
  data: "Your own state for this card, at most 16 KiB; people never see it.",
};

/** Fields every block has. */
export const BASE_DOCS: Record<string, string> = {
  id: "Unique in the card: 1..64 of A-Za-z0-9_.:- (not root, not starting with $).",
  emphasis: "high: at most 2 blocks of a card. low: shown quieter.",
  events: "The live events this block sends you. A block sends only the events listed here; each block type has its own names (`chat-run --help`, Interaction).",
  deliver: "Input blocks. immediate: one tap is the answer (single choice, confirm, compare, rating, scale, single-line input). onSubmit: the value goes with a submit action.",
  emitBy: "Who may send this block's live events: ask (who is asked), requester, members, owners or {memberIds}. Default: who is asked, for the blocks an open question is answered with; else every member.",
  fallbackText: "Plain text for clients that do not know this block type. Required on rank, rating, scale, confirm, imageChoice, matrix, slots, poll, buttons, diagram, embed, compare and questionnaire.",
};

/** One entry per block type: `is` says what the block is for, `fields` what its fields mean. */
export const BLOCK_DOCS: Record<string, { is: string; fields?: Record<string, string> }> = {
  text: { is: "A paragraph.", fields: { text: "Markdown." } },
  fields: { is: "Label and value pairs.", fields: { "fields.value": "Plain text." } },
  table: {
    is: "A table; with `select` it is an input.",
    fields: {
      columns: "The header of each column.",
      rows: "One list of strings per row, one cell per column. Numbers are written as strings.",
      rowKeys: "One stable key per row, used by table.rowSelect and by quotes; default the row index.",
      select: "Makes the table an input: the answer is the chosen row key or keys.",
      sortable: "People may sort by a column; you get table.sort only when `events` lists it.",
    },
  },
  list: { is: "A list of lines, each with an optional state.", fields: { "items.status": "todo, in_progress, done, failed or skipped." } },
  progress: { is: "A progress bar.", fields: { value: "From 0 to 1 (0.4 is 40 %)." } },
  code: { is: "A code block.", fields: { lang: "The language, for highlighting." } },
  image: { is: "A picture of the group's files.", fields: { fileId: "The id `chat-run file upload` printed." } },
  divider: { is: "A line between sections." },
  link: { is: "A link to a web page, a topic or a 产出." },
  decision: {
    is: "A choice: one option, or several with `multiple`.",
    fields: {
      label: "The question.",
      "options.detail": "A sentence under the option's label.",
      "options.recommended": "true on the option you would pick.",
      default: "The option ids chosen at first.",
      allowOther: "Adds a choice with free text.",
    },
  },
  input: {
    is: "One value from a person: text, a number, a date, a switch, a choice.",
    fields: {
      kind: "text, textarea, number, select, date, switch, chips or slider.",
      options: "The choices of select and chips.",
      default: "The value shown at first.",
    },
  },
  draft: { is: "A text people may edit before they submit it.", fields: { text: "Markdown.", editable: "false shows the text without editing." } },
  plan: {
    is: "A list of steps with their state.",
    fields: {
      items: "The steps, in order: {text, state}. state is done, current, waiting (for a person) or pending. Without items the block shows nothing.",
      runId: "Shows that run's live plan instead of `items`.",
    },
  },
  metric: {
    is: "One number with its label and its change.",
    fields: { value: "A number; the unit goes in `unit`.", delta: "The change against the last value, in the same unit.", better: "Which direction of `delta` is good: up or down." },
  },
  chart: {
    is: "A line, bar or area chart, or a sparkline.",
    fields: {
      kind: "line, bar, area or spark.",
      x: "The label of each point on the x axis.",
      series: "1 to 5 series; each `values` has one number per label of `x`.",
      summary: "One sentence that says what the chart shows.",
    },
  },
  diff: { is: "A change to review.", fields: { text: "A unified diff.", path: "The file it changes." } },
  evidence: {
    is: "One line of proof: what was checked and what shows it.",
    fields: {
      state: "pass, fail or info. On a done card a pass without fileId, url, artifact or recording is shown as your claim, not as passed.",
      fileId: "A file that shows it (a screenshot, a log).",
      url: "A web page that shows it.",
      artifact: "The 产出 version that shows it: {artifactId, version}.",
      tests: "Test counts. On a done card they come from the run's log, not from you.",
      recording: "A screen recording of the group's files.",
    },
  },
  ref: {
    is: "A reference people can open: a pull request, a 产出, a topic, a member or a web page.",
    fields: { target: "{kind: pr, url}, {kind: artifact, artifactId, version?}, {kind: topic, groupId, topicId}, {kind: member, memberId} or {kind: url, url}." },
  },
  rank: { is: "People put items in order.", fields: { rankTop: "Only the first n items are the answer.", cut: "A line after the first `after` items." } },
  rating: { is: "A rating from 1 to 5.", fields: { max: "Always 5.", labels: 'Words for some values, keys "1" to "5".' } },
  scale: { is: "A number on a scale from `min` to `max`.", fields: { ends: "The words at the two ends." } },
  confirm: { is: "A yes or no question; one tap is the answer.", fields: { yes: "The label of yes.", no: "The label of no." } },
  imageChoice: { is: "A choice between 2 to 4 pictures.", fields: { "options.fileId": "The id `chat-run file upload` printed.", "options.recommended": "true on the option you would pick." } },
  matrix: { is: "One choice per row from the same columns." },
  slots: { is: "A choice of time slots.", fields: { tz: "The IANA time zone the slots are shown in.", "slots.start": "Milliseconds since 1970.", "slots.end": "Milliseconds since 1970." } },
  poll: {
    is: "A vote of the group.",
    fields: { voters: '"group" or a list of member ids.', closesAt: "Milliseconds since 1970.", public: "Who voted for what is shown.", changeable: "A voter may change the vote until it closes." },
  },
  buttons: { is: "1 to 3 inline buttons. They send button.press (list it in `events`) and never answer an ask.", fields: { "buttons.confirm": "The client asks this before it sends the press." } },
  diagram: {
    is: "A diagram drawn from Mermaid source.",
    fields: {
      syntax: 'Always "mermaid".',
      source: "Mermaid that starts with flowchart, graph, sequenceDiagram, stateDiagram, stateDiagram-v2, classDiagram, erDiagram, gantt or timeline. No click, href, callback, call, %%{ directives, front matter or HTML.",
      alt: "What the diagram shows, for readers who cannot see it.",
    },
  },
  embed: {
    is: "A 产出 page shown live inside the card.",
    fields: { artifactId: "A 产出 of the same group.", version: "The version shown.", height: "160 to 480 pt." },
  },
  group: {
    is: "A titled section that can fold.",
    fields: { children: "The ids of its blocks (2.1). Give either `children` or `blocks`.", blocks: "Its blocks inline, leaves only (2.0).", summary: "Shown while it is folded." },
  },
  stack: { is: "Blocks one under the other.", fields: { children: "The ids of its blocks." } },
  row: { is: "2 or 3 blocks side by side.", fields: { children: "The ids of its blocks.", weights: "Relative widths 1 to 3, one per child." } },
  compare: {
    is: "2 to 4 options side by side; picking one is the answer.",
    fields: { "options.children": "The ids of the blocks shown in the option.", "options.recommended": "true on the option you would pick.", note: "A text note that goes with the pick." },
  },
  tabs: { is: "2 to 5 tabs, each with its blocks.", fields: { "tabs.children": "The ids of the tab's blocks.", selected: "The tab shown first." } },
  questionnaire: {
    is: "2 to 5 questions answered together with one button.",
    fields: { children: "The ids of its input blocks.", anonymous: "You get the answers as totals only." },
  },
};

/** One entry per patch op. */
export const OP_DOCS: Record<string, string> = {
  set: "Merges `props` into the block `id`. It cannot change id, type, children, options or tabs.",
  replace: "Puts `block` in the place of the block `id`. The new block has the same id; its type may differ.",
  insert: "Adds `block`: at the end, or at one of `after` (a block id), `before` (a block id) or `index`. `parent` is the id of a container, a compare option or a tab.",
  remove: "Removes the block `id` and its children.",
  data: 'Sets `value` at the JSON Pointer `path` of the card\'s `data` ("" is the whole of it); without `value` it deletes.',
  card: "Changes the card itself: title, summary, intent, status, fallback, ask, actions. `status: null` clears the band and `ask: null` closes the question.",
};
