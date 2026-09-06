import assert from "node:assert/strict";
import { describe, test } from "node:test";

import {
  EVENT_SCHEMA_VERSION,
  InMemorySink,
  MAX_EXCERPT_SCALARS,
  MAX_TEXT_SCALARS,
  RUN_KINDS,
  RUN_OUTCOMES,
  excerpt,
  foldRun,
  parseAgentCompletedPayload,
  parseAgentStartedPayload,
  parseEvent,
  serialiseEvent,
  stamp,
  type Event,
  type EventDraft,
  type EventPayloadMap,
} from "./index.js";

const RUN_STARTED_WIRE =
  '{"v":1,"type":"run.started","runId":"run-child","seq":1,"ts":"2026-09-06T10:45:01.000Z","payload":{"actor":"builder","ceilings":{"costUsd":2.5,"tokens":4000,"wallMs":60000},"harness":"codex","kind":"subagent","model":"gpt-5","parentRunId":"run-parent","parentToolUseId":"tool-7","repository":"onsager-ai/ethogram","schedule":"builder@2026-09-06T10:45Z","workOrder":"order-5"},"capturedAt":"2026-09-06T10:45:00.000Z"}';

const RUN_FINISHED_WIRE =
  '{"v":1,"type":"run.finished","runId":"run-child","seq":2,"ts":"2026-09-06T10:45:02.000Z","payload":{"costUsd":1.25,"durationMs":1250,"estimated":true,"outcome":"completed","reason":"placeholder complete","truncated":false,"usage":{"cacheCreationTokens":30,"cacheReadTokens":20,"inputTokens":10,"outputTokens":40,"unit":"weighted-tokens"}}}';

// Cross-SDK byte identity for an event with an unknown payload field (issue
// #12). This exact literal is also hand-built in the Rust suite
// (`lib.rs`'s `unknown_payload_field_matches_the_typescript_pinned_bytes`)
// and asserted there against the same string.
const UNKNOWN_PAYLOAD_FIELD_WIRE =
  '{"v":1,"type":"run.started","runId":"run-cross","seq":1,"ts":"2026-09-07T00:00:00.000Z","payload":{"0alpha":"before-actor","actor":"builder","harness":"codex","kind":"loop","list":[{"apple":2,"zebra":1},3,"text"],"nested":{"apple":2,"zebra":1},"zzzTail":"after-kind"}}';

// Cross-SDK byte identity for all six agent payloads. These exact literals
// are pasted into the Rust suite and asserted there against events built from
// Rust's typed payload structs rather than parsed fixtures.
const AGENT_STARTED_WIRE =
  '{"v":1,"type":"agent.started","runId":"run-agent","seq":1,"ts":"2026-09-07T01:00:01.000Z","payload":{"model":"gpt-5","pid":4242,"sessionId":"session-local-7","stage":"open-ended-stage"}}';
const AGENT_TEXT_WIRE =
  '{"v":1,"type":"agent.text","runId":"run-agent","seq":2,"ts":"2026-09-07T01:00:02.000Z","payload":{"parentToolUseId":"parent-tool-1","stage":"narrate","text":"A😀漢","truncated":false}}';
const AGENT_TOOL_USE_WIRE =
  '{"v":1,"type":"agent.tool_use","runId":"run-agent","seq":3,"ts":"2026-09-07T01:00:03.000Z","payload":{"inputExcerpt":"{\\"path\\":\\"README.md\\"}","parentToolUseId":"parent-tool-1","stage":"act","tool":"read_file","toolUseId":"tool-7","truncated":false}}';
const AGENT_TOOL_RESULT_WIRE =
  '{"v":1,"type":"agent.tool_result","runId":"run-agent","seq":4,"ts":"2026-09-07T01:00:04.000Z","payload":{"isError":false,"parentToolUseId":"parent-tool-1","resultExcerpt":"placeholder result","stage":"act","tool":"read_file","toolUseId":"tool-7","truncated":false}}';
const AGENT_COMPLETED_WIRE =
  '{"v":1,"type":"agent.completed","runId":"run-agent","seq":5,"ts":"2026-09-07T01:00:05.000Z","payload":{"costUsd":1.25,"durationMs":2500,"estimated":true,"model":"gpt-5","stage":"finish","turns":3,"usage":{"cacheCreationTokens":30,"cacheReadTokens":20,"inputTokens":10,"outputTokens":40,"unit":"weighted-tokens"}}}';
const AGENT_WARNING_WIRE =
  '{"v":1,"type":"agent.warning","runId":"run-agent","seq":6,"ts":"2026-09-07T01:00:06.000Z","payload":{"message":"placeholder warning","stage":"observe"}}';

const PERMITTED_RUN_KINDS = [
  "loop",
  "handoff",
  "subagent",
  "session",
  "judgment",
] as const;

const PERMITTED_RUN_OUTCOMES = [
  "completed",
  "failed",
  "no-op",
  "timed-out",
  "interrupted",
  "permission-denied",
  "canceled",
] as const;

const completeEvent = (): Event => ({
  v: 1,
  type: "test.happened",
  runId: "run-1",
  seq: 1,
  ts: "2026-09-06T00:00:01.000Z",
  payload: { ok: true },
});

type FuturePayloads = {
  "test.happened": { ok: boolean };
  "test.failed": { reason: string };
};

function assertFuturePayloadCorrelation(draft: EventDraft<FuturePayloads>): void {
  const event = stamp(draft, {
    runId: "run-1",
    seq: 1,
    ts: "2026-09-06T00:00:01.000Z",
  });
  if (event.type === "test.happened") {
    const ok: boolean = event.payload.ok;
    assert.equal(typeof ok, "boolean");
  }
}

function assertRunPayloadCorrelation(event: Event<EventPayloadMap>): void {
  if (event.type === "run.started") {
    const actor: string = event.payload.actor;
    assert.equal(typeof actor, "string");
  } else if (event.type === "run.finished") {
    const durationMs: number = event.payload.durationMs;
    assert.equal(typeof durationMs, "number");
  } else if (event.type === "agent.text") {
    const text: string = event.payload.text;
    assert.equal(typeof text, "string");
  } else if (
    event.type === "agent.tool_use" ||
    event.type === "agent.tool_result"
  ) {
    const tool: string = event.payload.tool;
    assert.equal(typeof tool, "string");
  } else if (event.type === "agent.warning") {
    const message: string = event.payload.message;
    assert.equal(typeof message, "string");
  }
}

function containsLoneSurrogate(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (next < 0xdc00 || next > 0xdfff) {
        return true;
      }
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      return true;
    }
  }
  return false;
}

describe("Event parsing", () => {
  test("rejects each missing required envelope field", () => {
    for (const field of ["v", "type", "runId", "seq", "ts", "payload"]) {
      const candidate: Record<string, unknown> = { ...completeEvent() };
      delete candidate[field];
      assert.throws(() => parseEvent(candidate), new RegExp(field));
    }
  });

  test("rejects unknown envelope fields", () => {
    assert.throws(
      () => parseEvent({ ...completeEvent(), stage: "build" }),
      /unknown field: stage/,
    );
  });

  test("rejects null for the optional capturedAt field", () => {
    assert.throws(
      () => parseEvent({ ...completeEvent(), capturedAt: null }),
      /capturedAt must be a string/,
    );
  });

  test("accepts an integral payload number at the safe bound", () => {
    assert.doesNotThrow(() =>
      parseEvent({
        ...completeEvent(),
        payload: { value: Number.MAX_SAFE_INTEGER },
      }),
    );
  });

  test("rejects a top-level integral payload number beyond the safe bound", () => {
    assert.throws(
      () =>
        parseEvent({
          ...completeEvent(),
          payload: Number.MAX_SAFE_INTEGER + 1,
        }),
      /^TypeError: payload is an integral number whose magnitude exceeds the safe integer bound/,
    );
  });

  test("rejects an out-of-range integral number at a nested path", () => {
    assert.throws(
      () =>
        parseEvent({
          ...completeEvent(),
          payload: { nested: { big: 1e21 } },
        }),
      /^TypeError: payload\.nested\.big is an integral number/,
    );
  });

  test("rejects an out-of-range integral number inside an array of objects", () => {
    assert.throws(
      () =>
        parseEvent({
          ...completeEvent(),
          payload: { items: [{ ok: true }, { total: 1e21 }] },
        }),
      /^TypeError: payload\.items\[1\]\.total is an integral number/,
    );
  });

  test("does not bound non-integral payload numbers", () => {
    assert.doesNotThrow(() =>
      parseEvent({
        ...completeEvent(),
        payload: { value: 0.1 },
      }),
    );
  });

  test("rejects 1e21, matching the ruling example in the review", () => {
    // 1e21 is integral-valued (its fractional part is exactly zero) and its
    // magnitude exceeds the bound, so it is rejected. JSON.parse has already
    // collapsed any too-large literal before parseEvent sees it, so only
    // magnitude can be tested here — that is sufficient, because the bound
    // is on magnitude.
    assert.throws(
      () => parseEvent({ ...completeEvent(), payload: { value: 1e21 } }),
      /is an integral number whose magnitude exceeds the safe integer bound/,
    );
  });

  test("rejects extremely large integral floats regardless of magnitude", () => {
    // Number.MAX_VALUE (1.7976931348623157e308) is, like every JS number at
    // or beyond 2^52 in magnitude, integral by construction — IEEE 754
    // leaves no mantissa bits for a fractional part at that scale, so
    // Number.isInteger(Number.MAX_VALUE) is true. It is therefore not exempt
    // from the bound; exempting it would itself be the kind of special case
    // the ruling in issue #9 rules out for 1e21.
    assert.throws(
      () =>
        parseEvent({
          ...completeEvent(),
          payload: { value: Number.MAX_VALUE },
        }),
      /is an integral number whose magnitude exceeds the safe integer bound/,
    );
  });

  test("keeps unknown event types open", () => {
    assert.deepEqual(parseEvent(completeEvent()), completeEvent());
  });
});

describe("run lifecycle payload parsing", () => {
  test("accepts every permitted run kind", () => {
    assert.deepEqual(RUN_KINDS, PERMITTED_RUN_KINDS);
    for (const kind of PERMITTED_RUN_KINDS) {
      assert.doesNotThrow(() =>
        parseEvent({
          ...completeEvent(),
          type: "run.started",
          payload: { kind, actor: "builder", harness: "codex" },
        }),
      );
    }
  });

  test("rejects an unknown run kind", () => {
    assert.throws(
      () =>
        parseEvent({
          ...completeEvent(),
          type: "run.started",
          payload: {
            kind: "pipeline",
            actor: "builder",
            harness: "codex",
          },
        }),
      /kind has unknown value: pipeline/,
    );
  });

  test("accepts every permitted run outcome", () => {
    assert.deepEqual(RUN_OUTCOMES, PERMITTED_RUN_OUTCOMES);
    for (const outcome of PERMITTED_RUN_OUTCOMES) {
      assert.doesNotThrow(() =>
        parseEvent({
          ...completeEvent(),
          type: "run.finished",
          payload: { outcome, durationMs: 1250 },
        }),
      );
    }
  });

  test("rejects an unknown run outcome", () => {
    assert.throws(
      () =>
        parseEvent({
          ...completeEvent(),
          type: "run.finished",
          payload: { outcome: "succeeded", durationMs: 1250 },
        }),
      /outcome has unknown value: succeeded/,
    );
  });

  test("rejects each missing required run.started field", () => {
    for (const field of ["kind", "actor", "harness"]) {
      const payload: Record<string, unknown> = {
        kind: "subagent",
        actor: "builder",
        harness: "codex",
      };
      delete payload[field];
      assert.throws(
        () =>
          parseEvent({
            ...completeEvent(),
            type: "run.started",
            payload,
          }),
        new RegExp(field),
      );
    }
  });

  test("rejects each missing required run.finished field", () => {
    for (const field of ["outcome", "durationMs"]) {
      const payload: Record<string, unknown> = {
        outcome: "completed",
        durationMs: 1250,
      };
      delete payload[field];
      assert.throws(
        () =>
          parseEvent({
            ...completeEvent(),
            type: "run.finished",
            payload,
          }),
        new RegExp(field),
      );
    }
  });

  test("rejects a non-integer ceilings token count", () => {
    assert.throws(
      () =>
        parseEvent({
          ...completeEvent(),
          type: "run.started",
          payload: {
            kind: "loop",
            actor: "builder",
            harness: "codex",
            ceilings: { tokens: 10.5 },
          },
        }),
      /ceilings\.tokens must be a non-negative safe integer/,
    );
  });

  test("rejects a non-integer usage token count", () => {
    assert.throws(
      () =>
        parseEvent({
          ...completeEvent(),
          type: "run.finished",
          payload: {
            outcome: "completed",
            durationMs: 1250,
            usage: { inputTokens: 10.5 },
          },
        }),
      /usage\.inputTokens must be a non-negative safe integer/,
    );
  });

  test("rejects a negative ceilings wall-clock ceiling", () => {
    assert.throws(
      () =>
        parseEvent({
          ...completeEvent(),
          type: "run.started",
          payload: {
            kind: "loop",
            actor: "builder",
            harness: "codex",
            ceilings: { wallMs: -1 },
          },
        }),
      /ceilings\.wallMs must be a non-negative safe integer/,
    );
  });

  test("rejects a negative usage token count", () => {
    assert.throws(
      () =>
        parseEvent({
          ...completeEvent(),
          type: "run.finished",
          payload: {
            outcome: "completed",
            durationMs: 1250,
            usage: { outputTokens: -1 },
          },
        }),
      /usage\.outputTokens must be a non-negative safe integer/,
    );
  });
});

describe("agent payload parsing", () => {
  test("accepts an open stage string on every agent event", () => {
    const cases = [
      ["agent.started", { stage: "consumer-specific/stage" }],
      ["agent.text", { stage: "consumer-specific/stage", text: "text" }],
      ["agent.tool_use", { stage: "consumer-specific/stage", tool: "read" }],
      ["agent.tool_result", { stage: "consumer-specific/stage", tool: "read" }],
      ["agent.completed", { stage: "consumer-specific/stage" }],
      ["agent.warning", { stage: "consumer-specific/stage", message: "warning" }],
    ] as const;

    for (const [type, payload] of cases) {
      assert.doesNotThrow(() =>
        parseEvent({ ...completeEvent(), type, payload }),
      );
    }
  });

  test("enforces every required agent payload field", () => {
    const cases = [
      ["agent.text", {}, "text"],
      ["agent.tool_use", {}, "tool"],
      ["agent.tool_result", {}, "tool"],
      ["agent.warning", {}, "message"],
    ] as const;

    for (const [type, payload, field] of cases) {
      assert.throws(
        () => parseEvent({ ...completeEvent(), type, payload }),
        new RegExp(field),
      );
      assert.throws(
        () =>
          parseEvent({
            ...completeEvent(),
            type,
            payload: { [field]: 7 },
          }),
        new RegExp(`${field} must be a string`),
      );
    }
  });

  test("validates every optional agent count as a non-negative safe integer", () => {
    const cases = [
      [parseAgentStartedPayload, "pid"],
      [parseAgentCompletedPayload, "turns"],
      [parseAgentCompletedPayload, "durationMs"],
    ] as const;

    for (const [parsePayload, field] of cases) {
      for (const invalid of [-1, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
        assert.throws(
          () => parsePayload({ [field]: invalid }),
          new RegExp(`${field} must be a non-negative safe integer`),
        );
      }
      assert.doesNotThrow(() =>
        parsePayload({ [field]: Number.MAX_SAFE_INTEGER }),
      );
    }
  });

  test("reuses run usage validation for agent.completed", () => {
    assert.throws(
      () =>
        parseEvent({
          ...completeEvent(),
          type: "agent.completed",
          payload: { usage: { cacheCreationTokens: 10.5 } },
        }),
      /usage\.cacheCreationTokens must be a non-negative safe integer/,
    );
  });

  test("retains unknown fields through every agent payload parser", () => {
    const cases = [
      ["agent.started", { future: { value: 1 } }],
      ["agent.text", { text: "text", future: { value: 1 } }],
      ["agent.tool_use", { tool: "read", future: { value: 1 } }],
      ["agent.tool_result", { tool: "read", future: { value: 1 } }],
      [
        "agent.completed",
        {
          future: { value: 1 },
          usage: { inputTokens: 2, futureUsage: "retained" },
        },
      ],
      ["agent.warning", { message: "warning", future: { value: 1 } }],
    ] as const;

    for (const [type, payload] of cases) {
      const forwarded = JSON.parse(
        serialiseEvent(
          parseEvent({ ...completeEvent(), type, payload }),
        ),
      ) as { payload: Record<string, unknown> };
      assert.deepEqual(forwarded.payload.future, { value: 1 });
      if (type === "agent.completed") {
        assert.deepEqual(forwarded.payload.usage, {
          futureUsage: "retained",
          inputTokens: 2,
        });
      }
    }
  });
});

describe("excerpt", () => {
  test("exports the protocol scalar bounds", () => {
    assert.equal(MAX_TEXT_SCALARS, 16_384);
    assert.equal(MAX_EXCERPT_SCALARS, 4_096);
  });

  test("handles ASCII below, at, and one scalar over the bound", () => {
    assert.deepEqual(excerpt("abc", 4), { text: "abc", truncated: false });
    assert.deepEqual(excerpt("abcd", 4), {
      text: "abcd",
      truncated: false,
    });
    assert.deepEqual(excerpt("abcde", 4), {
      text: "abcd",
      truncated: true,
    });
  });

  test("counts astral-plane characters as one scalar and leaves no lone surrogate", () => {
    const result = excerpt(
      "😀".repeat(MAX_EXCERPT_SCALARS + 1),
      MAX_EXCERPT_SCALARS,
    );

    assert.deepEqual(result, {
      text: "😀".repeat(MAX_EXCERPT_SCALARS),
      truncated: true,
    });
    assert.equal(Array.from(result.text).length, MAX_EXCERPT_SCALARS);
    assert.equal(result.text.length, MAX_EXCERPT_SCALARS * 2);
    assert.equal(containsLoneSurrogate(result.text), false);
  });

  test("counts three-byte UTF-8 characters as scalars rather than bytes", () => {
    const result = excerpt(
      "漢".repeat(MAX_EXCERPT_SCALARS + 1),
      MAX_EXCERPT_SCALARS,
    );

    assert.deepEqual(result, {
      text: "漢".repeat(MAX_EXCERPT_SCALARS),
      truncated: true,
    });
    assert.equal(Array.from(result.text).length, MAX_EXCERPT_SCALARS);
    assert.equal(Buffer.byteLength(result.text, "utf8"), MAX_EXCERPT_SCALARS * 3);
  });

  test("pins the same mixed-scalar expectation as Rust", () => {
    assert.deepEqual(excerpt("A😀漢B", 3), {
      text: "A😀漢",
      truncated: true,
    });
  });

  test("round-trips an over-bound astral agent.text without creating a surrogate", () => {
    const bounded = excerpt(
      "😀".repeat(MAX_TEXT_SCALARS + 1),
      MAX_TEXT_SCALARS,
    );
    const event: Event<EventPayloadMap> = {
      v: 1,
      type: "agent.text",
      runId: "run-excerpt",
      seq: 1,
      ts: "2026-09-07T02:00:00.000Z",
      payload: bounded,
    };

    const parsed = parseEvent(JSON.parse(serialiseEvent(event)) as unknown);
    assert.equal(serialiseEvent(parsed), serialiseEvent(event));
    assert.equal(
      Array.from((parsed.payload as { text: string }).text).length,
      MAX_TEXT_SCALARS,
    );
    assert.equal(
      containsLoneSurrogate((parsed.payload as { text: string }).text),
      false,
    );
  });

  test("documents JavaScript's current treatment of an input lone surrogate", () => {
    // The protocol has not ruled on invalid Unicode already present in a
    // JavaScript string. The iterator retains this pre-existing lone high
    // surrogate as one item; JSON.stringify escapes it, producing JSON that
    // serde_json rejects. This pins observation, not a sanitisation rule.
    const loneHighSurrogate = String.fromCharCode(0xd83d);
    const result = excerpt(`a${loneHighSurrogate}b`, 2);

    assert.deepEqual(result, {
      text: `a${loneHighSurrogate}`,
      truncated: true,
    });
    assert.equal(
      JSON.stringify(result),
      '{"text":"a\\ud83d","truncated":true}',
    );
  });
});

describe("stamp", () => {
  test("retains future payload-map correlation", () => {
    assertFuturePayloadCorrelation({
      type: "test.happened",
      payload: { ok: true },
    });
  });

  test("retains the protocol payload-map correlation", () => {
    assertRunPayloadCorrelation({
      v: 1,
      type: "run.started",
      runId: "run-child",
      seq: 1,
      ts: "2026-09-06T10:45:01.000Z",
      payload: { kind: "subagent", actor: "builder", harness: "codex" },
    });
  });

  test("sets the schema version and never accepts a producer override", () => {
    const producerValue = {
      v: 99,
      type: "test.happened",
      payload: null,
    };
    const event = stamp(producerValue, {
      runId: "run-1",
      seq: 1,
      ts: "2026-09-06T00:00:01.000Z",
    });

    assert.equal(EVENT_SCHEMA_VERSION, 1);
    assert.equal(event.v, 1);
  });

  test("preserves capturedAt when present", () => {
    const capturedAt = "2026-09-06T00:00:00.000Z";
    const draft: EventDraft = {
      type: "test.happened",
      payload: { ok: true },
      capturedAt,
    };

    assert.equal(
      stamp(draft, {
        runId: "run-1",
        seq: 1,
        ts: "2026-09-06T00:00:01.000Z",
      }).capturedAt,
      capturedAt,
    );
  });

  test("does not invent capturedAt when absent", () => {
    const event = stamp(
      { type: "test.happened", payload: { ok: true } },
      {
        runId: "run-1",
        seq: 1,
        ts: "2026-09-06T00:00:01.000Z",
      },
    );

    assert.equal(Object.hasOwn(event, "capturedAt"), false);
    assert.equal(serialiseEvent(event).includes("capturedAt"), false);
  });
});

test("the compact serialiser emits no presentation whitespace", () => {
  assert.equal(
    serialiseEvent(completeEvent()),
    '{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"ok":true}}',
  );
});

test("envelope key order does not depend on how the caller built the event", () => {
  // Rust emits its struct's declaration order unconditionally. An Event that
  // reached serialiseEvent from anywhere but parseEvent or stamp carries no
  // guarantee about key order, so spreading it would let the caller's
  // construction order leak onto the wire and diverge from Rust.
  const scrambled = {
    payload: { ok: true },
    ts: "2026-09-06T00:00:01.000Z",
    v: 1,
    runId: "run-1",
    type: "test.happened",
    seq: 1,
  } as unknown as Event;

  assert.equal(serialiseEvent(scrambled), serialiseEvent(completeEvent()));
});

test("capturedAt keeps its declared position when present", () => {
  const scrambled = {
    capturedAt: "2026-09-06T00:00:00.000Z",
    payload: { ok: true },
    v: 1,
    ts: "2026-09-06T00:00:01.000Z",
    runId: "run-1",
    type: "test.happened",
    seq: 1,
  } as unknown as Event;

  assert.equal(
    serialiseEvent(scrambled),
    '{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"ok":true},"capturedAt":"2026-09-06T00:00:00.000Z"}',
  );
});

describe("serialiseEvent payload key sorting", () => {
  test("sorts scrambled payload keys by UTF-8 bytes", () => {
    const event: Event = {
      ...completeEvent(),
      payload: { zebra: 1, mango: 2, apple: 3 },
    };

    assert.equal(
      serialiseEvent(event),
      '{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"apple":3,"mango":2,"zebra":1}}',
    );
  });

  test("sorts nested objects and objects inside arrays, leaving array order alone", () => {
    const event: Event = {
      ...completeEvent(),
      payload: {
        nested: { zebra: 1, apple: 2 },
        list: [
          { zebra: 1, apple: 2 },
          { mango: 3 },
        ],
      },
    };

    assert.equal(
      serialiseEvent(event),
      '{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"list":[{"apple":2,"zebra":1},{"mango":3}],"nested":{"apple":2,"zebra":1}}}',
    );
  });

  test("sorts by UTF-8 bytes, not by default UTF-16 string comparison", () => {
    // U+FFFF (a Basic Multilingual Plane character) encodes to UTF-8 bytes
    // EF BF BF, while U+10000 (the first astral-plane character, a surrogate
    // pair in UTF-16) encodes to F0 90 80 80. Because 0xEF < 0xF0, UTF-8 byte
    // order places U+FFFF first. Default JS string comparison (`<`), which
    // compares UTF-16 code units, disagrees: U+10000's leading surrogate is
    // 0xD800, which is less than U+FFFF's single code unit 0xFFFF, so naive
    // `<` would place U+10000 first instead — the exact divergence from
    // Rust's byte-wise `String` ordering this sort exists to avoid.
    const bmpKey = String.fromCodePoint(0xffff);
    const astralKey = String.fromCodePoint(0x10000);
    assert.ok(astralKey < bmpKey, "sanity check: UTF-16 order disagrees with UTF-8 byte order");

    const event: Event = {
      ...completeEvent(),
      payload: { [astralKey]: 1, [bmpKey]: 2 },
    };

    const expectedPayload = `{${JSON.stringify(bmpKey)}:2,${JSON.stringify(astralKey)}:1}`;
    assert.equal(
      serialiseEvent(event),
      `{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":${expectedPayload}}`,
    );
  });

  test("pins byte-identical run lifecycle events with Rust", () => {
    const started: Event<EventPayloadMap> = {
      v: 1,
      type: "run.started",
      runId: "run-child",
      seq: 1,
      ts: "2026-09-06T10:45:01.000Z",
      payload: {
        kind: "subagent",
        actor: "builder",
        harness: "codex",
        model: "gpt-5",
        parentRunId: "run-parent",
        parentToolUseId: "tool-7",
        schedule: "builder@2026-09-06T10:45Z",
        repository: "onsager-ai/ethogram",
        workOrder: "order-5",
        ceilings: { costUsd: 2.5, tokens: 4000, wallMs: 60000 },
      },
      capturedAt: "2026-09-06T10:45:00.000Z",
    };
    const finished: Event<EventPayloadMap> = {
      v: 1,
      type: "run.finished",
      runId: "run-child",
      seq: 2,
      ts: "2026-09-06T10:45:02.000Z",
      payload: {
        outcome: "completed",
        reason: "placeholder complete",
        truncated: false,
        costUsd: 1.25,
        usage: {
          inputTokens: 10,
          outputTokens: 40,
          cacheReadTokens: 20,
          cacheCreationTokens: 30,
          unit: "weighted-tokens",
        },
        durationMs: 1250,
        estimated: true,
      },
    };

    assert.equal(serialiseEvent(started), RUN_STARTED_WIRE);
    assert.equal(serialiseEvent(finished), RUN_FINISHED_WIRE);
  });

  test("pins byte-identical agent events with Rust", () => {
    const events: Event<EventPayloadMap>[] = [
      {
        v: 1,
        type: "agent.started",
        runId: "run-agent",
        seq: 1,
        ts: "2026-09-07T01:00:01.000Z",
        payload: {
          stage: "open-ended-stage",
          model: "gpt-5",
          sessionId: "session-local-7",
          pid: 4242,
        },
      },
      {
        v: 1,
        type: "agent.text",
        runId: "run-agent",
        seq: 2,
        ts: "2026-09-07T01:00:02.000Z",
        payload: {
          stage: "narrate",
          text: "A😀漢",
          truncated: false,
          parentToolUseId: "parent-tool-1",
        },
      },
      {
        v: 1,
        type: "agent.tool_use",
        runId: "run-agent",
        seq: 3,
        ts: "2026-09-07T01:00:03.000Z",
        payload: {
          stage: "act",
          tool: "read_file",
          inputExcerpt: '{"path":"README.md"}',
          truncated: false,
          toolUseId: "tool-7",
          parentToolUseId: "parent-tool-1",
        },
      },
      {
        v: 1,
        type: "agent.tool_result",
        runId: "run-agent",
        seq: 4,
        ts: "2026-09-07T01:00:04.000Z",
        payload: {
          stage: "act",
          tool: "read_file",
          isError: false,
          resultExcerpt: "placeholder result",
          truncated: false,
          toolUseId: "tool-7",
          parentToolUseId: "parent-tool-1",
        },
      },
      {
        v: 1,
        type: "agent.completed",
        runId: "run-agent",
        seq: 5,
        ts: "2026-09-07T01:00:05.000Z",
        payload: {
          stage: "finish",
          turns: 3,
          costUsd: 1.25,
          model: "gpt-5",
          usage: {
            inputTokens: 10,
            outputTokens: 40,
            cacheReadTokens: 20,
            cacheCreationTokens: 30,
            unit: "weighted-tokens",
          },
          durationMs: 2500,
          estimated: true,
        },
      },
      {
        v: 1,
        type: "agent.warning",
        runId: "run-agent",
        seq: 6,
        ts: "2026-09-07T01:00:06.000Z",
        payload: {
          stage: "observe",
          message: "placeholder warning",
        },
      },
    ];

    assert.deepEqual(events.map(serialiseEvent), [
      AGENT_STARTED_WIRE,
      AGENT_TEXT_WIRE,
      AGENT_TOOL_USE_WIRE,
      AGENT_TOOL_RESULT_WIRE,
      AGENT_COMPLETED_WIRE,
      AGENT_WARNING_WIRE,
    ]);
  });

  test("sorts all amended run usage fields", () => {
    const parsed = parseEvent(JSON.parse(RUN_FINISHED_WIRE) as unknown);
    assert.equal(serialiseEvent(parsed), RUN_FINISHED_WIRE);
    assert.match(
      RUN_FINISHED_WIRE,
      /"usage":\{"cacheCreationTokens":30,"cacheReadTokens":20,"inputTokens":10,"outputTokens":40,"unit":"weighted-tokens"\}/,
    );
  });

  test("omits absent optional run payload fields instead of writing null", () => {
    const started: Event<EventPayloadMap> = {
      v: 1,
      type: "run.started",
      runId: "run-root",
      seq: 1,
      ts: "2026-09-06T00:00:00.000Z",
      payload: { kind: "session", actor: "user", harness: "codex" },
    };
    const finished: Event<EventPayloadMap> = {
      v: 1,
      type: "run.finished",
      runId: "run-root",
      seq: 2,
      ts: "2026-09-06T00:00:01.000Z",
      payload: { outcome: "no-op", durationMs: 1000 },
    };

    assert.equal(
      serialiseEvent(started),
      '{"v":1,"type":"run.started","runId":"run-root","seq":1,"ts":"2026-09-06T00:00:00.000Z","payload":{"actor":"user","harness":"codex","kind":"session"}}',
    );
    assert.equal(
      serialiseEvent(finished),
      '{"v":1,"type":"run.finished","runId":"run-root","seq":2,"ts":"2026-09-06T00:00:01.000Z","payload":{"durationMs":1000,"outcome":"no-op"}}',
    );
  });
});

describe("payload tolerance (issue #12)", () => {
  // Payloads are tolerant at read and retaining on forward: an unknown
  // payload field is never rejected and never dropped, so a forwarder that
  // parses a newer producer's event does not lose data silently at exactly
  // the boundary this protocol exists to cross. What stays strict is the
  // envelope, the closed unions (`kind`, `outcome`), and required fields of
  // a known type — all covered elsewhere in this file.

  test("an unknown payload field round-trips across the sort boundary", () => {
    // "0alpha" sorts before the known key "actor"; "zzzTail" sorts after
    // the known key "kind". Both unknown fields must survive parsing and
    // reappear in the canonical sorted position.
    const raw = {
      v: 1,
      type: "run.started",
      runId: "run-1",
      seq: 1,
      ts: "2026-09-06T00:00:01.000Z",
      payload: {
        "0alpha": "before-actor",
        actor: "builder",
        harness: "codex",
        kind: "loop",
        zzzTail: "after-kind",
      },
    };

    assert.equal(
      serialiseEvent(parseEvent(raw)),
      '{"v":1,"type":"run.started","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"0alpha":"before-actor","actor":"builder","harness":"codex","kind":"loop","zzzTail":"after-kind"}}',
    );
  });

  test("an unknown payload field holding a nested object and an array is preserved and sorted", () => {
    const raw = {
      v: 1,
      type: "run.started",
      runId: "run-1",
      seq: 1,
      ts: "2026-09-06T00:00:01.000Z",
      payload: {
        kind: "loop",
        actor: "builder",
        harness: "codex",
        nested: { zebra: 1, apple: 2 },
        list: [{ zebra: 1, apple: 2 }, 3, "text"],
      },
    };

    assert.equal(
      serialiseEvent(parseEvent(raw)),
      '{"v":1,"type":"run.started","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"actor":"builder","harness":"codex","kind":"loop","list":[{"apple":2,"zebra":1},3,"text"],"nested":{"apple":2,"zebra":1}}}',
    );
  });

  test("unknown payload numbers round-trip byte-identically", () => {
    // The hazard named in the follow-up brief is specific to Rust's
    // `#[serde(flatten)]` buffering layer, which does not exist on this
    // side, but the expectation is the same: a large integer just inside
    // the safe bound, a small integer, an integral-valued float, and a
    // non-integral value must each keep their own canonical representation
    // (issue #9) after passing through as an unrecognised field.
    const raw = {
      v: 1,
      type: "run.started",
      runId: "run-1",
      seq: 1,
      ts: "2026-09-06T00:00:01.000Z",
      payload: {
        kind: "loop",
        actor: "builder",
        harness: "codex",
        bigInt: 9007199254740991,
        smallInt: 1,
        integralFloat: 2.0,
        fraction: 0.000001,
      },
    };

    assert.equal(
      serialiseEvent(parseEvent(raw)),
      '{"v":1,"type":"run.started","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"actor":"builder","bigInt":9007199254740991,"fraction":0.000001,"harness":"codex","integralFloat":2,"kind":"loop","smallInt":1}}',
    );
  });

  test("a payload without unknown fields serialises exactly as before", () => {
    const raw = {
      v: 1,
      type: "run.started",
      runId: "run-1",
      seq: 1,
      ts: "2026-09-06T00:00:01.000Z",
      payload: { kind: "loop", actor: "builder", harness: "codex" },
    };

    assert.equal(
      serialiseEvent(parseEvent(raw)),
      '{"v":1,"type":"run.started","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"actor":"builder","harness":"codex","kind":"loop"}}',
    );
  });

  test("pins byte-identical bytes for an unknown payload field with Rust", () => {
    const raw = {
      v: 1,
      type: "run.started",
      runId: "run-cross",
      seq: 1,
      ts: "2026-09-07T00:00:00.000Z",
      payload: {
        "0alpha": "before-actor",
        actor: "builder",
        harness: "codex",
        kind: "loop",
        list: [{ zebra: 1, apple: 2 }, 3, "text"],
        nested: { zebra: 1, apple: 2 },
        zzzTail: "after-kind",
      },
    };

    assert.equal(serialiseEvent(parseEvent(raw)), UNKNOWN_PAYLOAD_FIELD_WIRE);
  });
});

describe("InMemorySink", () => {
  test("stamps drafts with a gapless sequence and its clock", () => {
    const timestamps = [
      "2026-09-06T00:00:01.000Z",
      "2026-09-06T00:00:02.000Z",
    ];
    const sink = new InMemorySink(() => timestamps.shift() ?? "unreachable");

    const first = sink.appendDraft("run-1", {
      type: "test.happened",
      payload: 1,
    });
    const second = sink.appendDraft("run-1", {
      type: "test.happened",
      payload: 2,
    });

    assert.deepEqual([first.seq, second.seq], [1, 2]);
    assert.deepEqual(
      [first.ts, second.ts],
      ["2026-09-06T00:00:01.000Z", "2026-09-06T00:00:02.000Z"],
    );
  });

  test("preserves a shipped event and rejects a sequence gap", () => {
    const sink = new InMemorySink();
    const first = completeEvent();

    assert.deepEqual(sink.appendEvent(first), first);
    assert.throws(
      () => sink.appendEvent({ ...completeEvent(), seq: 3 }),
      /must be 2; received 3/,
    );
    assert.equal(sink.events("run-1").length, 1);
  });
});

describe("foldRun", () => {
  test("folds lifecycle events into one run and preserves its parent", () => {
    const events: Event[] = [
      {
        v: 1,
        type: "run.started",
        runId: "run-child",
        seq: 1,
        ts: "2026-09-06T10:45:01.000Z",
        payload: {
          kind: "subagent",
          actor: "builder",
          harness: "codex",
          parentRunId: "run-parent",
        },
      },
      {
        v: 1,
        type: "agent.tool_use",
        runId: "run-child",
        seq: 2,
        ts: "2026-09-06T10:45:01.500Z",
        payload: { name: "placeholder" },
      },
      {
        v: 1,
        type: "run.finished",
        runId: "run-child",
        seq: 3,
        ts: "2026-09-06T10:45:02.000Z",
        payload: { outcome: "completed", durationMs: 1000 },
      },
    ];

    assert.deepEqual(foldRun(events), {
      runId: "run-child",
      kind: "subagent",
      actor: "builder",
      harness: "codex",
      parentRunId: "run-parent",
      outcome: "completed",
      durationMs: 1000,
      open: false,
    });
  });
});
