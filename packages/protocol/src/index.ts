// Wire protocol version 1. The contract is docs/protocol.md; these schemas are the
// executable source of truth. Objects strip unknown fields on read.
import { z } from "zod";

import {
  Body,
  ClientMsgId,
  Count,
  eventBase,
  Id,
  Limit,
  MAX_BODY_CHARS,
  MAX_FRAME_BYTES,
  MAX_NAME_CHARS,
  MAX_TITLE_CHARS,
  MemberLevel,
  Millis,
  Name,
  PROTOCOL_VERSION,
  Rid,
  Seq,
  Title,
} from "./common.ts";
import { CardActionedEvent, CardActionFrame } from "./card.ts";
import {
  CardAnsweredEvent,
  CardAnswerFrame,
  CardAnswerRetractFrame,
  CardEventAck,
  CardEventFrame,
  CardEventTransient,
  CardPatchFrame,
  CardRev,
  CardRetractedEvent,
  CardState,
  CardV2,
  DecisionRecordedEvent,
  DecisionsResponse,
  GroupDecision,
  RecordDecisionRequest,
  RecordDecisionResponse,
  RecordedDecision,
  InteractionResolvedTransient,
  InteractionResolveFrame,
  PartRef,
} from "./card2.ts";
import { ArtifactPreviewReadyEvent, MediaInfo } from "./media.ts";
import { DeskNotedEvent, DeskNotesResponse, PutDeskNoteRequest, PutDeskNoteResponse } from "./desk.ts";
import { TopicLink, TopicLinkedEvent } from "./links.ts";
import { CharacterId } from "./characters.ts";
import {
  AddArtifactVersionRequest,
  ArtifactCreatedEvent,
  ArtifactResponse,
  ArtifactsResponse,
  ArtifactVersionAddedEvent,
  ArtifactVersionResponse,
  CreateFileRequest,
  CreateFileResponse,
  CreateArtifactRequest,
  MachineCreateArtifactRequest,
  FileMeta,
  FileResponse,
  Artifact,
  ArtifactVersion,
  ArtifactEventFrame,
  ArtifactEventAck,
  ArtifactDataFrame,
  ArtifactDataAck,
  ArtifactReplyFrame,
  ArtifactEventTransient,
  ArtifactDataTransient,
  ArtifactReplyTransient,
  ArtifactDataResponse,
  PutArtifactDataRequest,
  PutArtifactMediaRequest,
  PutArtifactMediaResponse,
  UploadPartResponse,
  ArtifactEventsResponse,
} from "./files.ts";
import { CreateHookRequest, CreateHookResponse, Hook, HookInRequest, HooksResponse } from "./hooks.ts";
import { BlockRef, BlockRefInput, editFields, messageFields, messageUpdatedFields, sendFields, FileRef } from "./message.ts";
import { Card } from "./card.ts";
import { eventFrameFields, groupSummaryPrefFields, MAX_WEEKLY_BUDGET_CENTS, UpdateGroupPrefsRequest, UpdateGroupRequest, UpdateMemberRequest } from "./members.ts";
import { groupSummaryReadFields, readFields, ReadsFrame, TopicState } from "./reads.ts";
import { messageReactionFields, ReactFrame, ReactionChangedEvent } from "./reactions.ts";
import { SearchFrame, SearchResultsFrame } from "./search.ts";
import { messageAsideFields, sendContextFields, TopicContext, TopicFollowFrame, TopicMuteFrame, topicFields, topicUpdatedFields, topicUpdateFields } from "./topic.ts";
import { UpdateInfo } from "./updates.ts";
import { DiagnosticsResponse } from "./diagnostics.ts";
import { AuditResponse, AuditEntry } from "./audit.ts";
import {
  BotRun,
  BotRunner,
  CreateMachineRequest,
  CreateMachineResponse,
  Machine,
  MachinesResponse,
  RunEventsResponse,
  RunContext,
  RunContextManifest,
  ContextMessage,
  ContextHit,
  TopicHistoryResponse,
  TopicMessageResponse,
  ContextSearchResponse,
  RunArtifactsResponse,
  RunArtifactResponse,
  RunCardResponse,
  RunDecisionsResponse,
  RunMembersResponse,
  TopicStateResponse,
  WorkBrief,
  FrontDeskDecision,
  FrontDeskDecisionsResponse,
  BotMetrics,
  RunUpdatedEvent,
  SetBotRunnerRequest,
  StartRunRequest,
  SetRunPlanRequest,
  EndRunRequest,
  SetRunAcceptanceRequest,
  ResumeRunRequest,
} from "./bots.ts";
import { RunPlan } from "./plan.ts";
import { MachineDownFrame, MachineUpFrame, RunEvent, RunLogEntry } from "./machine.ts";
import { ActivityResponse } from "./activity.ts";
import { TeamUsage } from "./usage.ts";
import { TeamRoster } from "./roster.ts";
import { LoginCodeRequest, MachineLogin, MachineMonitor, MachineSecretsResponse, SetSecretRequest, StartLoginRequest } from "./monitor.ts";
import {
  AiServiceConfig,
  WorkerModel,
  WorkerModelsResponse,
  SetWorkerModelRequest,
  AiServiceTestResult,
  ApprovalPolicy,
  Device,
  DevicesResponse,
  DigestTransient,
  EmailCodeRequest,
  EmailCodeResponse,
  EmailSignInRequest,
  MyEmailResponse,
  VerifyEmailRequest,
  eventFrameSettingsFields,
  NotifySettings,
  TestAiServiceRequest,
  UpdateAiServiceRequest,
  UpdateApprovalPolicyRequest,
  UpdateNotifySettingsRequest,
} from "./settings.ts";
import {
  AnswerCardRequest,
  BadgeResponse,
  PushData,
  PushDevice,
  PushDevicesResponse,
  RegisterPushDeviceRequest,
  RemovePushDeviceResponse,
} from "./push.ts";
import {
  CreateMemoryRequest,
  MachineMemoryNoteRequest,
  MemoriesResponse,
  MemoryItem,
  MemoryUpdatesResponse,
  MemoryVersionsResponse,
  UpdateMemoryRequest,
} from "./memory.ts";

import {
  MyPasswordResponse,
  PasswordSignInRequest,
  QrClaimRequest,
  QrCodeRequest,
  QrOfferResponse,
  QrOfferStatus,
  QrPeekResponse,
  QrPollRequest,
  QrRequestRequest,
  QrRequestResponse,
  RemovePasswordRequest,
  SetPasswordRequest,
} from "./account.ts";

export * from "./common.ts";
export * from "./account.ts";
export * from "./characters.ts";
export * from "./topic.ts";
export * from "./reads.ts";
export * from "./card.ts";
export * from "./card2.ts";
export * from "./pattern.ts";
export * from "./diagram.ts";
export * from "./media.ts";
export * from "./desk.ts";
export * from "./memory.ts";
export * from "./links.ts";
export * from "./message.ts";
export * from "./files.ts";
export * from "./members.ts";
export * from "./search.ts";
export * from "./reactions.ts";
export * from "./updates.ts";
export * from "./diagnostics.ts";
export * from "./hooks.ts";
export * from "./audit.ts";
export * from "./machine.ts";
export * from "./monitor.ts";
export * from "./activity.ts";
export * from "./usage.ts";
export * from "./roster.ts";
export * from "./bots.ts";
export * from "./plan.ts";
export * from "./settings.ts";
export * from "./push.ts";

// ---------------------------------------------------------------------------
// Entities
// ---------------------------------------------------------------------------

export const MemberKind = z.enum(["human", "bot"]);
export type MemberKind = z.infer<typeof MemberKind>;


export const Member = z.object({
  id: Id,
  kind: MemberKind,
  name: Name,
  role: z.string().nullable().optional(),
  level: MemberLevel,
  avatarUrl: z.string().nullable().optional(),
  deactivated: z.boolean().optional(),
  /** Character id; absent or null = no character (initial avatar). */
  character: CharacterId.nullable().optional(),
  /** 1.1: who the member reports to (汇报给), set by an owner; absent = nobody. */
  reportsTo: Id.optional(),
  /** 1.1: the member's weekly spending limit in US cents (限额), set by an owner; absent = none. */
  weeklyBudgetCents: z.int().nonnegative().max(MAX_WEEKLY_BUDGET_CENTS).optional(),
});
export type Member = z.infer<typeof Member>;

export const GroupKind = z.enum(["group", "dm"]);
export type GroupKind = z.infer<typeof GroupKind>;

export const Group = z.object({
  id: Id,
  kind: GroupKind,
  name: z.string(),
  memberIds: z.array(Id),
  createdBy: Id,
  createdAt: Millis,
  /** 1.1: the purpose line under the group's name (absent = none). */
  purpose: z.string().min(1).max(200).optional(),
});
export type Group = z.infer<typeof Group>;

export const GroupSummary = z.object({
  group: Group,
  lastSeq: Seq,
  readSeq: Seq,
  unread: z.int().nonnegative(),
  mentions: z.int().nonnegative(),
  ...groupSummaryReadFields,
  ...groupSummaryPrefFields,
});
export type GroupSummary = z.infer<typeof GroupSummary>;

export const TopicStatus = z.enum([
  "open",
  "in_progress",
  "waiting_answer",
  "waiting_review",
  "blocked",
  "done",
]);
export type TopicStatus = z.infer<typeof TopicStatus>;

export const Topic = z.object({
  id: Id,
  groupId: Id,
  title: z.string().max(MAX_TITLE_CHARS),
  status: TopicStatus,
  createdBy: Id,
  createdAt: Millis,
  rootMessageId: Id,
  lastSeq: Seq,
  messageCount: z.int().nonnegative(),
  ...topicFields,
});
export type Topic = z.infer<typeof Topic>;

export const Message = z.object({
  id: Id,
  clientMsgId: ClientMsgId,
  groupId: Id,
  topicId: Id,
  authorId: Id,
  body: Body,
  mentions: z.array(Id),
  createdSeq: Seq,
  createdAt: Millis,
  editedAt: Millis.nullable().optional(),
  deletedAt: Millis.nullable().optional(),
  streaming: z.boolean().optional(),
  ...messageFields,
  ...messageReactionFields,
  ...messageAsideFields,
});
export type Message = z.infer<typeof Message>;

export const ErrorCode = z.enum([
  "unauthorized",
  "forbidden",
  "not_found",
  "invalid",
  "conflict",
  "rate_limited",
  "too_large",
  "unsupported_protocol",
  "internal",
]);
export type ErrorCode = z.infer<typeof ErrorCode>;

export const ErrorBody = z.object({
  code: ErrorCode,
  message: z.string(),
  /** 1.1: the specific reason, e.g. `card.already_answered` (no new `ErrorCode` values). */
  reason: z.string().min(1).max(64).optional(),
  /** 1.1, `rate_limited`: wait at least this long before the next try. */
  retryAfterMs: z.int().nonnegative().optional(),
});
export type ErrorBody = z.infer<typeof ErrorBody>;

// ---------------------------------------------------------------------------
// Group event log
// ---------------------------------------------------------------------------


export const TopicCreatedEvent = z.object({
  ...eventBase,
  type: z.literal("topic.created"),
  data: z.object({ topic: Topic }),
});
export const TopicUpdatedEvent = z.object({
  ...eventBase,
  type: z.literal("topic.updated"),
  data: z.object({
    topicId: Id,
    title: Title.optional(),
    status: TopicStatus.optional(),
    ...topicUpdatedFields,
  }),
});
export const MessageCreatedEvent = z.object({
  ...eventBase,
  type: z.literal("message.created"),
  data: z.object({ message: Message }),
});
export const MessageUpdatedEvent = z.object({
  ...eventBase,
  type: z.literal("message.updated"),
  data: z.object({
    messageId: Id,
    body: Body,
    editedAt: Millis,
    streaming: z.boolean(),
    ...messageUpdatedFields,
  }),
});
export const MessageDeletedEvent = z.object({
  ...eventBase,
  type: z.literal("message.deleted"),
  data: z.object({ messageId: Id, deletedAt: Millis }),
});
export const GroupUpdatedEvent = z.object({
  ...eventBase,
  type: z.literal("group.updated"),
  /** `name` is the group's name after the change; 1.1 `purpose` is set when it changed ("" = cleared). */
  data: z.object({ name: z.string(), purpose: z.string().max(200).optional() }),
});
export const GroupMemberAddedEvent = z.object({
  ...eventBase,
  type: z.literal("group.memberAdded"),
  data: z.object({ memberId: Id }),
});
export const GroupMemberRemovedEvent = z.object({
  ...eventBase,
  type: z.literal("group.memberRemoved"),
  data: z.object({ memberId: Id }),
});

export const Event = z.discriminatedUnion("type", [
  TopicCreatedEvent,
  TopicUpdatedEvent,
  MessageCreatedEvent,
  MessageUpdatedEvent,
  MessageDeletedEvent,
  GroupUpdatedEvent,
  GroupMemberAddedEvent,
  GroupMemberRemovedEvent,
  CardActionedEvent,
  ArtifactCreatedEvent,
  ArtifactVersionAddedEvent,
  RunUpdatedEvent,
  ReactionChangedEvent,
  // 1.1
  CardAnsweredEvent,
  TopicLinkedEvent,
  CardRetractedEvent,
  ArtifactPreviewReadyEvent,
  DecisionRecordedEvent,
  DeskNotedEvent,
]);
export type Event = z.infer<typeof Event>;
export type EventType = Event["type"];
/** The event variant with the given `type`. */
export type EventOf<T extends EventType> = Extract<Event, { type: T }>;

export const TypingEvent = z.object({
  type: z.literal("typing"),
  groupId: Id,
  topicId: Id,
  memberId: Id,
  at: Millis,
});
export const PresenceEvent = z.object({
  type: z.literal("presence"),
  memberId: Id,
  online: z.boolean(),
  at: Millis,
});
export const MessageDeltaEvent = z.object({
  type: z.literal("message.delta"),
  groupId: Id,
  topicId: Id,
  messageId: Id,
  append: z.string(),
});

export const TransientEvent = z.discriminatedUnion("type", [
  TypingEvent,
  PresenceEvent,
  MessageDeltaEvent,
  // 1.1: only to connections whose hello has feature "interact/1".
  CardEventTransient,
  InteractionResolvedTransient,
  ArtifactEventTransient,
  ArtifactDataTransient,
  ArtifactReplyTransient,
  // settings: only to connections whose hello has feature "settings/1".
  DigestTransient,
]);
export type TransientEvent = z.infer<typeof TransientEvent>;

// ---------------------------------------------------------------------------
// WebSocket: client to server
// ---------------------------------------------------------------------------

export const HelloFrame = z.object({
  t: z.literal("hello"),
  // Any positive integer parses; the server answers unsupported_protocol for != 1.
  protocol: z.int().positive(),
  client: z.object({ name: z.string(), version: z.string() }),
  cursors: z.record(z.string(), Seq).optional(),
  /** 1.1: what this client handles (`CLIENT_FEATURES`); new transients go only to these. */
  features: z.array(z.string().min(1).max(32)).max(32).optional(),
  /** 1.1: Card v2 catalogs this client renders, e.g. ["tc.card/2.0", "tc.card/2.1"]. */
  supportedCatalogs: z.array(z.string().min(1).max(32)).max(16).optional(),
});
/** `hello.features` of a 1.1 client: the interaction loop and refs to parts. */
export const CLIENT_FEATURES = ["interact/1", "refs/1"] as const;
/** `welcome.features` of a 1.1 server; clients hide affordances the server does not list. */
export const SERVER_FEATURES = ["interact/1", "refs/1", "diagram/1"] as const;
export const SendFrame = z.object({
  t: z.literal("send"),
  rid: Rid,
  groupId: Id,
  topicId: Id.optional(),
  title: Title.optional(),
  clientMsgId: ClientMsgId,
  body: Body,
  mentions: z.array(Id).optional(),
  streaming: z.boolean().optional(),
  ...sendFields,
  ...sendContextFields,
});
export const EditFrame = z.object({
  t: z.literal("edit"),
  rid: Rid,
  groupId: Id,
  messageId: Id,
  body: Body,
  streaming: z.boolean().optional(),
  ...editFields,
});
export const DeltaFrame = z.object({
  t: z.literal("delta"),
  groupId: Id,
  messageId: Id,
  append: z.string().max(MAX_BODY_CHARS),
});
export const DeleteFrame = z.object({
  t: z.literal("delete"),
  rid: Rid,
  groupId: Id,
  messageId: Id,
});
export const TopicUpdateFrame = z.object({
  t: z.literal("topic.update"),
  rid: Rid,
  groupId: Id,
  topicId: Id,
  title: Title.optional(),
  status: TopicStatus.optional(),
  ...topicUpdateFields,
});
export const TypingFrame = z.object({
  t: z.literal("typing"),
  groupId: Id,
  topicId: Id,
});
export const ReadFrame = z.object({
  t: z.literal("read"),
  groupId: Id,
  seq: Seq,
  ...readFields,
});
export const FetchFrame = z.object({
  t: z.literal("fetch"),
  rid: Rid,
  groupId: Id,
  afterSeq: Seq,
  limit: Limit.optional(),
});
export const HistoryRequestFrame = z.object({
  t: z.literal("history"),
  rid: Rid,
  groupId: Id,
  topicId: Id,
  beforeSeq: Seq.optional(),
  limit: Limit.optional(),
});
export const PingFrame = z.object({ t: z.literal("ping") });

export const ClientFrame = z.discriminatedUnion("t", [
  HelloFrame,
  SendFrame,
  EditFrame,
  DeltaFrame,
  DeleteFrame,
  TopicUpdateFrame,
  TypingFrame,
  ReadFrame,
  FetchFrame,
  HistoryRequestFrame,
  PingFrame,
  TopicFollowFrame,
  CardActionFrame,
  SearchFrame,
  ReactFrame,
  // 1.1
  CardAnswerFrame,
  ArtifactEventFrame,
  CardEventFrame,
  CardAnswerRetractFrame,
  CardPatchFrame,
  InteractionResolveFrame,
  ArtifactDataFrame,
  ArtifactReplyFrame,
  TopicMuteFrame,
]);
export type ClientFrame = z.infer<typeof ClientFrame>;
export type ClientFrameType = ClientFrame["t"];
export type ClientFrameOf<T extends ClientFrameType> = Extract<ClientFrame, { t: T }>;

/** `ack.result` of a `send` frame. */
export const SendResult = z.object({
  message: Message,
  topic: Topic.optional(),
  duplicate: z.boolean().optional(),
});
export type SendResult = z.infer<typeof SendResult>;

/** ack.result of `card.patch`: the message at its new rev. */
export const CardPatchAck = z.object({ message: Message, rev: CardRev, duplicate: z.boolean().optional() });
export type CardPatchAck = z.infer<typeof CardPatchAck>;

// ---------------------------------------------------------------------------
// WebSocket: server to client
// ---------------------------------------------------------------------------

export const WelcomeFrame = z.object({
  t: z.literal("welcome"),
  protocol: z.literal(PROTOCOL_VERSION),
  me: Member,
  members: z.array(Member),
  groups: z.array(GroupSummary),
  serverTime: Millis,
  /** 1.1: what this server supports (`SERVER_FEATURES`). */
  features: z.array(z.string().min(1).max(32)).max(32).optional(),
});
export const EventFrame = z.object({ t: z.literal("event"), event: Event, ...eventFrameFields, ...eventFrameSettingsFields });
export const TransientFrame = z.object({
  t: z.literal("transient"),
  event: TransientEvent,
});
export const AckOkFrame = z.object({
  t: z.literal("ack"),
  rid: Rid,
  ok: z.literal(true),
  result: z.unknown().optional(),
});
export const AckErrorFrame = z.object({
  t: z.literal("ack"),
  rid: Rid,
  ok: z.literal(false),
  error: ErrorBody,
});
export const AckFrame = z.discriminatedUnion("ok", [AckOkFrame, AckErrorFrame]);
export type AckFrame = z.infer<typeof AckFrame>;
export const PageFrame = z.object({
  t: z.literal("page"),
  rid: Rid,
  groupId: Id,
  events: z.array(Event),
  hasMore: z.boolean(),
  lastSeq: Seq,
});
export const HistoryFrame = z.object({
  t: z.literal("history"),
  rid: Rid,
  groupId: Id,
  topicId: Id,
  messages: z.array(Message),
  hasMore: z.boolean(),
});
export const DirectoryFrame = z.object({
  t: z.literal("directory"),
  members: z.array(Member).optional(),
  groups: z.array(GroupSummary).optional(),
  removedGroupIds: z.array(Id).optional(),
});
export const ErrorFrame = z.object({ t: z.literal("error"), error: ErrorBody });
export const PongFrame = z.object({ t: z.literal("pong") });

export const ServerFrame = z.discriminatedUnion("t", [
  WelcomeFrame,
  EventFrame,
  TransientFrame,
  AckFrame,
  PageFrame,
  HistoryFrame,
  DirectoryFrame,
  ErrorFrame,
  PongFrame,
  ReadsFrame,
  SearchResultsFrame,
]);
export type ServerFrame = z.infer<typeof ServerFrame>;
export type ServerFrameType = ServerFrame["t"];
export type ServerFrameOf<T extends ServerFrameType> = Extract<ServerFrame, { t: T }>;

// ---------------------------------------------------------------------------
// HTTP API bodies
// ---------------------------------------------------------------------------

export const ErrorResponse = z.object({ error: ErrorBody });
export type ErrorResponse = z.infer<typeof ErrorResponse>;

export const BootstrapRequest = z.object({ ownerName: Name });
export type BootstrapRequest = z.infer<typeof BootstrapRequest>;
export const BootstrapResponse = z.object({
  token: z.string(),
  member: Member,
  group: Group,
});
export type BootstrapResponse = z.infer<typeof BootstrapResponse>;

export const RedeemRequest = z.object({
  inviteCode: z.string().min(1),
  deviceName: z.string().min(1).max(MAX_NAME_CHARS),
});
export type RedeemRequest = z.infer<typeof RedeemRequest>;
export const RedeemResponse = z.object({ token: z.string(), member: Member });
export type RedeemResponse = z.infer<typeof RedeemResponse>;

/** `POST /v1/auth/qr/poll`: `approved` carries the new token once; then the request is gone. */
export const QrPollResponse = z.object({
  state: z.enum(["waiting", "approved", "expired"]),
  token: z.string().optional(),
  member: Member.optional(),
});
export type QrPollResponse = z.infer<typeof QrPollResponse>;

/** `GET /v1/me` */
export const MeResponse = Member;
export type MeResponse = Member;
/** `GET /v1/members` */
export const MembersResponse = z.array(Member);
export type MembersResponse = z.infer<typeof MembersResponse>;

export const CreateInviteRequest = z.object({
  name: Name,
  kind: MemberKind,
  level: MemberLevel,
  role: z.string().optional(),
  /** Sign-in code for this existing member: redeeming it adds a token, not a member. */
  memberId: Id.optional(),
  /** How long the code works, 1-168 hours (default 168, a week). */
  expiresInHours: z.int().min(1).max(168).optional(),
});
export type CreateInviteRequest = z.infer<typeof CreateInviteRequest>;
export const CreateInviteResponse = z.object({ code: z.string(), expiresAt: Millis });
export type CreateInviteResponse = z.infer<typeof CreateInviteResponse>;

export const CreateBotRequest = z.object({ name: Name, role: z.string().optional(), character: CharacterId.optional() });
export type CreateBotRequest = z.infer<typeof CreateBotRequest>;
export const CreateBotResponse = z.object({ member: Member, token: z.string() });
export type CreateBotResponse = z.infer<typeof CreateBotResponse>;

export const RevokeTokensRequest = z.object({
  memberId: Id.optional(),
  all: z.boolean().optional(),
});
export type RevokeTokensRequest = z.infer<typeof RevokeTokensRequest>;
export const RevokeTokensResponse = z.object({ revoked: z.int().nonnegative() });
export type RevokeTokensResponse = z.infer<typeof RevokeTokensResponse>;

/** `GET /v1/groups` */
export const GroupsResponse = z.array(GroupSummary);
export type GroupsResponse = z.infer<typeof GroupsResponse>;

export const CreateGroupRequest = z.object({ name: Name, memberIds: z.array(Id) });
export type CreateGroupRequest = z.infer<typeof CreateGroupRequest>;
export const AddGroupMemberRequest = z.object({ memberId: Id });
export type AddGroupMemberRequest = z.infer<typeof AddGroupMemberRequest>;
export const CreateDmRequest = z.object({ memberId: Id });
export type CreateDmRequest = z.infer<typeof CreateDmRequest>;
/** Response of `POST /v1/groups`, `POST /v1/groups/:id/members` and `POST /v1/dms`. */
export const GroupSummaryResponse = GroupSummary;

/** `DELETE /v1/groups/:id/members/:memberId` */
export const OkResponse = z.object({ ok: z.literal(true) });
export type OkResponse = z.infer<typeof OkResponse>;
export const HealthResponse = z.object({ ok: z.literal(true), version: z.string() });
export type HealthResponse = z.infer<typeof HealthResponse>;

/** `POST /v1/groups/:id/messages`: the `send` frame without `t`, `rid` and `groupId`. Response: `SendResult`. */
export const PostMessageRequest = SendFrame.omit({ t: true, rid: true, groupId: true });
export type PostMessageRequest = z.infer<typeof PostMessageRequest>;

/** `PATCH /v1/members/:id` and `PUT /v1/members/:id/avatar` */
export const MemberResponse = Member;

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/** Every named schema. Fixture `kind` values are keys of this map. */
export const schemas = {
  Member,
  Group,
  GroupSummary,
  Topic,
  Message,
  ErrorBody,
  Event,
  TransientEvent,
  ClientFrame,
  ServerFrame,
  SendResult,
  ErrorResponse,
  BootstrapRequest,
  BootstrapResponse,
  RedeemRequest,
  RedeemResponse,
  // account: password and code sign-in
  MyPasswordResponse,
  SetPasswordRequest,
  RemovePasswordRequest,
  PasswordSignInRequest,
  QrOfferResponse,
  QrOfferStatus,
  QrClaimRequest,
  QrRequestRequest,
  QrRequestResponse,
  QrPollRequest,
  QrPollResponse,
  QrCodeRequest,
  QrPeekResponse,
  MeResponse,
  MembersResponse,
  CreateInviteRequest,
  CreateInviteResponse,
  CreateBotRequest,
  CreateBotResponse,
  RevokeTokensRequest,
  RevokeTokensResponse,
  GroupsResponse,
  CreateGroupRequest,
  AddGroupMemberRequest,
  CreateDmRequest,
  GroupSummaryResponse,
  OkResponse,
  HealthResponse,
  // V1 extensions
  TopicState,
  Card,
  FileRef,
  FileMeta,
  CreateFileRequest,
  CreateFileResponse,
  FileResponse,
  UploadPartResponse,
  ArtifactEventsResponse,
  PutDeskNoteRequest,
  PutDeskNoteResponse,
  DeskNotesResponse,
  Artifact,
  ArtifactVersion,
  CreateArtifactRequest,
  AddArtifactVersionRequest,
  MachineCreateArtifactRequest,
  ArtifactVersionResponse,
  ArtifactResponse,
  ArtifactsResponse,
  UpdateMemberRequest,
  MemberResponse,
  UpdateGroupPrefsRequest,
  UpdateGroupRequest,
  UpdateInfo,
  DiagnosticsResponse,
  Hook,
  CreateHookRequest,
  CreateHookResponse,
  HooksResponse,
  HookInRequest,
  PostMessageRequest,
  AuditEntry,
  AuditResponse,
  // Bot runs and machines
  BotRun,
  RunEvent,
  RunLogEntry,
  RunEventsResponse,
  RunContext,
  TopicHistoryResponse,
  TopicMessageResponse,
  ContextSearchResponse,
  RunArtifactsResponse,
  RunArtifactResponse,
  RunCardResponse,
  RunDecisionsResponse,
  RunMembersResponse,
  TopicStateResponse,
  FrontDeskDecision,
  FrontDeskDecisionsResponse,
  BotMetrics,
  SetBotRunnerRequest,
  BotRunner,
  RunPlan,
  StartRunRequest,
  SetRunPlanRequest,
  EndRunRequest,
  Machine,
  CreateMachineRequest,
  CreateMachineResponse,
  MachinesResponse,
  MachineMonitor,
  ActivityResponse,
  TeamUsage,
  TeamRoster,
  StartLoginRequest,
  LoginCodeRequest,
  MachineLogin,
  MachineSecretsResponse,
  SetSecretRequest,
  MachineUpFrame,
  MachineDownFrame,
  // 1.1
  CardV2,
  TopicLink,
  TopicContext,
  GroupDecision,
  DecisionsResponse,
  RecordedDecision,
  RecordDecisionRequest,
  RecordDecisionResponse,
  SetRunAcceptanceRequest,
  ResumeRunRequest,
  // 1.1 interaction loop, refs, media
  CardState,
  PartRef,
  BlockRefInput,
  BlockRef,
  MediaInfo,
  CardEventAck,
  CardPatchAck,
  ArtifactEventAck,
  ArtifactDataAck,
  ArtifactDataResponse,
  PutArtifactDataRequest,
  // settings
  NotifySettings,
  UpdateNotifySettingsRequest,
  ApprovalPolicy,
  UpdateApprovalPolicyRequest,
  AiServiceConfig,
  WorkerModel,
  WorkerModelsResponse,
  SetWorkerModelRequest,
  UpdateAiServiceRequest,
  TestAiServiceRequest,
  AiServiceTestResult,
  Device,
  DevicesResponse,
  EmailCodeRequest,
  EmailCodeResponse,
  EmailSignInRequest,
  VerifyEmailRequest,
  MyEmailResponse,
  PutArtifactMediaRequest,
  PutArtifactMediaResponse,
  // memory (沉淀)
  MemoryItem,
  CreateMemoryRequest,
  UpdateMemoryRequest,
  MemoriesResponse,
  MemoryVersionsResponse,
  MemoryUpdatesResponse,
  MachineMemoryNoteRequest,
  // push
  RegisterPushDeviceRequest,
  PushDevice,
  PushDevicesResponse,
  RemovePushDeviceResponse,
  BadgeResponse,
  PushData,
  AnswerCardRequest,
} as const;
export type SchemaName = keyof typeof schemas;

/** Parses one JSON text as a client frame. Checks the frame size first. */
export function parseClientFrame(
  text: string,
): { ok: true; frame: ClientFrame } | { ok: false; code: ErrorCode; message: string } {
  return parseFrame(ClientFrame, text);
}

/** Parses one JSON text as a server frame. Checks the frame size first. */
export function parseServerFrame(
  text: string,
): { ok: true; frame: ServerFrame } | { ok: false; code: ErrorCode; message: string } {
  return parseFrame(ServerFrame, text);
}

function parseFrame<T>(
  schema: z.ZodType<T>,
  text: string,
): { ok: true; frame: T } | { ok: false; code: ErrorCode; message: string } {
  // UTF-8 byte length is at least the UTF-16 length; only encode when it could exceed.
  if (text.length > MAX_FRAME_BYTES || (text.length * 3 > MAX_FRAME_BYTES &&
    new TextEncoder().encode(text).byteLength > MAX_FRAME_BYTES)) {
    return { ok: false, code: "too_large", message: "frame exceeds 1 MiB" };
  }
  let json: unknown;
  try {
    json = JSON.parse(text);
  } catch {
    return { ok: false, code: "invalid", message: "frame is not valid JSON" };
  }
  const result = schema.safeParse(json);
  if (!result.success) {
    return { ok: false, code: "invalid", message: z.prettifyError(result.error) };
  }
  return { ok: true, frame: result.data };
}
