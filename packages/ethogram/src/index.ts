export const EVENT_SCHEMA_VERSION = 1 as const;

/**
 * The open payload map is deliberately empty for the envelope-only release.
 * Later vocabulary specs can add keyed payloads to this interface; the mapped
 * types below then become correlated discriminated unions without changing the
 * public EventDraft or Event shapes.
 */
export interface EventPayloadMap {}

type EventType<Payloads extends object> = Extract<keyof Payloads, string>;

type DraftMember<Type extends string, Payload> = {
  type: Type;
  payload: Payload;
  capturedAt?: string;
};

type DraftFor<Payloads extends object> = {
  [Type in EventType<Payloads>]: DraftMember<Type, Payloads[Type]>;
}[EventType<Payloads>];

export type EventDraft<Payloads extends object = EventPayloadMap> =
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

export type Event<Payloads extends object = EventPayloadMap> =
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

/** Parse a decoded JSON value as an Event, rejecting envelope drift. */
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
    payload: value.payload,
    ...(typeof value.capturedAt === "string"
      ? { capturedAt: value.capturedAt }
      : {}),
  };
}

/**
 * Serialise an Event in its canonical compact form: no presentation
 * whitespace, envelope keys in the order this SDK always constructs an Event
 * (`v`, `type`, `runId`, `seq`, `ts`, `payload`, `capturedAt`), and payload
 * object keys sorted recursively by UTF-8 byte order (array order is left
 * alone, but objects nested inside an array are themselves sorted). Both SDKs
 * commit to emitting exactly these bytes for the same event, so the
 * conformance harness diffs producer output directly rather than normalising
 * it first.
 */
export function serialiseEvent(event: Event): string {
  return JSON.stringify({
    ...event,
    payload: sortObjectKeysByUtf8Bytes(event.payload),
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
