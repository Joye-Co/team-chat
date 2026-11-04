// TypeScript SDK for external bots (Bun and Node 22+). See docs/bots.md.
export {
  ACK_EMOJI,
  ANONYMOUS_ACTOR,
  Bot,
  DEFAULT_WRITE_RATE,
  SDK_VERSION,
  type AnswerValue,
  type ArtifactEventContext,
  type AsideContext,
  type BotOptions,
  type CardActionContext,
  type CardAnswerContext,
  type CardEventContext,
  type DoneResult,
  type EditInput,
  type EventContext,
  type MessageContext,
  type ResolvedRef,
  type SendInput,
  type Target,
  type TopicChange,
  type WebSocketCtor,
  withoutAnonymousRequester,
} from "./bot.ts";
export {
  type ApprovalCardOptions,
  type AskCardOptions,
  type AskOptions,
  answeredCard,
  approvalCard,
  askCard,
  CARD_IDS,
  type ChoiceOption,
  checkCardV2,
  type DecisionCardOptions,
  type DoneCardOptions,
  type DraftCardOptions,
  decisionCard,
  doneCard,
  draftCard,
  type Evidence,
  failedV1Card,
  type FormCardOptions,
  type FormInput,
  formCard,
} from "./cards.ts";
export {
  blocks,
  type Child,
  type ComposeCardOptions,
  checkDiagram,
  choice,
  composeCard,
  diagramLabels,
  flattenBlocks,
  mermaid,
  parts,
} from "./blocks.ts";
export { CardDraft, opsFit, type PatchInput } from "./interact.ts";
export { DELETED_REF_LABEL, findPart, type MarkdownBlock, markdownBlocks, refPrefix, stripRefPrefix } from "./refs.ts";
export { ChatError, type ChatErrorCode, isRetryable } from "./errors.ts";
export { checkArtifactEvents, MAX_PATTERN_INPUT, patternProblem, schemaPatternProblem } from "./pattern.ts";
export { HttpClient, type HttpClientOptions, type PostMessageInput, type UploadInput, sha256Hex } from "./http.ts";
export { consoleLogger, type Logger, type LogLevel, redact } from "./log.ts";
export { type AsideRef, type BotStateData, FileStateStore, MemoryStateStore, type StateStore } from "./state.ts";
export { StreamWriter, type StreamOptions } from "./stream.ts";
export type * from "@chat/protocol";
