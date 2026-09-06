export const EVENT_SCHEMA_VERSION = 1 as const;

export const RUN_KINDS = [
  "loop",
  "handoff",
  "subagent",
  "session",
  "judgment",
] as const;

export type RunKind = (typeof RUN_KINDS)[number];

export const RUN_OUTCOMES = [
  "completed",
  "failed",
  "no-op",
  "timed-out",
  "interrupted",
  "permission-denied",
  "canceled",
] as const;

export type RunOutcome = (typeof RUN_OUTCOMES)[number];

export interface RunCeilings {
  costUsd?: number;
  tokens?: number;
  wallMs?: number;
}

export interface RunStartedPayload {
  kind: RunKind;
  actor: string;
  harness: string;
  model?: string;
  parentRunId?: string;
  parentToolUseId?: string;
  schedule?: string;
  repository?: string;
  workOrder?: string;
  ceilings?: RunCeilings;
}

export interface RunUsage {
  inputTokens?: number;
  outputTokens?: number;
  cacheReadTokens?: number;
  cacheCreationTokens?: number;
  unit?: string;
}

export interface RunFinishedPayload {
  outcome: RunOutcome;
  reason?: string;
  truncated?: boolean;
  costUsd?: number;
  usage?: RunUsage;
  durationMs: number;
  estimated?: boolean;
}

/**
 * The protocol payload map. Supplying it to `EventDraft` or `Event` produces a
 * correlated discriminated union; their unparameterised forms deliberately
 * remain open for callers that only need the envelope or handle future types.
 */
export interface EventPayloadMap {
  "run.started": RunStartedPayload;
  "run.finished": RunFinishedPayload;
}

type EventType<Payloads extends object> = Extract<keyof Payloads, string>;

type DraftMember<Type extends string, Payload> = {
  type: Type;
  payload: Payload;
  capturedAt?: string;
};

type DraftFor<Payloads extends object> = {
  [Type in EventType<Payloads>]: DraftMember<Type, Payloads[Type]>;
}[EventType<Payloads>];

export type EventDraft<Payloads extends object = object> =
  [EventType<Payloads>] extends [never]
    ? DraftMember<string, unknown>
    : DraftFor<Payloads>;

type StoredFields = {
  v: typeof EVENT_SCHEMA_VERSION;
  runId: string;
  seq: number;
  ts: string;
};

type EventMember<Type extends string, Payload> = StoredFields &
  DraftMember<Type, Payload>;

type EventFor<Payloads extends object> = {
  [Type in EventType<Payloads>]: EventMember<Type, Payloads[Type]>;
}[EventType<Payloads>];

export type Event<Payloads extends object = object> =
  [EventType<Payloads>] extends [never]
    ? EventMember<string, unknown>
    : EventFor<Payloads>;

export interface StampFields {
  runId: string;
  seq: number;
  ts: string;
}

type Stamped<Draft> = Draft extends DraftMember<infer Type, infer Payload>
  ? EventMember<Type, Payload>
  : never;

export function stamp<Draft extends DraftMember<string, unknown>>(
  draft: Draft,
  fields: StampFields,
): Stamped<Draft> {
  return {
    v: EVENT_SCHEMA_VERSION,
    type: draft.type,
    runId: fields.runId,
    seq: fields.seq,
    ts: fields.ts,
    payload: draft.payload,
    ...(draft.capturedAt === undefined
      ? {}
      : { capturedAt: draft.capturedAt }),
  } as Stamped<Draft>;
}

const REQUIRED_EVENT_FIELDS = [
  "v",
  "type",
  "runId",
  "seq",
  "ts",
  "payload",
] as const;

const EVENT_FIELDS = new Set<string>([
  ...REQUIRED_EVENT_FIELDS,
  "capturedAt",
]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

const RUN_KIND_VALUES = new Set<string>(RUN_KINDS);
const RUN_OUTCOME_VALUES = new Set<string>(RUN_OUTCOMES);

const RUN_STARTED_FIELDS = new Set<string>([
  "kind",
  "actor",
  "harness",
  "model",
  "parentRunId",
  "parentToolUseId",
  "schedule",
  "repository",
  "workOrder",
  "ceilings",
]);

const RUN_CEILING_FIELDS = new Set<string>([
  "costUsd",
  "tokens",
  "wallMs",
]);

const RUN_FINISHED_FIELDS = new Set<string>([
  "outcome",
  "reason",
  "truncated",
  "costUsd",
  "usage",
  "durationMs",
  "estimated",
]);

const RUN_USAGE_FIELDS = new Set<string>([
  "inputTokens",
  "outputTokens",
  "cacheReadTokens",
  "cacheCreationTokens",
  "unit",
]);

/**
 * Returns the entries of `value` whose keys are not in `fields`, to be
 * carried forward as an opaque extension rather than rejected or dropped
 * (issue #12): a sink that forwards an event it does not fully understand
 * must be byte-preserving, or the stream loses data silently at exactly the
 * boundary this protocol exists to cross. Only the **envelope** and the
 * closed unions (`kind`, `outcome`) stay strict; an unknown payload field is
 * tolerated at read and retained on forward.
 */
function extractUnknownFields(
  value: Record<string, unknown>,
  fields: ReadonlySet<string>,
): Record<string, unknown> {
  const unknown: Record<string, unknown> = {};
  for (const [key, fieldValue] of Object.entries(value)) {
    if (!fields.has(key)) {
      unknown[key] = fieldValue;
    }
  }
  return unknown;
}

function requiredString(
  value: Record<string, unknown>,
  field: string,
  name: string,
): string {
  if (!Object.hasOwn(value, field)) {
    throw new TypeError(`${name} is missing required field: ${field}`);
  }
  const fieldValue = value[field];
  if (typeof fieldValue !== "string") {
    throw new TypeError(`${name}.${field} must be a string`);
  }
  return fieldValue;
}

function requiredNumber(
  value: Record<string, unknown>,
  field: string,
  name: string,
): number {
  if (!Object.hasOwn(value, field)) {
    throw new TypeError(`${name} is missing required field: ${field}`);
  }
  const fieldValue = value[field];
  if (typeof fieldValue !== "number" || !Number.isFinite(fieldValue)) {
    throw new TypeError(`${name}.${field} must be a finite number`);
  }
  return fieldValue;
}

function optionalString(
  value: Record<string, unknown>,
  field: string,
  name: string,
): string | undefined {
  if (!Object.hasOwn(value, field)) {
    return undefined;
  }
  const fieldValue = value[field];
  if (typeof fieldValue !== "string") {
    throw new TypeError(`${name}.${field} must be a string when present`);
  }
  return fieldValue;
}

function optionalNumber(
  value: Record<string, unknown>,
  field: string,
  name: string,
): number | undefined {
  if (!Object.hasOwn(value, field)) {
    return undefined;
  }
  const fieldValue = value[field];
  if (typeof fieldValue !== "number" || !Number.isFinite(fieldValue)) {
    throw new TypeError(`${name}.${field} must be a finite number when present`);
  }
  return fieldValue;
}

function optionalBoolean(
  value: Record<string, unknown>,
  field: string,
  name: string,
): boolean | undefined {
  if (!Object.hasOwn(value, field)) {
    return undefined;
  }
  const fieldValue = value[field];
  if (typeof fieldValue !== "boolean") {
    throw new TypeError(`${name}.${field} must be a boolean when present`);
  }
  return fieldValue;
}

function parseRunCeilings(value: unknown): RunCeilings {
  const name = "RunStartedPayload.ceilings";
  if (!isRecord(value)) {
    throw new TypeError(`${name} must be an object`);
  }

  const costUsd = optionalNumber(value, "costUsd", name);
  const tokens = optionalNumber(value, "tokens", name);
  const wallMs = optionalNumber(value, "wallMs", name);
  return {
    ...(costUsd === undefined ? {} : { costUsd }),
    ...(tokens === undefined ? {} : { tokens }),
    ...(wallMs === undefined ? {} : { wallMs }),
    ...extractUnknownFields(value, RUN_CEILING_FIELDS),
  };
}

function parseRunUsage(value: unknown): RunUsage {
  const name = "RunFinishedPayload.usage";
  if (!isRecord(value)) {
    throw new TypeError(`${name} must be an object`);
  }

  const inputTokens = optionalNumber(value, "inputTokens", name);
  const outputTokens = optionalNumber(value, "outputTokens", name);
  const cacheReadTokens = optionalNumber(value, "cacheReadTokens", name);
  const cacheCreationTokens = optionalNumber(
    value,
    "cacheCreationTokens",
    name,
  );
  const unit = optionalString(value, "unit", name);
  return {
    ...(inputTokens === undefined ? {} : { inputTokens }),
    ...(outputTokens === undefined ? {} : { outputTokens }),
    ...(cacheReadTokens === undefined ? {} : { cacheReadTokens }),
    ...(cacheCreationTokens === undefined ? {} : { cacheCreationTokens }),
    ...(unit === undefined ? {} : { unit }),
    ...extractUnknownFields(value, RUN_USAGE_FIELDS),
  };
}

/**
 * Parse and validate a `run.started` payload. Unknown fields are tolerated
 * and retained (issue #12): required fields and the closed `kind` union stay
 * strict, but a field this SDK does not recognise survives parsing and is
 * re-emitted on serialisation rather than being rejected or silently
 * dropped by the object literal below.
 */
export function parseRunStartedPayload(value: unknown): RunStartedPayload {
  const name = "RunStartedPayload";
  if (!isRecord(value)) {
    throw new TypeError(`${name} must be an object`);
  }

  const kind = requiredString(value, "kind", name);
  if (!RUN_KIND_VALUES.has(kind)) {
    throw new TypeError(`${name}.kind has unknown value: ${kind}`);
  }
  const actor = requiredString(value, "actor", name);
  const harness = requiredString(value, "harness", name);
  const model = optionalString(value, "model", name);
  const parentRunId = optionalString(value, "parentRunId", name);
  const parentToolUseId = optionalString(value, "parentToolUseId", name);
  const schedule = optionalString(value, "schedule", name);
  const repository = optionalString(value, "repository", name);
  const workOrder = optionalString(value, "workOrder", name);
  const ceilings = Object.hasOwn(value, "ceilings")
    ? parseRunCeilings(value.ceilings)
    : undefined;

  return {
    kind: kind as RunKind,
    actor,
    harness,
    ...(model === undefined ? {} : { model }),
    ...(parentRunId === undefined ? {} : { parentRunId }),
    ...(parentToolUseId === undefined ? {} : { parentToolUseId }),
    ...(schedule === undefined ? {} : { schedule }),
    ...(repository === undefined ? {} : { repository }),
    ...(workOrder === undefined ? {} : { workOrder }),
    ...(ceilings === undefined ? {} : { ceilings }),
    ...extractUnknownFields(value, RUN_STARTED_FIELDS),
  };
}

/**
 * Parse and validate a `run.finished` payload. Unknown fields are tolerated
 * and retained (issue #12): required fields and the closed `outcome` union
 * stay strict, but a field this SDK does not recognise survives parsing and
 * is re-emitted on serialisation rather than being rejected or silently
 * dropped by the object literal below.
 */
export function parseRunFinishedPayload(value: unknown): RunFinishedPayload {
  const name = "RunFinishedPayload";
  if (!isRecord(value)) {
    throw new TypeError(`${name} must be an object`);
  }

  const outcome = requiredString(value, "outcome", name);
  if (!RUN_OUTCOME_VALUES.has(outcome)) {
    throw new TypeError(`${name}.outcome has unknown value: ${outcome}`);
  }
  const reason = optionalString(value, "reason", name);
  const truncated = optionalBoolean(value, "truncated", name);
  const costUsd = optionalNumber(value, "costUsd", name);
  const usage = Object.hasOwn(value, "usage")
    ? parseRunUsage(value.usage)
    : undefined;
  const durationMs = requiredNumber(value, "durationMs", name);
  const estimated = optionalBoolean(value, "estimated", name);

  return {
    outcome: outcome as RunOutcome,
    ...(reason === undefined ? {} : { reason }),
    ...(truncated === undefined ? {} : { truncated }),
    ...(costUsd === undefined ? {} : { costUsd }),
    ...(usage === undefined ? {} : { usage }),
    durationMs,
    ...(estimated === undefined ? {} : { estimated }),
    ...extractUnknownFields(value, RUN_FINISHED_FIELDS),
  };
}

function parseKnownPayload(eventType: string, payload: unknown): unknown {
  switch (eventType) {
    case "run.started":
      return parseRunStartedPayload(payload);
    case "run.finished":
      return parseRunFinishedPayload(payload);
    default:
      return payload;
  }
}

/**
 * The largest magnitude at which an integral number round-trips exactly
 * between this SDK and the Rust SDK (2^53 − 1). Shared by `Event.seq`
 * validation and payload-number validation (issue #9): both reject an
 * out-of-range integral value at parse time rather than rounding it.
 */
const MAX_SAFE_INTEGER_MAGNITUDE = Number.MAX_SAFE_INTEGER;

/**
 * Recursively validates that every integral-valued number in `value` is
 * within the safe-integer magnitude bound, naming the offending path (for
 * example `payload.nested.count` or `payload.items[2].total`) when the check
 * fails. Non-integral numbers are never bounded, no matter how large their
 * magnitude. Mirrors the `Event.seq` check above and reuses the same bound
 * (issue #9): a value that needs more precision must be carried as a string
 * instead of a number.
 *
 * `JSON.parse` has already collapsed any literal too large to represent
 * exactly before this function ever sees it, so only magnitude can be
 * tested here; that is sufficient, because the bound is on magnitude.
 */
function validatePayloadNumbers(value: unknown, path: string): void {
  if (typeof value === "number") {
    if (Number.isInteger(value) && Math.abs(value) > MAX_SAFE_INTEGER_MAGNITUDE) {
      throw new TypeError(
        `${path} is an integral number whose magnitude exceeds the safe integer bound of ${MAX_SAFE_INTEGER_MAGNITUDE}; a value that needs more precision must be carried as a string`,
      );
    }
    return;
  }
  if (Array.isArray(value)) {
    for (const [index, item] of value.entries()) {
      validatePayloadNumbers(item, `${path}[${index}]`);
    }
    return;
  }
  if (isRecord(value)) {
    for (const [key, child] of Object.entries(value)) {
      validatePayloadNumbers(child, `${path}.${key}`);
    }
  }
}

/**
 * Recursively sorts an object's own keys by UTF-8 byte order, leaving array
 * order untouched (though objects nested inside an array are themselves
 * sorted). Sorts by UTF-8 bytes via `Buffer.compare`, deliberately not by
 * default JavaScript string comparison: `<` on strings compares UTF-16 code
 * units, which diverges from Rust's byte-wise `String` ordering for
 * characters outside the Basic Multilingual Plane.
 */
function sortObjectKeysByUtf8Bytes(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(sortObjectKeysByUtf8Bytes);
  }
  if (isRecord(value)) {
    return Object.fromEntries(
      Object.entries(value)
        .sort(([left], [right]) =>
          Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8")),
        )
        .map(([key, child]) => [key, sortObjectKeysByUtf8Bytes(child)]),
    );
  }
  return value;
}

/**
 * Parse a decoded JSON value as an Event, rejecting envelope drift and invalid
 * payloads for event types this SDK knows. Unknown event types deliberately
 * retain the open payload behaviour: both SDKs validate the two `run.*`
 * vocabulary members here without turning the envelope parser into a closed
 * event-type registry.
 *
 * A payload's *unknown fields* are a separate axis from its *unknown type*
 * and are tolerated rather than rejected (issue #12): `parseRunStartedPayload`
 * and `parseRunFinishedPayload` no longer reject a field they do not
 * recognise, and they carry it forward into the returned payload object
 * rather than silently dropping it, so `serialiseEvent` re-emits it. Only the
 * envelope stays closed to unknown fields, via the check just below.
 */
export function parseEvent(value: unknown): Event {
  if (!isRecord(value)) {
    throw new TypeError("Event must be a JSON object");
  }

  const unknownFields = Object.keys(value).filter(
    (field) => !EVENT_FIELDS.has(field),
  );
  if (unknownFields.length > 0) {
    throw new TypeError(
      `Event contains unknown field${unknownFields.length === 1 ? "" : "s"}: ${unknownFields.join(", ")}`,
    );
  }

  for (const field of REQUIRED_EVENT_FIELDS) {
    if (!Object.hasOwn(value, field)) {
      throw new TypeError(`Event is missing required field: ${field}`);
    }
  }

  if (value.v !== EVENT_SCHEMA_VERSION) {
    throw new TypeError(
      `Event.v must be ${EVENT_SCHEMA_VERSION}; received ${String(value.v)}`,
    );
  }
  if (typeof value.type !== "string") {
    throw new TypeError("Event.type must be a string");
  }
  if (typeof value.runId !== "string") {
    throw new TypeError("Event.runId must be a string");
  }
  if (!Number.isSafeInteger(value.seq) || (value.seq as number) < 1) {
    throw new TypeError("Event.seq must be a positive safe integer");
  }
  if (typeof value.ts !== "string") {
    throw new TypeError("Event.ts must be a string");
  }
  if (value.payload === undefined) {
    throw new TypeError("Event.payload must be a JSON value");
  }
  validatePayloadNumbers(value.payload, "payload");
  const payload = parseKnownPayload(value.type, value.payload);
  if (
    Object.hasOwn(value, "capturedAt") &&
    typeof value.capturedAt !== "string"
  ) {
    throw new TypeError("Event.capturedAt must be a string when present");
  }

  return {
    v: EVENT_SCHEMA_VERSION,
    type: value.type,
    runId: value.runId,
    seq: value.seq as number,
    ts: value.ts,
    payload,
    ...(typeof value.capturedAt === "string"
      ? { capturedAt: value.capturedAt }
      : {}),
  };
}

/**
 * Serialise an Event in its canonical compact form: no presentation
 * whitespace, envelope keys in declared order (`v`, `type`, `runId`, `seq`,
 * `ts`, `payload`, `capturedAt`), and payload object keys sorted recursively
 * by UTF-8 byte order (array order is left alone, but objects nested inside an
 * array are themselves sorted). Both SDKs commit to emitting exactly these
 * bytes for the same event, so the conformance harness diffs producer output
 * directly rather than normalising it first.
 *
 * The envelope is rebuilt field by field rather than spread from `event`,
 * because a spread would preserve whatever key order the caller happened to
 * construct — and an Event that reached this function from anywhere but
 * `parseEvent` or `stamp` carries no guarantee about that. Rust emits its
 * struct's declaration order unconditionally; this is how TypeScript matches
 * it unconditionally too.
 */
export function serialiseEvent(event: Event): string {
  return JSON.stringify({
    v: event.v,
    type: event.type,
    runId: event.runId,
    seq: event.seq,
    ts: event.ts,
    payload: sortObjectKeysByUtf8Bytes(event.payload),
    ...(event.capturedAt === undefined ? {} : { capturedAt: event.capturedAt }),
  });
}

export type Clock = () => string;

/** A minimal in-memory reference for sequencing rules, not a storage engine. */
export class InMemorySink {
  readonly #clock: Clock;
  readonly #runs = new Map<string, Event[]>();

  constructor(clock: Clock = () => new Date().toISOString()) {
    this.#clock = clock;
  }

  appendDraft(runId: string, draft: EventDraft): Event {
    const events = this.#eventsFor(runId);
    const event = stamp(draft, {
      runId,
      seq: events.length + 1,
      ts: this.#clock(),
    });
    events.push(event);
    return event;
  }

  appendEvent(input: Event): Event {
    const event = parseEvent(input);
    const events = this.#eventsFor(event.runId);
    const expected = events.length + 1;
    if (event.seq !== expected) {
      throw new Error(
        `Event sequence for run ${event.runId} must be ${expected}; received ${event.seq}`,
      );
    }
    events.push(event);
    return event;
  }

  events(runId: string): readonly Event[] {
    return [...(this.#runs.get(runId) ?? [])];
  }

  #eventsFor(runId: string): Event[] {
    let events = this.#runs.get(runId);
    if (events === undefined) {
      events = [];
      this.#runs.set(runId, events);
    }
    return events;
  }
}

export interface FoldedRun {
  runId: string;
  kind: RunKind;
  actor: string;
  harness: string;
  parentRunId?: string;
  outcome?: RunOutcome;
  durationMs?: number;
  open: boolean;
}

/**
 * Reference implementation of the lifecycle fold for one run, not a
 * consumer-facing run model. Unrelated events between the two lifecycle
 * markers are ignored; malformed lifecycle payloads and mismatched run ids
 * are rejected so the example cannot manufacture a coherent run from an
 * incoherent sequence.
 */
export function foldRun(events: Iterable<Event>): FoldedRun | undefined {
  let run: FoldedRun | undefined;

  for (const event of events) {
    if (event.type === "run.started") {
      if (run !== undefined) {
        throw new Error("Run fold received more than one run.started event");
      }
      const payload = parseRunStartedPayload(event.payload);
      run = {
        runId: event.runId,
        kind: payload.kind,
        actor: payload.actor,
        harness: payload.harness,
        ...(payload.parentRunId === undefined
          ? {}
          : { parentRunId: payload.parentRunId }),
        open: true,
      };
      continue;
    }

    if (event.type === "run.finished") {
      if (run === undefined) {
        throw new Error("Run fold received run.finished before run.started");
      }
      if (event.runId !== run.runId) {
        throw new Error(
          `Run fold expected run id ${run.runId}; received ${event.runId}`,
        );
      }
      const payload = parseRunFinishedPayload(event.payload);
      run = {
        ...run,
        outcome: payload.outcome,
        durationMs: payload.durationMs,
        open: false,
      };
    }
  }

  return run;
}
