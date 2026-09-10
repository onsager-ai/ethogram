import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { describe, test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  AGENT_COMPLETED,
  AGENT_STARTED,
  AGENT_TEXT,
  AGENT_TOOL_RESULT,
  AGENT_TOOL_USE,
  AGENT_WARNING,
  CAPTURE_REFUSAL_CAUSES,
  CAPTURE_REFUSED,
  CONTROL_APPLIED,
  CONTROL_KINDS,
  CONTROL_REQUESTED,
  DECISION_ANSWERED,
  DECISION_KINDS,
  DECISION_REQUESTED,
  EVENT_SCHEMA_VERSION,
  InMemorySink,
  KNOWN_TYPES,
  MAX_EXCERPT_SCALARS,
  MAX_PAYLOAD_BYTES,
  MAX_TEXT_SCALARS,
  RUN_FINISHED,
  RUN_KINDS,
  RUN_OUTCOMES,
  RUN_STARTED,
  RunClosedError,
  SequenceError,
  excerpt,
  foldRun,
  parseAgentCompletedPayload,
  parseAgentStartedPayload,
  parseCaptureRefusedPayload,
  parseControlAppliedPayload,
  parseControlRequestedPayload,
  parseDecisionAnsweredPayload,
  parseDecisionRequestedPayload,
  parseEvent,
  serialiseEvent,
  stamp,
  validate,
  validateDecisionAnswerAgainstRequest,
  type CaptureRefusedPayload,
  type DecisionAnsweredPayload,
  type DecisionRequestedPayload,
  type Event,
  type EventDraft,
  type EventPayloadMap,
} from "./index.js";

const RUN_STARTED_WIRE =
  '{"v":1,"type":"run.started","runId":"run-child","seq":1,"ts":"2026-09-06T10:45:01.000Z","payload":{"actor":"builder","ceilings":{"costUsd":2.5,"tokens":4000,"wallMs":60000},"harness":"codex","kind":"subagent","model":"gpt-5","parentRunId":"run-parent","parentToolUseId":"tool-7","repository":"onsager-ai/ethogram","schedule":"builder@2026-09-06T10:45Z","workOrder":"order-5"},"capturedAt":"2026-09-06T10:45:00.000Z"}';

const RUN_FINISHED_WIRE =
  '{"v":1,"type":"run.finished","runId":"run-child","seq":2,"ts":"2026-09-06T10:45:02.000Z","payload":{"costUsd":1.25,"durationMs":1250,"estimated":true,"outcome":"completed","reason":"placeholder complete","truncated":false,"usage":{"cacheCreationTokens":30,"cacheReadTokens":20,"inputTokens":10,"outputTokens":40,"unit":"weighted-tokens"}}}';

// Cross-SDK byte identity for the new vocabulary and all five ceilings. These
// exact literals are pasted into the Rust suite and asserted against events
// hand-built through each SDK's typed API.
const RELAY_CEILINGS_WIRE =
  '{"v":1,"type":"run.started","runId":"run-batch","seq":1,"ts":"2026-09-07T04:00:00.000Z","payload":{"actor":"observer","ceilings":{"costUsd":2.5,"idleMs":30000,"tokens":4000,"turns":12,"wallMs":60000},"harness":"relay-harness","kind":"relay"}}';
const CAPPED_OUTCOME_WIRE =
  '{"v":1,"type":"run.finished","runId":"run-batch","seq":2,"ts":"2026-09-07T04:00:01.000Z","payload":{"durationMs":1000,"outcome":"capped","reason":"turns"}}';

// Cross-SDK byte identity for the two outcomes added by spec onsager-ai/ethogram#31
// (onsager-ai/ostrom-hub#146). These exact literals are pasted into the Rust suite and
// asserted there against events hand-built through its typed API.
const BLOCKED_OUTCOME_WIRE =
  '{"v":1,"type":"run.finished","runId":"run-blocked","seq":1,"ts":"2026-09-07T08:00:00.000Z","payload":{"durationMs":500,"outcome":"blocked","reason":"awaiting-upstream-quota"}}';
const UNSTARTED_OUTCOME_WIRE =
  '{"v":1,"type":"run.finished","runId":"run-unstarted","seq":1,"ts":"2026-09-07T08:00:01.000Z","payload":{"durationMs":0,"outcome":"unstarted","reason":"spawn"}}';

// This value is intentionally one neither SDK will ever know. Keeping the
// same literal in both suites proves an older relay retaining an unfamiliar
// member emits exactly the bytes a future vocabulary-aware SDK would emit.
const UNKNOWN_OUTCOME_WIRE =
  '{"v":1,"type":"run.finished","runId":"run-cross-version","seq":1,"ts":"2026-09-07T04:00:02.000Z","payload":{"durationMs":1250,"outcome":"not-a-real-outcome"}}';

// Cross-SDK byte identity for an event with an unknown payload field (issue
// onsager-ai/ethogram#12). This exact literal is also hand-built in the Rust suite
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

// Cross-SDK byte identity for `agent.completed.sessionId` (issue onsager-ai/ethogram#6 on
// onsager-ai/umwelt#22). This exact literal is also hand-built in the Rust suite
// (`lib.rs`'s `agent_completed_session_id_matches_the_typescript_pinned_bytes`)
// and asserted there against the same string, proving both SDKs agree on the
// new field's bytes without touching a single existing fixture.
const AGENT_COMPLETED_WITH_SESSION_WIRE =
  '{"v":1,"type":"agent.completed","runId":"run-agent","seq":7,"ts":"2026-09-07T01:00:07.000Z","payload":{"costUsd":2.5,"durationMs":3200,"estimated":false,"model":"gpt-5","sessionId":"session-local-7","stage":"finish","turns":5,"usage":{"cacheCreationTokens":15,"cacheReadTokens":5,"inputTokens":50,"outputTokens":75,"unit":"weighted-tokens"}}}';

// Cross-SDK byte identity for both `control.*` events (spec onsager-ai/ethogram#8). These exact
// literals are pasted into the Rust suite and asserted there against events
// built from Rust's typed payload structs rather than parsed fixtures.
const CONTROL_REQUESTED_WIRE =
  '{"v":1,"type":"control.requested","runId":"run-control","seq":1,"ts":"2026-09-07T05:00:00.000Z","payload":{"by":"operator","controlId":"control-1","kind":"steer","text":"take point on the next turn","truncated":false}}';
const CONTROL_APPLIED_FAILED_WIRE =
  '{"v":1,"type":"control.applied","runId":"run-control","seq":2,"ts":"2026-09-07T05:00:01.000Z","payload":{"controlId":"control-1","ok":false,"reason":"not-live"}}';
const CONTROL_APPLIED_INTERRUPT_WIRE =
  '{"v":1,"type":"control.applied","runId":"run-control","seq":3,"ts":"2026-09-07T05:00:02.000Z","payload":{"controlId":"control-2","landedIn":"tool-9","ok":true}}';

// This value is intentionally one neither SDK will ever know, matching issue
// onsager-ai/ethogram#12's own example. Keeping the same literal in both suites proves an older
// relay retaining an unfamiliar member emits exactly the bytes a future
// vocabulary-aware SDK would emit.
const UNKNOWN_CONTROL_KIND_WIRE =
  '{"v":1,"type":"control.requested","runId":"run-cross-version","seq":1,"ts":"2026-09-07T05:00:03.000Z","payload":{"by":"operator","controlId":"control-3","kind":"teleport"}}';

// Cross-SDK byte identity for a fully populated `over_bound` refusal and a
// minimal `gap` refusal (spec onsager-ai/ethogram#15). These exact literals are pasted into the
// Rust suite and asserted against events hand-built through each SDK's typed
// API.
const CAPTURE_REFUSED_OVER_BOUND_WIRE =
  '{"v":1,"type":"capture.refused","runId":"run-relay","seq":1,"ts":"2026-09-07T06:00:00.000Z","payload":{"cause":"over_bound","count":20000,"field":"AgentTextPayload.text","max":16384,"sourceRunId":"run-source","sourceSeq":8,"sourceType":"agent.text"}}';
const CAPTURE_REFUSED_GAP_WIRE =
  '{"v":1,"type":"capture.refused","runId":"run-relay","seq":2,"ts":"2026-09-07T06:00:01.000Z","payload":{"cause":"gap","sourceRunId":"run-source-gap"}}';

// This value is intentionally one neither SDK will ever know. The cause
// string and the whole canonical event must survive an older relay exactly.
const UNKNOWN_CAPTURE_REFUSAL_CAUSE_WIRE =
  '{"v":1,"type":"capture.refused","runId":"run-relay","seq":3,"ts":"2026-09-07T06:00:02.000Z","payload":{"cause":"never-a-valid-capture-refusal-cause","sourceRunId":"run-source"}}';

// Cross-SDK byte identity for both `decision.*` events (spec onsager-ai/ethogram#7). These exact
// literals are pasted into the Rust suite and asserted against events built
// through each SDK's correlated typed API.
const DECISION_REQUESTED_WIRE =
  '{"v":1,"type":"decision.requested","runId":"run-decision","seq":1,"ts":"2026-09-07T07:00:00.000Z","payload":{"decisionId":"decision-1","dossier":{"blastRadius":"one repository","optionsRuledOut":["auto-proceed","discard the request"],"question":"May the run execute the deployment tool?","recommendedAction":"deny unless the operator confirms the target","truncated":false},"expiresAt":"2026-09-07T07:05:00.000Z","kind":"permission","onTimeout":"deny","options":[{"id":"allow","label":"Allow once"},{"id":"deny","label":"Deny"}],"subject":"deploy"}}';
const DECISION_ANSWERED_HUMAN_WIRE =
  '{"v":1,"type":"decision.answered","runId":"run-decision","seq":2,"ts":"2026-09-07T07:01:00.000Z","payload":{"by":"principal:user:alice","decisionId":"decision-1","optionId":"allow"}}';
const DECISION_ANSWERED_TIMEOUT_WIRE =
  '{"v":1,"type":"decision.answered","runId":"run-decision","seq":3,"ts":"2026-09-07T07:05:00.000Z","payload":{"by":"principal:runtime:permission-timeout","byTimeout":true,"decisionId":"decision-1","optionId":"deny","reversal":"allow"}}';

// Cross-SDK byte identity for `decision.answered.requestedRunId` (spec onsager-ai/ethogram#7
// correction). This exact literal is pasted into the Rust suite and asserted
// against an event built through each SDK's typed API.
const DECISION_ANSWERED_WITH_REQUESTED_RUN_WIRE =
  '{"v":1,"type":"decision.answered","runId":"run-decision-answer","seq":1,"ts":"2026-09-07T07:10:00.000Z","payload":{"by":"principal:user:alice","decisionId":"decision-1","optionId":"allow","requestedRunId":"run-decision"}}';

// Cross-SDK byte identity for a `<verb>:<subject>` action-id `reversal`
// (ruled on onsager-ai/ethogram#7): `revoke:required_checks` undoes `excuse:required_checks`
// even though it was never among the options offered to the human. This
// exact literal is pasted into the Rust suite and asserted against an event
// built through each SDK's typed API.
const DECISION_ANSWERED_ACTION_REVERSAL_WIRE =
  '{"v":1,"type":"decision.answered","runId":"run-decision-revoke","seq":1,"ts":"2026-09-07T09:00:00.000Z","payload":{"by":"principal:user:alice","decisionId":"decision-revoke-1","optionId":"excuse:required_checks","reversal":"revoke:required_checks"}}';

// This value is intentionally one neither SDK will ever know. The kind
// string and the whole canonical event must survive an older relay exactly.
const UNKNOWN_DECISION_KIND_WIRE =
  '{"v":1,"type":"decision.requested","runId":"run-cross-version","seq":1,"ts":"2026-09-07T07:00:03.000Z","payload":{"decisionId":"decision-unknown","dossier":{"blastRadius":"none","optionsRuledOut":[],"question":"Unknown kind?","recommendedAction":"inspect"},"kind":"never-a-valid-decision-kind","options":[]}}';

const PERMITTED_CONTROL_KINDS = ["interrupt", "steer", "answer"] as const;

const PERMITTED_CAPTURE_REFUSAL_CAUSES = [
  "over_bound",
  "gap",
  "duplicate",
  "finished",
  "malformed",
] as const;

const PERMITTED_DECISION_KINDS = [
  "permission",
  "tripwire",
  "gate_inconclusive",
  "human_decides",
  "budget",
] as const;

const PERMITTED_RUN_KINDS = [
  "loop",
  "handoff",
  "subagent",
  "session",
  "judgment",
  "relay",
] as const;

const PERMITTED_RUN_OUTCOMES = [
  "completed",
  "failed",
  "no-op",
  "timed-out",
  "interrupted",
  "permission-denied",
  "canceled",
  "capped",
  "blocked",
  "unstarted",
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
  } else if (event.type === "capture.refused") {
    const sourceRunId: string = event.payload.sourceRunId;
    assert.equal(typeof sourceRunId, "string");
  } else if (event.type === "decision.requested") {
    const question: string = event.payload.dossier.question;
    assert.equal(typeof question, "string");
  } else if (event.type === "decision.answered") {
    const optionId: string = event.payload.optionId;
    assert.equal(typeof optionId, "string");
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

  // Ruled from onsager-ai/ostrom-hub#146: an optional field is absent or has a value; an
  // explicit `null` is a parse error, because it cannot populate an optional
  // field faithfully. This is a representability question, not a `validate`
  // policy. The capturedAt test just above already covers the envelope's own
  // optional field; the three tests below cover one payload field of each
  // optional shape this SDK has (string, safe integer, boolean), and the
  // final test pairs with all three to show the same fields parse cleanly
  // when merely absent.

  test("rejects null for the optional run.started.parentRunId string field", () => {
    assert.throws(
      () =>
        parseEvent({
          ...completeEvent(),
          type: "run.started",
          payload: {
            kind: "subagent",
            actor: "builder",
            harness: "codex",
            parentRunId: null,
          },
        }),
      /RunStartedPayload\.parentRunId must be a string when present/,
    );
  });

  test("rejects null for the optional agent.completed.turns integer field", () => {
    assert.throws(
      () =>
        parseEvent({
          ...completeEvent(),
          type: "agent.completed",
          payload: { turns: null },
        }),
      /AgentCompletedPayload\.turns must be a non-negative safe integer when present/,
    );
  });

  test("rejects null for the optional agent.text.truncated boolean field", () => {
    assert.throws(
      () =>
        parseEvent({
          ...completeEvent(),
          type: "agent.text",
          payload: { text: "hello", truncated: null },
        }),
      /AgentTextPayload\.truncated must be a boolean when present/,
    );
  });

  test("accepts the same optional payload fields when merely absent, not null", () => {
    const started = parseEvent({
      ...completeEvent(),
      type: "run.started",
      payload: { kind: "subagent", actor: "builder", harness: "codex" },
    });
    assert.equal(
      Object.hasOwn(started.payload as object, "parentRunId"),
      false,
    );

    const completed = parseEvent({
      ...completeEvent(),
      type: "agent.completed",
      payload: {},
    });
    assert.equal(Object.hasOwn(completed.payload as object, "turns"), false);

    const text = parseEvent({
      ...completeEvent(),
      type: "agent.text",
      payload: { text: "hello" },
    });
    assert.equal(Object.hasOwn(text.payload as object, "truncated"), false);
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
    // the ruling in issue onsager-ai/ethogram#9 rules out for 1e21.
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
      const payload = { kind, actor: "builder", harness: "codex" };
      assert.doesNotThrow(() =>
        parseEvent({
          ...completeEvent(),
          type: "run.started",
          payload,
        }),
      );
      assert.doesNotThrow(() => validate(RUN_STARTED, payload));
    }
  });

  test("parses an unknown run kind verbatim and validate reports it", () => {
    const event = parseEvent({
      ...completeEvent(),
      type: "run.started",
      payload: {
        kind: "pipeline",
        actor: "builder",
        harness: "codex",
      },
    });

    assert.equal((event.payload as { kind: string }).kind, "pipeline");
    assert.throws(
      () =>
        validate("run.started", {
          kind: "pipeline",
          actor: "builder",
          harness: "codex",
        }),
      /kind has unknown value: pipeline/,
    );
  });

  test("accepts every permitted run outcome", () => {
    assert.deepEqual(RUN_OUTCOMES, PERMITTED_RUN_OUTCOMES);
    for (const outcome of PERMITTED_RUN_OUTCOMES) {
      const payload = { outcome, durationMs: 1250 };
      assert.doesNotThrow(() =>
        parseEvent({
          ...completeEvent(),
          type: "run.finished",
          payload,
        }),
      );
      assert.doesNotThrow(() => validate(RUN_FINISHED, payload));
    }
  });

  test("parses an unknown run outcome verbatim and validate reports it", () => {
    const event = parseEvent({
      ...completeEvent(),
      type: "run.finished",
      payload: { outcome: "succeeded", durationMs: 1250 },
    });

    assert.equal((event.payload as { outcome: string }).outcome, "succeeded");
    assert.throws(
      () =>
        validate("run.finished", {
          outcome: "succeeded",
          durationMs: 1250,
        }),
      /outcome has unknown value: succeeded/,
    );
  });

  test("refuses the hub literal abandoned", () => {
    // onsager-ai/ostrom-hub#146: "abandoned" is the hub's own name for "timed-out"
    // under another spelling, and the hub renames it rather than this
    // protocol adopting it. It is refused exactly like any other
    // unrecognised value — this test is what stops someone adding it later
    // by reflex.
    const event = parseEvent({
      ...completeEvent(),
      type: "run.finished",
      payload: { outcome: "abandoned", durationMs: 1250 },
    });

    assert.equal((event.payload as { outcome: string }).outcome, "abandoned");
    assert.throws(
      () =>
        validate("run.finished", {
          outcome: "abandoned",
          durationMs: 1250,
        }),
      /outcome has unknown value: abandoned/,
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

  test("applies the safe-integer rule to idle and turn ceilings", () => {
    for (const field of ["idleMs", "turns"] as const) {
      for (const invalid of [-1, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
        assert.throws(
          () =>
            parseEvent({
              ...completeEvent(),
              type: "run.started",
              payload: {
                kind: "loop",
                actor: "builder",
                harness: "codex",
                ceilings: { [field]: invalid },
              },
            }),
          new RegExp(`${field}.*safe integer`),
        );
      }
      assert.doesNotThrow(() =>
        parseEvent({
          ...completeEvent(),
          type: "run.started",
          payload: {
            kind: "loop",
            actor: "builder",
            harness: "codex",
            ceilings: { [field]: Number.MAX_SAFE_INTEGER },
          },
        }),
      );
    }
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

  test("rejects a non-integer run.finished durationMs", () => {
    assert.throws(
      () =>
        parseEvent({
          ...completeEvent(),
          type: "run.finished",
          payload: { outcome: "completed", durationMs: 1250.5 },
        }),
      /durationMs must be a non-negative safe integer/,
    );
  });

  test("rejects a negative run.finished durationMs", () => {
    assert.throws(
      () =>
        parseEvent({
          ...completeEvent(),
          type: "run.finished",
          payload: { outcome: "completed", durationMs: -5 },
        }),
      /durationMs must be a non-negative safe integer/,
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
          sessionId: "session-local-7",
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
        // A known field (`sessionId`) alongside an unknown one (`future`):
        // neither displaces the other.
        assert.equal(forwarded.payload.sessionId, "session-local-7");
      }
    }
  });
});

describe("control payload parsing (spec onsager-ai/ethogram#8)", () => {
  test("parseEvent accepts every permitted control kind without text", () => {
    // parseEvent answers "can both SDKs carry this?", not "should a
    // producer have emitted this?" A steer naming no text is perfectly
    // representable -- validate (below) rejects it as a producer error, but
    // a forwarder must still be able to relay it. This is the test that
    // would fail if someone later "helpfully" moved the steer-needs-text
    // rule into the parser.
    assert.deepEqual(CONTROL_KINDS, PERMITTED_CONTROL_KINDS);
    for (const kind of PERMITTED_CONTROL_KINDS) {
      const payload = { controlId: "control-1", kind, by: "operator" };
      assert.doesNotThrow(() =>
        parseEvent({
          ...completeEvent(),
          type: "control.requested",
          payload,
        }),
      );
    }
  });

  test("validate accepts an interrupt with no text", () => {
    // An interrupt has nothing to say by design.
    const payload = { controlId: "control-1", kind: "interrupt", by: "operator" };
    assert.doesNotThrow(() => validate(CONTROL_REQUESTED, payload));
  });

  test("validate accepts a steer with text", () => {
    const payload = {
      controlId: "control-1",
      kind: "steer",
      by: "operator",
      text: "take point on the next turn",
    };
    assert.doesNotThrow(() => validate(CONTROL_REQUESTED, payload));
  });

  test("validate rejects a steer with absent text", () => {
    const payload = { controlId: "control-1", kind: "steer", by: "operator" };
    assert.throws(
      () => validate(CONTROL_REQUESTED, payload),
      new TypeError(
        'ControlRequestedPayload.text is required and must not be empty when kind is "steer": a steer with nothing to say is a producer error',
      ),
    );
  });

  test("validate rejects a steer with empty text", () => {
    // A zero-length instruction is the same defect as an absent one.
    const payload = {
      controlId: "control-1",
      kind: "steer",
      by: "operator",
      text: "",
    };
    assert.throws(
      () => validate(CONTROL_REQUESTED, payload),
      new TypeError(
        'ControlRequestedPayload.text is required and must not be empty when kind is "steer": a steer with nothing to say is a producer error',
      ),
    );
  });

  test("parses an unknown control kind verbatim and validate reports it", () => {
    // "teleport" is a value neither SDK will ever know, matching issue onsager-ai/ethogram#12's
    // own example. There is deliberately no "pause" member either (see
    // ControlKind's doc comment), but that is a closed-vocabulary fact, not
    // an unknown-string one, so it is not exercised here.
    const event = parseEvent({
      ...completeEvent(),
      type: "control.requested",
      payload: { controlId: "control-3", kind: "teleport", by: "operator" },
    });

    assert.equal((event.payload as { kind: string }).kind, "teleport");
    assert.throws(
      () => validate(CONTROL_REQUESTED, event.payload),
      /kind has unknown value: teleport/,
    );
  });

  test("unknown control kind keeps cross-version byte identity with Rust and the input", () => {
    const parsed = parseEvent(JSON.parse(UNKNOWN_CONTROL_KIND_WIRE) as unknown);

    assert.equal((parsed.payload as { kind: string }).kind, "teleport");
    assert.equal(serialiseEvent(parsed), UNKNOWN_CONTROL_KIND_WIRE);
  });

  test("rejects each missing required control.requested field", () => {
    for (const field of ["controlId", "kind", "by"]) {
      const payload: Record<string, unknown> = {
        controlId: "control-1",
        kind: "steer",
        by: "operator",
      };
      delete payload[field];
      assert.throws(
        () =>
          parseEvent({
            ...completeEvent(),
            type: "control.requested",
            payload,
          }),
        new RegExp(field),
      );
    }
  });

  test("rejects each missing required control.applied field", () => {
    for (const field of ["controlId", "ok"]) {
      const payload: Record<string, unknown> = {
        controlId: "control-1",
        ok: true,
      };
      delete payload[field];
      assert.throws(
        () =>
          parseEvent({
            ...completeEvent(),
            type: "control.applied",
            payload,
          }),
        new RegExp(field),
      );
    }
  });

  test("retains unknown fields through both control payload parsers", () => {
    const cases = [
      [
        "control.requested",
        {
          controlId: "control-1",
          kind: "steer",
          by: "operator",
          future: { value: 1 },
        },
      ],
      [
        "control.applied",
        { controlId: "control-1", ok: true, future: { value: 1 } },
      ],
    ] as const;

    for (const [type, payload] of cases) {
      const forwarded = JSON.parse(
        serialiseEvent(parseEvent({ ...completeEvent(), type, payload })),
      ) as { payload: Record<string, unknown> };
      assert.deepEqual(forwarded.payload.future, { value: 1 });
    }
  });

  test("absent optional control payload fields are omitted instead of writing null", () => {
    assert.equal(
      JSON.stringify(
        parseControlRequestedPayload({
          controlId: "control-1",
          kind: "interrupt",
          by: "operator",
        }),
      ),
      '{"controlId":"control-1","kind":"interrupt","by":"operator"}',
    );
    assert.equal(
      JSON.stringify(
        parseControlAppliedPayload({ controlId: "control-1", ok: true }),
      ),
      '{"controlId":"control-1","ok":true}',
    );
  });
});

describe("capture.refused payload parsing (spec onsager-ai/ethogram#15)", () => {
  test("accepts every permitted capture refusal cause", () => {
    assert.deepEqual(
      CAPTURE_REFUSAL_CAUSES,
      PERMITTED_CAPTURE_REFUSAL_CAUSES,
    );
    for (const cause of PERMITTED_CAPTURE_REFUSAL_CAUSES) {
      const payload = { cause, sourceRunId: "run-source" };
      assert.doesNotThrow(() =>
        parseEvent({ ...completeEvent(), type: CAPTURE_REFUSED, payload }),
      );
      assert.doesNotThrow(() => validate(CAPTURE_REFUSED, payload));
    }
  });

  test("rejects each missing required capture.refused field", () => {
    for (const field of ["cause", "sourceRunId"]) {
      const payload: Record<string, unknown> = {
        cause: "gap",
        sourceRunId: "run-source",
      };
      delete payload[field];

      assert.throws(
        () =>
          parseEvent({ ...completeEvent(), type: CAPTURE_REFUSED, payload }),
        new RegExp(field),
      );
      assert.throws(
        () => validate(CAPTURE_REFUSED, payload),
        new RegExp(field),
      );
    }
  });

  test("parses an unknown cause verbatim, reports it, and round-trips its bytes", () => {
    const parsed = parseEvent(
      JSON.parse(UNKNOWN_CAPTURE_REFUSAL_CAUSE_WIRE) as unknown,
    );
    const cause = (parsed.payload as { cause: string }).cause;

    assert.equal(cause, "never-a-valid-capture-refusal-cause");
    assert.throws(
      () => validate(CAPTURE_REFUSED, parsed.payload),
      /cause has unknown value: never-a-valid-capture-refusal-cause/,
    );
    assert.equal(serialiseEvent(parsed), UNKNOWN_CAPTURE_REFUSAL_CAUSE_WIRE);
  });

  test("validates every optional count as a non-negative safe integer", () => {
    for (const field of ["sourceSeq", "count", "max"] as const) {
      for (const invalid of [-1, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
        const payload = {
          cause: "over_bound",
          sourceRunId: "run-source",
          [field]: invalid,
        };
        assert.throws(
          () =>
            parseEvent({ ...completeEvent(), type: CAPTURE_REFUSED, payload }),
          new RegExp(`${field}.*safe integer`),
        );
        assert.throws(
          () => validate(CAPTURE_REFUSED, payload),
          new RegExp(`${field}.*safe integer`),
        );
      }

      const payload = {
        cause: "over_bound",
        sourceRunId: "run-source",
        [field]: Number.MAX_SAFE_INTEGER,
      };
      assert.doesNotThrow(() =>
        parseEvent({ ...completeEvent(), type: CAPTURE_REFUSED, payload }),
      );
      assert.doesNotThrow(() => validate(CAPTURE_REFUSED, payload));
    }
  });

  test("carries an over-bound detail at parse and rejects it at validate", () => {
    const event = parseEvent({
      ...completeEvent(),
      type: CAPTURE_REFUSED,
      payload: {
        cause: "malformed",
        sourceRunId: "run-source",
        detail: "x".repeat(MAX_EXCERPT_SCALARS + 1),
      },
    });

    assert.throws(
      () => validate(CAPTURE_REFUSED, event.payload),
      /CaptureRefusedPayload\.detail has 4097 Unicode scalar values; maximum is 4096/,
    );
  });

  test("retains and re-emits unknown payload fields", () => {
    const forwarded = JSON.parse(
      serialiseEvent(
        parseEvent({
          ...completeEvent(),
          type: CAPTURE_REFUSED,
          payload: {
            cause: "gap",
            sourceRunId: "run-source-gap",
            future: { value: 1 },
          },
        }),
      ),
    ) as { payload: Record<string, unknown> };

    assert.deepEqual(forwarded.payload.future, { value: 1 });
  });

  test("omits absent optional fields instead of writing null", () => {
    const payload = parseCaptureRefusedPayload({
      cause: "gap",
      sourceRunId: "run-source-gap",
    });

    assert.equal(
      JSON.stringify(payload),
      '{"cause":"gap","sourceRunId":"run-source-gap"}',
    );
    assert.ok(!JSON.stringify(payload).includes(":null"));
  });

  test("never carries a content-bearing field", () => {
    // `Required` makes this genuinely fully populated: adding even an
    // optional field to `CaptureRefusedPayload` first fails typechecking.
    // Once it is populated here, the exact permitted-key assertion still
    // fails unless the no-content boundary is deliberately revisited.
    // Listing only currently imagined forbidden names would not catch a
    // newly invented content field.
    const payload = {
      cause: "over_bound",
      sourceRunId: "run-source",
      sourceSeq: 8,
      sourceType: AGENT_TEXT,
      field: "AgentTextPayload.text",
      count: 20_000,
      max: 16_384,
      detail: "parser message only",
      truncated: false,
    } satisfies Required<CaptureRefusedPayload>;
    const event: Event<EventPayloadMap> = {
      v: 1,
      type: CAPTURE_REFUSED,
      runId: "run-relay",
      seq: 1,
      ts: "2026-09-07T06:00:00.000Z",
      payload,
    };
    const serialised = JSON.parse(serialiseEvent(event)) as {
      payload: Record<string, unknown>;
    };

    assert.deepEqual(Object.keys(serialised.payload).sort(), [
      "cause",
      "count",
      "detail",
      "field",
      "max",
      "sourceRunId",
      "sourceSeq",
      "sourceType",
      "truncated",
    ]);
  });
});

describe("decision payload parsing and validation (spec onsager-ai/ethogram#7)", () => {
  const minimalRequest = (kind: string = "permission"): Record<string, unknown> => ({
    decisionId: "decision-1",
    kind,
    dossier: {
      question: "Proceed?",
      optionsRuledOut: ["auto-proceed"],
      recommendedAction: "ask the operator",
      blastRadius: "one run",
    },
    options: [{ id: "allow", label: "Allow" }],
  });

  const consistencyRequest = (): DecisionRequestedPayload => ({
    decisionId: "decision-1",
    kind: "permission",
    dossier: {
      question: "Proceed?",
      optionsRuledOut: [],
      recommendedAction: "ask the operator",
      blastRadius: "one run",
    },
    // `deny` is deliberately not a human option here, so these tests
    // distinguish the helper's timeout-only allowance from membership.
    options: [{ id: "allow", label: "Allow" }],
    onTimeout: "deny",
  });

  const consistencyAnswer = (optionId: string): DecisionAnsweredPayload => ({
    decisionId: "decision-1",
    optionId,
    by: "principal:user:alice",
  });

  test("accepts every permitted decision kind without onTimeout", () => {
    assert.deepEqual(DECISION_KINDS, PERMITTED_DECISION_KINDS);
    for (const kind of PERMITTED_DECISION_KINDS) {
      const payload = minimalRequest(kind);
      assert.doesNotThrow(() =>
        parseEvent({ ...completeEvent(), type: DECISION_REQUESTED, payload }),
      );
      assert.doesNotThrow(() => validate(DECISION_REQUESTED, payload));
    }
  });

  test("rejects each missing required decision.requested field", () => {
    for (const field of ["decisionId", "kind", "dossier", "options"]) {
      const payload = minimalRequest();
      delete payload[field];

      assert.throws(
        () =>
          parseEvent({ ...completeEvent(), type: DECISION_REQUESTED, payload }),
        new RegExp(field),
      );
      assert.throws(
        () => validate(DECISION_REQUESTED, payload),
        new RegExp(field),
      );
    }
  });

  test("rejects each missing required dossier field", () => {
    for (const field of [
      "question",
      "optionsRuledOut",
      "recommendedAction",
      "blastRadius",
    ]) {
      const payload = minimalRequest();
      delete (payload.dossier as Record<string, unknown>)[field];

      assert.throws(
        () =>
          parseEvent({ ...completeEvent(), type: DECISION_REQUESTED, payload }),
        new RegExp(field),
      );
      assert.throws(
        () => validate(DECISION_REQUESTED, payload),
        new RegExp(field),
      );
    }
  });

  test("rejects each missing required option field", () => {
    for (const field of ["id", "label"]) {
      const payload = minimalRequest();
      const options = payload.options as Record<string, unknown>[];
      delete options[0]?.[field];

      assert.throws(
        () =>
          parseEvent({ ...completeEvent(), type: DECISION_REQUESTED, payload }),
        new RegExp(field),
      );
      assert.throws(
        () => validate(DECISION_REQUESTED, payload),
        new RegExp(field),
      );
    }
  });

  test("rejects each missing required decision.answered field", () => {
    for (const field of ["decisionId", "optionId", "by"]) {
      const payload: Record<string, unknown> = {
        decisionId: "decision-1",
        optionId: "allow",
        by: "principal:user:alice",
      };
      delete payload[field];

      assert.throws(
        () =>
          parseEvent({ ...completeEvent(), type: DECISION_ANSWERED, payload }),
        new RegExp(field),
      );
      assert.throws(
        () => validate(DECISION_ANSWERED, payload),
        new RegExp(field),
      );
    }
  });

  test("parses an unknown kind verbatim, reports it, and round-trips its bytes", () => {
    const parsed = parseEvent(JSON.parse(UNKNOWN_DECISION_KIND_WIRE) as unknown);
    const kind = (parsed.payload as { kind: string }).kind;

    assert.equal(kind, "never-a-valid-decision-kind");
    assert.equal(JSON.stringify(kind), '"never-a-valid-decision-kind"');
    assert.throws(
      () => validate(DECISION_REQUESTED, parsed.payload),
      /kind has unknown value: never-a-valid-decision-kind/,
    );
    assert.equal(serialiseEvent(parsed), UNKNOWN_DECISION_KIND_WIRE);
  });

  test("validate enforces the permission-only onTimeout rule", () => {
    for (const kind of [
      "tripwire",
      "gate_inconclusive",
      "human_decides",
      "budget",
    ]) {
      const payload = { ...minimalRequest(kind), onTimeout: "deny" };
      assert.throws(
        () => validate(DECISION_REQUESTED, payload),
        new TypeError(
          `DecisionRequestedPayload.onTimeout is permitted only when kind is "permission"; received kind "${kind}"`,
        ),
      );
    }

    assert.throws(
      () =>
        validate(DECISION_REQUESTED, {
          ...minimalRequest("permission"),
          onTimeout: "allow",
        }),
      new TypeError(
        'DecisionRequestedPayload.onTimeout must be "deny" when kind is "permission"; received "allow"',
      ),
    );
    assert.doesNotThrow(() =>
      validate(DECISION_REQUESTED, {
        ...minimalRequest("permission"),
        options: [
          { id: "allow", label: "Allow" },
          { id: "deny", label: "Deny" },
        ],
        onTimeout: "deny",
      }),
    );
    for (const kind of PERMITTED_DECISION_KINDS) {
      assert.doesNotThrow(() =>
        validate(DECISION_REQUESTED, minimalRequest(kind)),
      );
    }
  });

  test("validate rejects onTimeout naming no request option", () => {
    // minimalRequest's only option is "allow"; "deny" is permitted by the
    // kind/value rules above but was never offered.
    assert.throws(
      () =>
        validate(DECISION_REQUESTED, {
          ...minimalRequest("permission"),
          onTimeout: "deny",
        }),
      new TypeError(
        "DecisionRequestedPayload.onTimeout must name one of the request's options[].id; received \"deny\"",
      ),
    );
  });

  test("validate accepts onTimeout naming an existing option", () => {
    assert.doesNotThrow(() =>
      validate(DECISION_REQUESTED, {
        ...minimalRequest("permission"),
        options: [
          { id: "allow", label: "Allow" },
          { id: "deny", label: "Deny" },
        ],
        onTimeout: "deny",
      }),
    );
  });

  test("parseEvent accepts onTimeout on a tripwire", () => {
    // This is a producer-policy violation, but it is representable. The test
    // fails if the rule ever leaks from validate into parsing.
    assert.doesNotThrow(() =>
      parseEvent({
        ...completeEvent(),
        type: DECISION_REQUESTED,
        payload: { ...minimalRequest("tripwire"), onTimeout: "deny" },
      }),
    );
  });

  test("parseEvent accepts onTimeout naming no request option", () => {
    // Naming an option the request never offered is a producer-policy
    // violation, but the event is still representable. This fails if the
    // options-membership rule ever leaks into parsing.
    assert.doesNotThrow(() =>
      parseEvent({
        ...completeEvent(),
        type: DECISION_REQUESTED,
        payload: { ...minimalRequest("permission"), onTimeout: "deny" },
      }),
    );
  });

  test("the cross-event helper checks option, timeout, and decision id, and accepts any reversal", () => {
    const request = consistencyRequest();
    const valid = consistencyAnswer("allow");
    assert.doesNotThrow(() =>
      validateDecisionAnswerAgainstRequest(request, valid),
    );

    assert.throws(
      () =>
        validateDecisionAnswerAgainstRequest(
          request,
          consistencyAnswer("missing"),
        ),
      /optionId/,
    );

    const timeout = { ...consistencyAnswer("deny"), byTimeout: true };
    assert.doesNotThrow(() =>
      validateDecisionAnswerAgainstRequest(request, timeout),
    );
    assert.throws(() =>
      validateDecisionAnswerAgainstRequest(request, {
        ...timeout,
        byTimeout: false,
      }),
    );
    const { byTimeout: _byTimeout, ...withoutByTimeout } = timeout;
    assert.throws(() =>
      validateDecisionAnswerAgainstRequest(request, withoutByTimeout),
    );

    // Ruled on onsager-ai/ethogram#7: a `<verb>:<subject>` action id is a legitimate `reversal`
    // even though it was never offered as a request option —
    // `revoke:required_checks` undoes `excuse:required_checks`, an action
    // the human was never offered as a choice. This deliberately replaces a
    // prior assertion that such a reversal was rejected: that behaviour is
    // the constraint being loosened here, not a bug being preserved.
    assert.doesNotThrow(() =>
      validateDecisionAnswerAgainstRequest(request, {
        ...valid,
        reversal: "revoke:required_checks",
      }),
    );
    assert.throws(
      () =>
        validateDecisionAnswerAgainstRequest(request, {
          ...valid,
          decisionId: "decision-2",
        }),
      /does not match request/,
    );
  });

  test("dossier and option-label bounds are validation-only", () => {
    const over = "😀".repeat(MAX_EXCERPT_SCALARS + 1);
    const cases: readonly [Record<string, unknown>, string][] = [
      [
        {
          ...minimalRequest(),
          dossier: {
            ...(minimalRequest().dossier as Record<string, unknown>),
            question: over,
          },
        },
        "DecisionRequestedPayload.dossier.question",
      ],
      [
        {
          ...minimalRequest(),
          dossier: {
            ...(minimalRequest().dossier as Record<string, unknown>),
            optionsRuledOut: [over],
          },
        },
        "DecisionRequestedPayload.dossier.optionsRuledOut[0]",
      ],
      [
        {
          ...minimalRequest(),
          dossier: {
            ...(minimalRequest().dossier as Record<string, unknown>),
            recommendedAction: over,
          },
        },
        "DecisionRequestedPayload.dossier.recommendedAction",
      ],
      [
        {
          ...minimalRequest(),
          dossier: {
            ...(minimalRequest().dossier as Record<string, unknown>),
            blastRadius: over,
          },
        },
        "DecisionRequestedPayload.dossier.blastRadius",
      ],
      [
        { ...minimalRequest(), options: [{ id: "allow", label: over }] },
        "DecisionRequestedPayload.options[0].label",
      ],
    ];

    for (const [payload, field] of cases) {
      assert.doesNotThrow(() =>
        parseEvent({ ...completeEvent(), type: DECISION_REQUESTED, payload }),
      );
      assert.throws(
        () => validate(DECISION_REQUESTED, payload),
        new TypeError(
          `${field} has ${MAX_EXCERPT_SCALARS + 1} Unicode scalar values; maximum is ${MAX_EXCERPT_SCALARS}`,
        ),
      );
    }
  });

  test("decision identifiers are not excerpt-bounded", () => {
    const identifier = "x".repeat(MAX_EXCERPT_SCALARS + 1);
    assert.doesNotThrow(() =>
      validate(DECISION_REQUESTED, {
        ...minimalRequest(),
        decisionId: identifier,
        options: [{ id: identifier, label: "Allow" }],
      }),
    );
    assert.doesNotThrow(() =>
      validate(DECISION_ANSWERED, {
        decisionId: identifier,
        optionId: identifier,
        by: "principal:user:alice",
        reversal: identifier,
      }),
    );
  });

  test("omits absent optional fields instead of writing null", () => {
    const request = parseDecisionRequestedPayload(minimalRequest());
    const answer = parseDecisionAnsweredPayload(consistencyAnswer("allow"));

    for (const field of ["subject", "expiresAt", "onTimeout"]) {
      assert.equal(Object.hasOwn(request, field), false);
    }
    assert.equal(Object.hasOwn(request.dossier, "truncated"), false);
    for (const field of ["byTimeout", "reversal", "requestedRunId"]) {
      assert.equal(Object.hasOwn(answer, field), false);
    }
    assert.ok(!JSON.stringify(request).includes(":null"));
    assert.ok(!JSON.stringify(answer).includes(":null"));
  });

  test("serialises requestedRunId when present, and it round-trips", () => {
    const answer = parseDecisionAnsweredPayload({
      ...consistencyAnswer("allow"),
      requestedRunId: "run-decision",
    });
    assert.equal(answer.requestedRunId, "run-decision");

    const event = parseEvent({
      ...completeEvent(),
      type: DECISION_ANSWERED,
      payload: answer,
    });
    const serialised = JSON.parse(serialiseEvent(event)) as {
      payload: Record<string, unknown>;
    };
    assert.equal(serialised.payload.requestedRunId, "run-decision");
  });

  test("retains and re-emits unknown fields at every decision payload level", () => {
    const forwardedRequest = JSON.parse(
      serialiseEvent(
        parseEvent({
          ...completeEvent(),
          type: DECISION_REQUESTED,
          payload: {
            ...minimalRequest(),
            dossier: {
              ...(minimalRequest().dossier as Record<string, unknown>),
              futureDossier: { value: 1 },
            },
            options: [
              {
                id: "allow",
                label: "Allow",
                futureOption: { value: 2 },
              },
            ],
            futureRequest: { value: 3 },
          },
        }),
      ),
    ) as { payload: Record<string, unknown> };
    const forwardedAnswer = JSON.parse(
      serialiseEvent(
        parseEvent({
          ...completeEvent(),
          type: DECISION_ANSWERED,
          payload: {
            ...consistencyAnswer("allow"),
            requestedRunId: "run-decision",
            futureAnswer: { value: 4 },
          },
        }),
      ),
    ) as { payload: Record<string, unknown> };

    assert.deepEqual(forwardedRequest.payload.futureRequest, { value: 3 });
    assert.deepEqual(
      (forwardedRequest.payload.dossier as Record<string, unknown>).futureDossier,
      { value: 1 },
    );
    assert.deepEqual(
      (forwardedRequest.payload.options as Record<string, unknown>[])[0]
        ?.futureOption,
      { value: 2 },
    );
    assert.deepEqual(forwardedAnswer.payload.futureAnswer, { value: 4 });
    // `requestedRunId` is a known field, not an extra: adding it must not
    // disturb the unknown-field tolerance path exercised above.
    assert.equal(forwardedAnswer.payload.requestedRunId, "run-decision");
  });
});

describe("validate", () => {
  test("enforces required fields and integer bounds for known types", () => {
    assert.throws(() => validate(RUN_STARTED, {}), /required field: kind/);
    assert.throws(
      () =>
        validate(AGENT_COMPLETED, {
          nested: { turns: Number.MAX_SAFE_INTEGER + 1 },
        }),
      /payload\.nested\.turns is an integral number whose magnitude exceeds the safe integer bound: actual 9007199254740992; maximum 9007199254740991/,
    );
  });

  test("leaves unknown event types open to anything within the universal bounds", () => {
    // No per-field or closed-union checks apply to an unrecognised type
    // (there is no typed shape to check against), but it is not fully
    // unvalidated any more: the universal bounds (issue onsager-ai/ethogram#28) still run. This
    // payload sits comfortably under both, so it validates cleanly.
    assert.doesNotThrow(() => validate("future.happened", "not-an-object"));
  });

  test("rejects a non-string sessionId on agent.completed", () => {
    assert.throws(
      () => validate(AGENT_COMPLETED, { sessionId: 7 }),
      /AgentCompletedPayload\.sessionId must be a string when present/,
    );
  });

  test("rejects a non-string requestedRunId on decision.answered", () => {
    assert.throws(
      () =>
        validate(DECISION_ANSWERED, {
          decisionId: "decision-1",
          optionId: "allow",
          by: "principal:user:alice",
          requestedRunId: 7,
        }),
      /DecisionAnsweredPayload\.requestedRunId must be a string when present/,
    );
  });

  test("reports every capture bound with the field, actual count, and maximum", () => {
    const cases: readonly [string, unknown, string, number][] = [
      // `agent.text`'s own bound is `MAX_TEXT_SCALARS` — the same value as
      // the universal text-scalar floor (issue onsager-ai/ethogram#28), which runs first in
      // `validate` and so is what actually reports this case; the
      // field-specific `AgentTextPayload.text` check below it is never
      // reached for an over-bound `text`, since nothing over the universal
      // bound can also be under it.
      //
      // That shadowing is a fact about the two constants being equal, not a
      // loosened assertion. If `MAX_TEXT_SCALARS` ever rises above
      // `agent.text`'s own field bound, the field-specific message returns
      // and this expectation must change back to `AgentTextPayload.text`.
      [
        AGENT_TEXT,
        { text: "😀".repeat(MAX_TEXT_SCALARS + 1) },
        "payload.text",
        MAX_TEXT_SCALARS,
      ],
      [
        AGENT_TOOL_USE,
        { tool: "read", inputExcerpt: "😀".repeat(MAX_EXCERPT_SCALARS + 1) },
        "AgentToolUsePayload.inputExcerpt",
        MAX_EXCERPT_SCALARS,
      ],
      [
        AGENT_TOOL_RESULT,
        {
          tool: "read",
          resultExcerpt: "😀".repeat(MAX_EXCERPT_SCALARS + 1),
        },
        "AgentToolResultPayload.resultExcerpt",
        MAX_EXCERPT_SCALARS,
      ],
      [
        RUN_FINISHED,
        {
          outcome: "completed",
          durationMs: 1,
          reason: "😀".repeat(MAX_EXCERPT_SCALARS + 1),
        },
        "RunFinishedPayload.reason",
        MAX_EXCERPT_SCALARS,
      ],
      [
        AGENT_WARNING,
        { message: "😀".repeat(MAX_EXCERPT_SCALARS + 1) },
        "AgentWarningPayload.message",
        MAX_EXCERPT_SCALARS,
      ],
      [
        CONTROL_REQUESTED,
        {
          controlId: "control-1",
          kind: "steer",
          by: "operator",
          text: "😀".repeat(MAX_EXCERPT_SCALARS + 1),
        },
        "ControlRequestedPayload.text",
        MAX_EXCERPT_SCALARS,
      ],
      [
        CONTROL_APPLIED,
        {
          controlId: "control-1",
          ok: false,
          reason: "😀".repeat(MAX_EXCERPT_SCALARS + 1),
        },
        "ControlAppliedPayload.reason",
        MAX_EXCERPT_SCALARS,
      ],
      [
        CAPTURE_REFUSED,
        {
          cause: "malformed",
          sourceRunId: "run-source",
          detail: "😀".repeat(MAX_EXCERPT_SCALARS + 1),
        },
        "CaptureRefusedPayload.detail",
        MAX_EXCERPT_SCALARS,
      ],
    ];

    for (const [type, payload, field, maximum] of cases) {
      assert.throws(
        () => validate(type, payload),
        new RegExp(
          `${field.replaceAll(".", "\\.")} has ${maximum + 1} Unicode scalar values; maximum is ${maximum}`,
        ),
      );
    }
  });

  test("parseEvent carries an over-bound event that validate refuses", () => {
    const payload = { text: "x".repeat(20_000) };
    const event = {
      ...completeEvent(),
      type: AGENT_TEXT,
      payload,
    };

    assert.doesNotThrow(() => parseEvent(event));
    // Reported by the universal text-scalar bound (issue onsager-ai/ethogram#28), which runs
    // before the eventType switch and shares `agent.text`'s own bound value,
    // so it is what actually reports this case. If `MAX_TEXT_SCALARS` ever
    // rises above `agent.text`'s field bound, the field-specific
    // `AgentTextPayload.text` message returns and this expectation must
    // change back.
    assert.throws(
      () => validate(event.type, event.payload),
      new TypeError(
        "payload.text has 20000 Unicode scalar values; maximum is 16384",
      ),
    );
  });
});

describe("universal validate bounds (issue onsager-ai/ethogram#28)", () => {
  /**
   * Builds a JSON payload of plain ASCII text spread across ten short,
   * equal-length keys — each nowhere near `MAX_TEXT_SCALARS` on its own —
   * whose canonical serialised form (the same bytes `validate`'s size bound
   * measures) is exactly `target` UTF-8 bytes. Used to hit the
   * `MAX_PAYLOAD_BYTES` boundary exactly, without any single string leaf
   * tripping the scalar bound instead: ASCII `"a"` never needs escaping, so
   * appending one character to any field's string always adds exactly one
   * byte to the total.
   */
  function payloadOfExactByteSize(target: number): Record<string, string> {
    const fields = 10;
    const empty: Record<string, string> = {};
    for (let index = 0; index < fields; index += 1) {
      empty[`p${index}`] = "";
    }
    const base = Buffer.byteLength(JSON.stringify(empty), "utf8");
    assert.ok(
      target >= base,
      `target ${target} is below the minimal payload size ${base} for this scheme`,
    );
    const remaining = target - base;
    const perField = Math.floor(remaining / fields);
    const leftover = remaining % fields;
    assert.ok(
      perField + 1 <= MAX_TEXT_SCALARS,
      `target ${target} needs a field longer than MAX_TEXT_SCALARS; raise fields`,
    );

    const payload: Record<string, string> = {};
    for (let index = 0; index < fields; index += 1) {
      const length = perField + (index < leftover ? 1 : 0);
      payload[`p${index}`] = "a".repeat(length);
    }
    assert.equal(
      Buffer.byteLength(JSON.stringify(payload), "utf8"),
      target,
      "payloadOfExactByteSize construction is wrong",
    );
    return payload;
  }

  test("rejects an unknown event type with an over-long string leaf", () => {
    assert.throws(
      () =>
        validate("future.happened", {
          note: "x".repeat(MAX_TEXT_SCALARS + 1),
        }),
      new TypeError(
        `payload.note has ${MAX_TEXT_SCALARS + 1} Unicode scalar values; maximum is ${MAX_TEXT_SCALARS}`,
      ),
    );
  });

  test("rejects an unknown event type with an over-large serialised payload", () => {
    const payload = payloadOfExactByteSize(MAX_PAYLOAD_BYTES + 1);
    assert.throws(
      () => validate("future.happened", payload),
      new TypeError(
        `payload has ${MAX_PAYLOAD_BYTES + 1} bytes; maximum is ${MAX_PAYLOAD_BYTES}`,
      ),
    );
  });

  test("rejects an over-long string in a known type's retained unknown field", () => {
    assert.throws(
      () =>
        validate(AGENT_TEXT, {
          text: "ok",
          note: "x".repeat(MAX_TEXT_SCALARS + 1),
        }),
      new TypeError(
        `payload.note has ${MAX_TEXT_SCALARS + 1} Unicode scalar values; maximum is ${MAX_TEXT_SCALARS}`,
      ),
    );
  });

  test("locates an over-long string nested in an object, an array, and an object inside an array", () => {
    assert.throws(
      () =>
        validate("future.happened", {
          nested: { note: "x".repeat(MAX_TEXT_SCALARS + 1) },
        }),
      new TypeError(
        `payload.nested.note has ${MAX_TEXT_SCALARS + 1} Unicode scalar values; maximum is ${MAX_TEXT_SCALARS}`,
      ),
    );
    assert.throws(
      () =>
        validate("future.happened", {
          items: ["x".repeat(MAX_TEXT_SCALARS + 1)],
        }),
      new TypeError(
        `payload.items[0] has ${MAX_TEXT_SCALARS + 1} Unicode scalar values; maximum is ${MAX_TEXT_SCALARS}`,
      ),
    );
    assert.throws(
      () =>
        validate("future.happened", {
          items: [{ note: "x".repeat(MAX_TEXT_SCALARS + 1) }],
        }),
      new TypeError(
        `payload.items[0].note has ${MAX_TEXT_SCALARS + 1} Unicode scalar values; maximum is ${MAX_TEXT_SCALARS}`,
      ),
    );
  });

  test("accepts a string at exactly MAX_TEXT_SCALARS and rejects one scalar more", () => {
    // Multi-byte characters prove the count is scalars, not bytes.
    assert.doesNotThrow(() =>
      validate("future.happened", { note: "漢".repeat(MAX_TEXT_SCALARS) }),
    );
    assert.throws(
      () =>
        validate("future.happened", {
          note: "漢".repeat(MAX_TEXT_SCALARS + 1),
        }),
      new TypeError(
        `payload.note has ${MAX_TEXT_SCALARS + 1} Unicode scalar values; maximum is ${MAX_TEXT_SCALARS}`,
      ),
    );
  });

  test("accepts a payload at exactly MAX_PAYLOAD_BYTES and rejects one byte more", () => {
    const atBound = payloadOfExactByteSize(MAX_PAYLOAD_BYTES);
    assert.doesNotThrow(() => validate("future.happened", atBound));

    const overBound = payloadOfExactByteSize(MAX_PAYLOAD_BYTES + 1);
    assert.throws(
      () => validate("future.happened", overBound),
      new TypeError(
        `payload has ${MAX_PAYLOAD_BYTES + 1} bytes; maximum is ${MAX_PAYLOAD_BYTES}`,
      ),
    );
  });

  // Pinned so a future narrowing of MAX_PAYLOAD_BYTES fails loudly: an
  // agent.text at exactly MAX_TEXT_SCALARS composed entirely of astral-plane
  // characters is 16,384 * 4 = 65,536 bytes of text alone, which the
  // originally proposed 65,536-byte bound would have rejected. See
  // MAX_PAYLOAD_BYTES's doc comment for why the constant is 131,072 instead.
  test("an agent.text of exactly MAX_TEXT_SCALARS astral-plane characters validates cleanly", () => {
    assert.doesNotThrow(() =>
      validate(AGENT_TEXT, { text: "😀".repeat(MAX_TEXT_SCALARS) }),
    );
  });

  test("parseEvent accepts both new over-bound grounds that validate rejects", () => {
    const overText = {
      ...completeEvent(),
      type: "future.happened",
      payload: { note: "x".repeat(MAX_TEXT_SCALARS + 1) },
    };
    assert.doesNotThrow(() => parseEvent(overText));
    assert.throws(() => validate(overText.type, overText.payload));

    const overSize = {
      ...completeEvent(),
      type: "future.happened",
      payload: payloadOfExactByteSize(MAX_PAYLOAD_BYTES + 1),
    };
    assert.doesNotThrow(() => parseEvent(overSize));
    assert.throws(() => validate(overSize.type, overSize.payload));
  });
});

describe("conformance corpus validates cleanly (issue onsager-ai/ethogram#28)", () => {
  test("every conformance/v1 fixture parses and validates without error", async () => {
    const repositoryRoot = resolve(
      dirname(fileURLToPath(import.meta.url)),
      "../../..",
    );
    const corpusDirectory = join(repositoryRoot, "conformance", "v1");
    const fixtureNames = (
      await readdir(corpusDirectory, { withFileTypes: true })
    )
      .filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
      .map((entry) => entry.name);

    assert.ok(
      fixtureNames.length > 0,
      "expected at least one fixture in conformance/v1",
    );

    for (const name of fixtureNames) {
      const source = await readFile(join(corpusDirectory, name), "utf8");
      const event = parseEvent(JSON.parse(source) as unknown);
      assert.doesNotThrow(
        () => validate(event.type, event.payload),
        `fixture ${name} failed validate`,
      );
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

  test("replaces a lone high surrogate with U+FFFD (issue onsager-ai/ethogram#6)", () => {
    // The previously-reported case: a lone high surrogate with no matching
    // low surrogate. Per the ruling, this is silently replaced with U+FFFD
    // rather than left intact or rejected, so the resulting JSON is
    // well-formed and serde_json can parse it.
    const loneHighSurrogate = String.fromCharCode(0xd83d);
    const result = excerpt(`a${loneHighSurrogate}b`, 2);

    assert.deepEqual(result, { text: "a�", truncated: true });
    assert.equal(containsLoneSurrogate(result.text), false);
    assert.equal(
      JSON.stringify(result),
      '{"text":"a�","truncated":true}',
    );
  });

  test("replaces a lone low surrogate with U+FFFD", () => {
    const loneLowSurrogate = String.fromCharCode(0xdc00);
    const result = excerpt(`a${loneLowSurrogate}b`, 3);

    assert.deepEqual(result, { text: "a�b", truncated: false });
    assert.equal(containsLoneSurrogate(result.text), false);
  });

  test("leaves a valid surrogate pair completely untouched", () => {
    // A naive fix that replaces surrogate code units individually (rather
    // than the code points the string iterator yields) would mangle this:
    // "😀" is itself a high/low surrogate pair, and neither half is lone.
    const result = excerpt("😀", 5);

    assert.deepEqual(result, { text: "😀", truncated: false });
  });

  test("replaces a lone surrogate while leaving a valid pair in the same string alone", () => {
    const loneHighSurrogate = String.fromCharCode(0xd83d);
    const result = excerpt(`😀a${loneHighSurrogate}`, 3);

    assert.deepEqual(result, { text: `😀a�`, truncated: false });
  });

  test("does not mark a lone surrogate exactly at the bound as truncated", () => {
    // Replacement is one code point in, one code point out, so it must not
    // change how many scalar values the bound counts.
    const loneLowSurrogate = String.fromCharCode(0xdc00);
    const result = excerpt(`a${loneLowSurrogate}`, 2);

    assert.deepEqual(result, { text: "a�", truncated: false });
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

  test("pins relay, capped, and all five ceilings byte-identically with Rust", () => {
    const started: Event<EventPayloadMap> = {
      v: 1,
      type: RUN_STARTED,
      runId: "run-batch",
      seq: 1,
      ts: "2026-09-07T04:00:00.000Z",
      payload: {
        kind: "relay",
        actor: "observer",
        harness: "relay-harness",
        ceilings: {
          costUsd: 2.5,
          tokens: 4000,
          wallMs: 60000,
          idleMs: 30000,
          turns: 12,
        },
      },
    };
    const finished: Event<EventPayloadMap> = {
      v: 1,
      type: RUN_FINISHED,
      runId: "run-batch",
      seq: 2,
      ts: "2026-09-07T04:00:01.000Z",
      payload: {
        outcome: "capped",
        reason: "turns",
        durationMs: 1000,
      },
    };

    assert.equal(serialiseEvent(started), RELAY_CEILINGS_WIRE);
    assert.equal(serialiseEvent(finished), CAPPED_OUTCOME_WIRE);
  });

  test("pins blocked and unstarted outcomes byte-identically with Rust", () => {
    const blocked: Event<EventPayloadMap> = {
      v: 1,
      type: RUN_FINISHED,
      runId: "run-blocked",
      seq: 1,
      ts: "2026-09-07T08:00:00.000Z",
      payload: {
        outcome: "blocked",
        reason: "awaiting-upstream-quota",
        durationMs: 500,
      },
    };
    const unstarted: Event<EventPayloadMap> = {
      v: 1,
      type: RUN_FINISHED,
      runId: "run-unstarted",
      seq: 1,
      ts: "2026-09-07T08:00:01.000Z",
      payload: {
        outcome: "unstarted",
        reason: "spawn",
        durationMs: 0,
      },
    };

    assert.equal(serialiseEvent(blocked), BLOCKED_OUTCOME_WIRE);
    assert.equal(serialiseEvent(unstarted), UNSTARTED_OUTCOME_WIRE);
  });

  test("blocked and unstarted round-trip through parseEvent and serialiseEvent", () => {
    for (const wire of [BLOCKED_OUTCOME_WIRE, UNSTARTED_OUTCOME_WIRE]) {
      const event = parseEvent(JSON.parse(wire) as unknown);
      assert.equal(serialiseEvent(event), wire);
    }
  });

  test("unknown outcome keeps cross-version byte identity with Rust and the input", () => {
    const parsed = parseEvent(JSON.parse(UNKNOWN_OUTCOME_WIRE) as unknown);

    assert.equal(
      (parsed.payload as { outcome: string }).outcome,
      "not-a-real-outcome",
    );
    assert.equal(serialiseEvent(parsed), UNKNOWN_OUTCOME_WIRE);
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

  test("pins byte-identical agent.completed sessionId with Rust", () => {
    const completed: Event<EventPayloadMap> = {
      v: 1,
      type: "agent.completed",
      runId: "run-agent",
      seq: 7,
      ts: "2026-09-07T01:00:07.000Z",
      payload: {
        stage: "finish",
        turns: 5,
        sessionId: "session-local-7",
        costUsd: 2.5,
        model: "gpt-5",
        usage: {
          inputTokens: 50,
          outputTokens: 75,
          cacheReadTokens: 5,
          cacheCreationTokens: 15,
          unit: "weighted-tokens",
        },
        durationMs: 3200,
        estimated: false,
      },
    };

    assert.equal(serialiseEvent(completed), AGENT_COMPLETED_WITH_SESSION_WIRE);
  });

  test("pins byte-identical control events with Rust", () => {
    const requested: Event<EventPayloadMap> = {
      v: 1,
      type: CONTROL_REQUESTED,
      runId: "run-control",
      seq: 1,
      ts: "2026-09-07T05:00:00.000Z",
      payload: {
        controlId: "control-1",
        kind: "steer",
        text: "take point on the next turn",
        truncated: false,
        by: "operator",
      },
    };
    const appliedFailed: Event<EventPayloadMap> = {
      v: 1,
      type: CONTROL_APPLIED,
      runId: "run-control",
      seq: 2,
      ts: "2026-09-07T05:00:01.000Z",
      payload: {
        controlId: "control-1",
        ok: false,
        reason: "not-live",
      },
    };
    const appliedInterrupt: Event<EventPayloadMap> = {
      v: 1,
      type: CONTROL_APPLIED,
      runId: "run-control",
      seq: 3,
      ts: "2026-09-07T05:00:02.000Z",
      payload: {
        controlId: "control-2",
        ok: true,
        landedIn: "tool-9",
      },
    };

    assert.equal(serialiseEvent(requested), CONTROL_REQUESTED_WIRE);
    assert.equal(serialiseEvent(appliedFailed), CONTROL_APPLIED_FAILED_WIRE);
    assert.equal(
      serialiseEvent(appliedInterrupt),
      CONTROL_APPLIED_INTERRUPT_WIRE,
    );
  });

  test("pins byte-identical capture.refused events with Rust", () => {
    const overBound: Event<EventPayloadMap> = {
      v: 1,
      type: CAPTURE_REFUSED,
      runId: "run-relay",
      seq: 1,
      ts: "2026-09-07T06:00:00.000Z",
      payload: {
        cause: "over_bound",
        sourceRunId: "run-source",
        sourceSeq: 8,
        sourceType: AGENT_TEXT,
        field: "AgentTextPayload.text",
        count: 20_000,
        max: 16_384,
      },
    };
    const gap: Event<EventPayloadMap> = {
      v: 1,
      type: CAPTURE_REFUSED,
      runId: "run-relay",
      seq: 2,
      ts: "2026-09-07T06:00:01.000Z",
      payload: { cause: "gap", sourceRunId: "run-source-gap" },
    };

    validate(CAPTURE_REFUSED, overBound.payload);
    assert.ok(
      overBound.payload.count !== undefined &&
        overBound.payload.max !== undefined &&
        overBound.payload.count > overBound.payload.max,
    );
    assert.equal(
      serialiseEvent(overBound),
      CAPTURE_REFUSED_OVER_BOUND_WIRE,
    );
    assert.equal(serialiseEvent(gap), CAPTURE_REFUSED_GAP_WIRE);

    for (const expected of [
      CAPTURE_REFUSED_OVER_BOUND_WIRE,
      CAPTURE_REFUSED_GAP_WIRE,
    ]) {
      assert.equal(
        serialiseEvent(parseEvent(JSON.parse(expected) as unknown)),
        expected,
      );
    }
  });

  test("pins byte-identical decision events with Rust", () => {
    const requested: Event<EventPayloadMap> = {
      v: 1,
      type: DECISION_REQUESTED,
      runId: "run-decision",
      seq: 1,
      ts: "2026-09-07T07:00:00.000Z",
      payload: {
        decisionId: "decision-1",
        kind: "permission",
        dossier: {
          question: "May the run execute the deployment tool?",
          optionsRuledOut: ["auto-proceed", "discard the request"],
          recommendedAction: "deny unless the operator confirms the target",
          blastRadius: "one repository",
          truncated: false,
        },
        options: [
          { id: "allow", label: "Allow once" },
          { id: "deny", label: "Deny" },
        ],
        subject: "deploy",
        expiresAt: "2026-09-07T07:05:00.000Z",
        onTimeout: "deny",
      },
    };
    const human: Event<EventPayloadMap> = {
      v: 1,
      type: DECISION_ANSWERED,
      runId: "run-decision",
      seq: 2,
      ts: "2026-09-07T07:01:00.000Z",
      payload: {
        decisionId: "decision-1",
        optionId: "allow",
        by: "principal:user:alice",
      },
    };
    const timeout: Event<EventPayloadMap> = {
      v: 1,
      type: DECISION_ANSWERED,
      runId: "run-decision",
      seq: 3,
      ts: "2026-09-07T07:05:00.000Z",
      payload: {
        decisionId: "decision-1",
        optionId: "deny",
        by: "principal:runtime:permission-timeout",
        byTimeout: true,
        reversal: "allow",
      },
    };

    validate(DECISION_REQUESTED, requested.payload);
    assert.equal(serialiseEvent(requested), DECISION_REQUESTED_WIRE);
    assert.equal(serialiseEvent(human), DECISION_ANSWERED_HUMAN_WIRE);
    assert.equal(serialiseEvent(timeout), DECISION_ANSWERED_TIMEOUT_WIRE);

    for (const expected of [
      DECISION_REQUESTED_WIRE,
      DECISION_ANSWERED_HUMAN_WIRE,
      DECISION_ANSWERED_TIMEOUT_WIRE,
    ]) {
      assert.equal(
        serialiseEvent(parseEvent(JSON.parse(expected) as unknown)),
        expected,
      );
    }
  });

  test("pins byte-identical decision.answered with requestedRunId with Rust", () => {
    const answer: Event<EventPayloadMap> = {
      v: 1,
      type: DECISION_ANSWERED,
      runId: "run-decision-answer",
      seq: 1,
      ts: "2026-09-07T07:10:00.000Z",
      payload: {
        decisionId: "decision-1",
        optionId: "allow",
        by: "principal:user:alice",
        requestedRunId: "run-decision",
      },
    };

    validate(DECISION_ANSWERED, answer.payload);
    assert.equal(
      serialiseEvent(answer),
      DECISION_ANSWERED_WITH_REQUESTED_RUN_WIRE,
    );
    assert.equal(
      serialiseEvent(
        parseEvent(
          JSON.parse(DECISION_ANSWERED_WITH_REQUESTED_RUN_WIRE) as unknown,
        ),
      ),
      DECISION_ANSWERED_WITH_REQUESTED_RUN_WIRE,
    );
  });

  test("pins byte-identical decision.answered with an action-id reversal with Rust", () => {
    // Pins the loosened rule (ruled on onsager-ai/ethogram#7): a `<verb>:<subject>` action id
    // is a conforming `reversal` even though it names no option this request
    // ever offered.
    const answer: Event<EventPayloadMap> = {
      v: 1,
      type: DECISION_ANSWERED,
      runId: "run-decision-revoke",
      seq: 1,
      ts: "2026-09-07T09:00:00.000Z",
      payload: {
        decisionId: "decision-revoke-1",
        optionId: "excuse:required_checks",
        by: "principal:user:alice",
        reversal: "revoke:required_checks",
      },
    };

    validate(DECISION_ANSWERED, answer.payload);
    assert.equal(
      serialiseEvent(answer),
      DECISION_ANSWERED_ACTION_REVERSAL_WIRE,
    );
    assert.equal(
      serialiseEvent(
        parseEvent(JSON.parse(DECISION_ANSWERED_ACTION_REVERSAL_WIRE) as unknown),
      ),
      DECISION_ANSWERED_ACTION_REVERSAL_WIRE,
    );
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

describe("serialiseEvent number canonicalisation (issue onsager-ai/ethogram#9)", () => {
  // Class 2 of issue onsager-ai/ethogram#9: negative zero serialises as `0`. Rust pins it in
  // `negative_zero_serialises_as_zero`; until now this side had no
  // assertion at all, which is what onsager-ai/ethogram#52 was filed for.
  //
  // The behaviour is correct today because `JSON.stringify(-0)` is `"0"`.
  // But that is a language behaviour, not a decision this SDK records, and
  // the ruling asked for the assertion specifically so the rule "does not
  // depend on a cast". `serialiseEvent` is not a thin wrapper around
  // `JSON.stringify`: it rebuilds the envelope field by field and walks the
  // payload recursively to sort keys. A future step in that walk could
  // reconstruct a number and preserve the sign with nothing to notice.

  test("negative zero serialises as 0 at every depth", () => {
    // Checked at the top level, inside a nested object, and inside an array
    // because the Rust canonicaliser is recursive: the two SDKs have to
    // agree at every depth, not only at the first one a reader tries.
    const event: Event = {
      ...completeEvent(),
      payload: {
        value: -0,
        nested: { value: -0 },
        list: [-0, 1.5, -0],
      },
    };

    assert.equal(
      serialiseEvent(event),
      '{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"list":[0,1.5,0],"nested":{"value":0},"value":0}}',
    );
  });

  test("negative zero arriving on the wire also serialises as 0", () => {
    // The realistic path rather than a hand-built payload: a producer
    // without this canonicalisation writes `-0.0` — serde_json does — and
    // this SDK reads and re-emits it. `JSON.parse` yields the double `-0`,
    // so the flattening has to survive the round trip and not merely apply
    // to a literal written in this file.
    const wire =
      '{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"value":-0.0}}';
    const parsed = parseEvent(JSON.parse(wire) as unknown);

    assert.ok(
      Object.is((parsed.payload as { value: number }).value, -0),
      "the parsed payload should still hold -0, or this test proves nothing",
    );
    assert.equal(
      serialiseEvent(parsed),
      '{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"value":0}}',
    );
  });
});

describe("ULP-neighbour differential test (short decimals)", () => {
  test("ulp neighbours of short decimals match measured JavaScript output", () => {
    // The class-4 canonicalisation on the Rust side was diff-tested against
    // real JavaScript over 200,000 randomly sampled f64 values plus an
    // exponent sweep, byte-identical, zero differences -- and it still
    // missed a real defect, because uniform random sampling over the bit
    // space almost always produces values with full-length mantissas. The
    // shape that failed was a *short decimal perturbed by about one ULP*
    // (`0.0976519` nudged by a hair), which is vanishingly rare under random
    // sampling and extremely common in real money and telemetry, since it
    // is what summing a handful of prices produces. The specific bug is
    // fixed on the Rust side (see its own guard test); this covers the
    // sampling gap that let it through, on both SDKs, independently of
    // whether that particular bug ever recurs.
    //
    // Each base below is a short, money-/telemetry-shaped decimal. For each,
    // the neighbouring doubles one and two ULPs above and below are
    // generated here via a DataView/BigUint64Array bit-pattern round trip,
    // mirroring the Rust suite's `f64::from_bits(base.to_bits() ± n)`
    // equivalent. The *expected* strings were computed once with a
    // throwaway Node script (`JSON.stringify` of each bit-shifted double)
    // and are hard-coded here and in the Rust suite, since the two suites
    // cannot share a live process to compare against a running Node. Every
    // one of the 40 values agreed between this table and what
    // `JSON.stringify` produces when the table was generated -- had any
    // disagreed, that would have been a live class-4 divergence, not a
    // table update.
    const bases = [0.0976519, 0.1, 0.3, 1.25, 12.34, 0.001, 99.99, 1234.5678];

    // [index into bases, signed ULP offset from that base, expected
    // JSON.stringify output for the resulting double]
    const expected: Array<[number, number, string]> = [
      [0, -2, "0.09765189999999997"],
      [0, -1, "0.09765189999999999"],
      [0, 0, "0.0976519"],
      [0, 1, "0.09765190000000001"],
      [0, 2, "0.09765190000000003"],
      [1, -2, "0.09999999999999998"],
      [1, -1, "0.09999999999999999"],
      [1, 0, "0.1"],
      [1, 1, "0.10000000000000002"],
      [1, 2, "0.10000000000000003"],
      [2, -2, "0.2999999999999999"],
      [2, -1, "0.29999999999999993"],
      [2, 0, "0.3"],
      [2, 1, "0.30000000000000004"],
      [2, 2, "0.3000000000000001"],
      [3, -2, "1.2499999999999996"],
      [3, -1, "1.2499999999999998"],
      [3, 0, "1.25"],
      [3, 1, "1.2500000000000002"],
      [3, 2, "1.2500000000000004"],
      [4, -2, "12.339999999999996"],
      [4, -1, "12.339999999999998"],
      [4, 0, "12.34"],
      [4, 1, "12.340000000000002"],
      [4, 2, "12.340000000000003"],
      [5, -2, "0.0009999999999999996"],
      [5, -1, "0.0009999999999999998"],
      [5, 0, "0.001"],
      [5, 1, "0.0010000000000000002"],
      [5, 2, "0.0010000000000000005"],
      [6, -2, "99.98999999999997"],
      [6, -1, "99.98999999999998"],
      [6, 0, "99.99"],
      [6, 1, "99.99000000000001"],
      [6, 2, "99.99000000000002"],
      [7, -2, "1234.5677999999996"],
      [7, -1, "1234.5677999999998"],
      [7, 0, "1234.5678"],
      [7, 1, "1234.5678000000003"],
      [7, 2, "1234.5678000000005"],
    ];

    assert.equal(
      expected.length,
      bases.length * 5,
      "table covers every base at ULP offsets -2, -1, 0, 1, 2",
    );

    const buffer = new ArrayBuffer(8);
    const view = new DataView(buffer);

    function bitsOf(value: number): bigint {
      view.setFloat64(0, value, false);
      return view.getBigUint64(0, false);
    }

    function fromBits(bits: bigint): number {
      view.setBigUint64(0, bits, false);
      return view.getFloat64(0, false);
    }

    for (const [baseIndex, offset, expectedString] of expected) {
      const base = bases[baseIndex]!;
      const bits = bitsOf(base) + BigInt(offset);
      const value = fromBits(bits);

      const event: Event = {
        ...completeEvent(),
        payload: { value },
      };

      assert.equal(
        serialiseEvent(event),
        `{"v":1,"type":"test.happened","runId":"run-1","seq":1,"ts":"2026-09-06T00:00:01.000Z","payload":{"value":${expectedString}}}`,
        `base ${base} (index ${baseIndex}) offset ${offset} expected ${expectedString}`,
      );
    }
  });
});

describe("payload tolerance (issue onsager-ai/ethogram#12)", () => {
  // Payloads are tolerant at read and retaining on forward: an unknown
  // payload field is never rejected and never dropped, so a forwarder that
  // parses a newer producer's event does not lose data silently at exactly
  // the boundary this protocol exists to cross. The envelope and required
  // fields stay strict at parse; `validate` closes the `kind` and `outcome`
  // unions — all covered elsewhere in this file.

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

  // -- A run has at most one `run.finished` (issues onsager-ai/ethogram#5 and onsager-ai/ethogram#3) ----------

  const runFinishedDraft = (): EventDraft => ({
    type: RUN_FINISHED,
    payload: { outcome: "completed", durationMs: 1 },
  });

  const agentTextDraft = (text: string): EventDraft => ({
    type: AGENT_TEXT,
    payload: { text },
  });

  test("appendDraft refuses a second run.finished", () => {
    const sink = new InMemorySink(() => "2026-09-07T00:00:00.000Z");
    sink.appendDraft("run-1", runFinishedDraft());

    assert.throws(
      () => sink.appendDraft("run-1", runFinishedDraft()),
      (error: unknown) =>
        error instanceof RunClosedError &&
        error.runId === "run-1" &&
        /already recorded a terminal event/.test(error.message),
    );
  });

  test("appendDraft refuses agent.text after run.finished", () => {
    const sink = new InMemorySink(() => "2026-09-07T00:00:00.000Z");
    sink.appendDraft("run-1", runFinishedDraft());

    assert.throws(
      () => sink.appendDraft("run-1", agentTextDraft("too late")),
      RunClosedError,
    );
  });

  test("appendEvent refuses a second run.finished, distinctly from a sequence gap", () => {
    const sink = new InMemorySink(() => "unused");
    sink.appendEvent(completeEvent());
    sink.appendEvent({
      ...completeEvent(),
      type: RUN_FINISHED,
      seq: 2,
      payload: { outcome: "completed", durationMs: 1 },
    });

    let closedError: unknown;
    try {
      sink.appendEvent({
        ...completeEvent(),
        type: AGENT_TEXT,
        seq: 3,
        payload: { text: "too late" },
      });
    } catch (error) {
      closedError = error;
    }
    assert.ok(closedError instanceof RunClosedError);
    assert.equal((closedError as RunClosedError).runId, "run-1");

    // A still-open run with the same kind of skipped seq refuses via
    // SequenceError instead: the two failure modes stay distinguishable
    // rather than one swallowing the other.
    const otherSink = new InMemorySink(() => "unused");
    otherSink.appendEvent(completeEvent());
    let gapError: unknown;
    try {
      otherSink.appendEvent({ ...completeEvent(), seq: 3 });
    } catch (error) {
      gapError = error;
    }
    assert.ok(gapError instanceof SequenceError);
    assert.notEqual(
      (closedError as Error).message,
      (gapError as Error).message,
      "a sequence gap and a closed run must report different messages",
    );
  });

  test("appendEvent refuses a gap on a closed run as RunClosedError, not SequenceError", () => {
    // Once a run is closed, *any* further append is refused as
    // RunClosedError — even one that also happens to skip a seq. The
    // closed-run check runs first, so this is not misreported as a gap.
    const sink = new InMemorySink(() => "unused");
    sink.appendEvent(completeEvent());
    sink.appendEvent({
      ...completeEvent(),
      type: RUN_FINISHED,
      seq: 2,
      payload: { outcome: "completed", durationMs: 1 },
    });

    assert.throws(
      () =>
        sink.appendEvent({
          ...completeEvent(),
          type: AGENT_TEXT,
          seq: 99,
          payload: { text: "too late" },
        }),
      RunClosedError,
    );
  });

  test("appendEvent refuses a real control.applied after run.finished, not a sequence gap", () => {
    // A `control.applied` sounds like the one post-terminal event that
    // "surely" should still be recordable — an interrupt landing just after
    // the run ends. Ruled on onsager-ai/umwelt#1: a closed run accepts nothing after
    // run.finished, control events included, and this is refused the same
    // way as any other post-terminal append: as RunClosedError, not
    // SequenceError, even though this append's seq is otherwise the
    // expected next value.
    const sink = new InMemorySink(() => "unused");
    sink.appendEvent(completeEvent());
    sink.appendEvent({
      ...completeEvent(),
      type: RUN_FINISHED,
      seq: 2,
      payload: { outcome: "completed", durationMs: 1 },
    });

    const before = sink.events("run-1");

    let closedError: unknown;
    try {
      sink.appendEvent({
        ...completeEvent(),
        type: CONTROL_APPLIED,
        seq: 3,
        payload: { controlId: "control-1", ok: true },
      });
    } catch (error) {
      closedError = error;
    }

    assert.ok(closedError instanceof RunClosedError);
    assert.ok(!(closedError instanceof SequenceError));
    assert.equal((closedError as RunClosedError).runId, "run-1");

    assert.deepEqual(sink.events("run-1"), before);
    assert.equal(
      sink.events("run-1").length,
      2,
      "a refused control.applied append must not consume a seq",
    );
  });

  test("a refused append leaves stored events and seq unchanged", () => {
    const sink = new InMemorySink(() => "2026-09-07T00:00:00.000Z");
    sink.appendDraft("run-1", runFinishedDraft());

    const before = sink.events("run-1");
    assert.equal(before.length, 1);

    assert.throws(() => sink.appendDraft("run-1", agentTextDraft("too late")));
    assert.throws(() =>
      sink.appendEvent({
        ...completeEvent(),
        type: AGENT_TEXT,
        seq: 2,
        payload: { text: "also too late" },
      }),
    );

    const after = sink.events("run-1");
    assert.deepEqual(after, before);
    assert.equal(after.length, 1, "a refused append must not consume a seq");

    // The seq counter, not just the event count, is unchanged: proving that
    // a fresh run's next draft still takes seq 2 confirms the refused
    // appends above never advanced any shared counting state (this run
    // stays closed, so it cannot itself accept a "next legitimate" append).
    const otherSink = new InMemorySink(() => "2026-09-07T00:00:00.000Z");
    otherSink.appendDraft("run-2", agentTextDraft("first"));
    const second = otherSink.appendDraft("run-2", agentTextDraft("second"));
    assert.equal(second.seq, 2);
  });

  test("closing one run does not close another", () => {
    const sink = new InMemorySink(() => "2026-09-07T00:00:00.000Z");
    sink.appendDraft("run-1", runFinishedDraft());

    assert.throws(() => sink.appendDraft("run-1", agentTextDraft("too late")));
    assert.doesNotThrow(() =>
      sink.appendDraft("run-2", agentTextDraft("fine")),
    );
    assert.equal(sink.events("run-2").length, 1);
  });

  test("a run without run.finished keeps accepting appends normally", () => {
    const sink = new InMemorySink(() => "2026-09-07T00:00:00.000Z");
    sink.appendDraft("run-1", agentTextDraft("one"));
    sink.appendDraft("run-1", agentTextDraft("two"));
    const third = sink.appendDraft("run-1", agentTextDraft("three"));

    assert.equal(third.seq, 3);
    assert.equal(sink.events("run-1").length, 3);
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

describe("known event type constants (issue onsager-ai/ethogram#4)", () => {
  const sink = new InMemorySink(() => "2026-09-07T03:00:00.000Z");

  // Builds an event of `type` from `payload`, stamps it, serialises it, and
  // parses the `type` field back out. This is deliberately not
  // `assert.equal(RUN_STARTED, "run.started")`: that proves only that
  // someone typed the same string twice. Going through the wire fails if the
  // exported constant and what a real event of that type actually produces
  // ever part company.
  //
  // Each call uses its own run id (rather than sharing "run-known-types"
  // across every type) because one of these types is RUN_FINISHED itself: a
  // shared run would close after that call and refuse every following one
  // (issues onsager-ai/ethogram#5 and onsager-ai/ethogram#3), which would make this test about sink refusal rather
  // than about the round-trip it means to check.
  const roundTrippedType = (type: string, payload: unknown): string => {
    const stamped = sink.appendDraft(`run-known-types-${type}`, {
      type,
      payload,
    } as EventDraft);
    const wire = serialiseEvent(stamped);
    return parseEvent(JSON.parse(wire) as unknown).type;
  };

  test("each exported constant equals the type field its own round-trip produces", () => {
    assert.equal(
      roundTrippedType(RUN_STARTED, {
        kind: "loop",
        actor: "builder",
        harness: "codex",
      }),
      RUN_STARTED,
    );
    assert.equal(
      roundTrippedType(RUN_FINISHED, { outcome: "completed", durationMs: 1250 }),
      RUN_FINISHED,
    );
    assert.equal(roundTrippedType(AGENT_STARTED, {}), AGENT_STARTED);
    assert.equal(roundTrippedType(AGENT_TEXT, { text: "hello" }), AGENT_TEXT);
    assert.equal(
      roundTrippedType(AGENT_TOOL_USE, { tool: "read" }),
      AGENT_TOOL_USE,
    );
    assert.equal(
      roundTrippedType(AGENT_TOOL_RESULT, { tool: "read" }),
      AGENT_TOOL_RESULT,
    );
    assert.equal(roundTrippedType(AGENT_COMPLETED, {}), AGENT_COMPLETED);
    assert.equal(
      roundTrippedType(AGENT_WARNING, { message: "warning" }),
      AGENT_WARNING,
    );
    assert.equal(
      roundTrippedType(CONTROL_REQUESTED, {
        controlId: "control-1",
        kind: "steer",
        by: "operator",
      }),
      CONTROL_REQUESTED,
    );
    assert.equal(
      roundTrippedType(CONTROL_APPLIED, { controlId: "control-1", ok: true }),
      CONTROL_APPLIED,
    );
    assert.equal(
      roundTrippedType(CAPTURE_REFUSED, {
        cause: "gap",
        sourceRunId: "run-source",
      }),
      CAPTURE_REFUSED,
    );
    assert.equal(
      roundTrippedType(DECISION_REQUESTED, {
        decisionId: "decision-1",
        kind: "permission",
        dossier: {
          question: "Proceed?",
          optionsRuledOut: [],
          recommendedAction: "ask",
          blastRadius: "one run",
        },
        options: [],
      }),
      DECISION_REQUESTED,
    );
    assert.equal(
      roundTrippedType(DECISION_ANSWERED, {
        decisionId: "decision-1",
        optionId: "allow",
        by: "principal:user:alice",
      }),
      DECISION_ANSWERED,
    );
  });

  test("KNOWN_TYPES holds exactly the thirteen recognised types, with no duplicates", () => {
    assert.equal(KNOWN_TYPES.length, 13);
    assert.equal(new Set(KNOWN_TYPES).size, 13);
    assert.deepEqual(
      new Set(KNOWN_TYPES),
      new Set([
        RUN_STARTED,
        RUN_FINISHED,
        AGENT_STARTED,
        AGENT_TEXT,
        AGENT_TOOL_USE,
        AGENT_TOOL_RESULT,
        AGENT_COMPLETED,
        AGENT_WARNING,
        CONTROL_REQUESTED,
        CONTROL_APPLIED,
        CAPTURE_REFUSED,
        DECISION_REQUESTED,
        DECISION_ANSWERED,
      ]),
    );
  });

  test("every KNOWN_TYPES entry uses the parsing path, and an unrecognised type does not", () => {
    // A string payload fails `isRecord` in every known payload parser, so
    // this distinguishes "parsed through a typed payload" from the
    // untouched pass-through an unrecognised type gets.
    const malformedPayload = "not-an-object";
    for (const type of KNOWN_TYPES) {
      assert.throws(
        () => parseEvent({ ...completeEvent(), type, payload: malformedPayload }),
        `${type} should be parsed through its typed payload`,
      );
    }
    assert.doesNotThrow(() =>
      parseEvent({
        ...completeEvent(),
        type: "future.happened",
        payload: malformedPayload,
      }),
    );
  });
});

describe("consumer rule stated on every retaining union (issue onsager-ai/ethogram#54)", () => {
  // Discover unions from their own declaration shape, `export type X =
  // KnownX | (string & {})` (`\s*` spans the line breaks the actual source
  // sometimes wraps this in, such as CaptureRefusalCause's multi-line form),
  // rather than from a hand-written list — a hand-maintained set has been
  // wrong twice in this repository. `KnownType` (the event-type union) is
  // not one of these six and is deliberately not special-cased here: its
  // declaration is `(typeof KNOWN_TYPES)[number]`, which has neither a
  // `KnownKnownType` reference nor `(string & {})`, so this shape already
  // excludes it without help.
  const UNION_PATTERN =
    /export type (\w+) =\s*\|?\s*Known\1\s*\|\s*\(string\s*&\s*\{\}\);/g;

  // TypeScript doc comments are `/** ... */` blocks: drop the delimiters and
  // each line's leading `*`, then join with spaces so a phrase split across
  // the comment's own line wraps still reads as one contiguous string for
  // the marker check below.
  function normaliseComment(comment: string): string {
    return comment
      .replace(/^\/\*\*/, "")
      .replace(/\*\/$/, "")
      .split("\n")
      .map((line) => line.trim().replace(/^\*/, "").trim())
      .join(" ");
  }

  function docCommentBefore(source: string, declarationStart: number): string {
    const before = source.slice(0, declarationStart).trimEnd();
    if (!before.endsWith("*/")) {
      return "";
    }
    const start = before.lastIndexOf("/**");
    return start === -1 ? "" : before.slice(start);
  }

  test("every union discovered by its retaining shape states the consumer rule", async () => {
    const source = await readFile(
      join(dirname(fileURLToPath(import.meta.url)), "index.ts"),
      "utf8",
    );
    const matches = [...source.matchAll(UNION_PATTERN)];
    assert.ok(matches.length > 0, "source scan found no retaining unions");

    // A scan that goes blind fails open: only the unions matched above are
    // ever checked, so a declaration reflowed out of the pattern's reach
    // would go unchecked and this test would still pass. `(string & {})` is
    // the retaining shape itself and appears exactly once per retaining
    // union, so counting it is a second, looser scan of the same file: if
    // the two disagree, the stricter pattern has stopped seeing a union
    // rather than a union having been removed.
    const retainingShapes = source.match(/\(string\s*&\s*\{\}\)/g) ?? [];
    assert.equal(
      matches.length,
      retainingShapes.length,
      `the union pattern matched ${matches.length} declarations but the file contains ${retainingShapes.length} retaining shapes; the scan has gone blind to one`,
    );

    // Same marker as the Rust scan, and the same reason: matching the whole
    // ruled sentence would make this test a formatting assertion that fails
    // on the first reflow. "never as a default" is the rule's acting clause,
    // appears nowhere else in index.ts today, and this revision only ever
    // writes it as part of the full three-clause sentence, so its presence
    // stands in for that sentence without pinning its exact wording.
    const MARKER = "never as a default";

    const missing = matches
      .filter(
        (match) =>
          !normaliseComment(docCommentBefore(source, match.index ?? 0)).includes(
            MARKER,
          ),
      )
      .map((match) => match[1]);

    assert.deepEqual(
      missing,
      [],
      `unions missing the consumer rule marker (${JSON.stringify(MARKER)}) in their doc comment: ${missing.join(", ")}`,
    );
  });
});

describe("every run kind carries a definition (issue onsager-ai/ethogram#64)", () => {
  // Each member is decided by a fact a producer can check rather than by what
  // its name suggests, and the definition lives on the union a consumer meets
  // rather than only in the README. The marker is the phrase introducing that
  // fact, not the whole sentence: matching the sentence would make this a
  // formatting assertion someone deletes the first time a reflow breaks it.
  const MARKER = "Decided by";

  test("every RUN_KINDS member has a defining bullet in the RunKind doc", async () => {
    const source = await readFile(
      join(dirname(fileURLToPath(import.meta.url)), "index.ts"),
      "utf8",
    );

    // The member list comes from the exported constant at run time, not from
    // a scan of the source, so it cannot go blind and quietly check fewer
    // members than exist. What can go blind is finding the doc comment, so
    // that is asserted before anything is read out of it.
    const declaration = source.indexOf("export type RunKind = KnownRunKind");
    assert.ok(declaration !== -1, "RunKind declaration not found in index.ts");
    const before = source.slice(0, declaration).trimEnd();
    const start = before.lastIndexOf("/**");
    assert.ok(
      start !== -1 && before.endsWith("*/"),
      "RunKind carries no doc comment to read definitions out of",
    );
    const doc = before.slice(start);

    assert.ok(RUN_KINDS.length > 0, "RUN_KINDS is empty");
    const undefined_ = RUN_KINDS.filter((kind) => {
      const bullet = doc.indexOf(`\`"${kind}"\``);
      if (bullet === -1) {
        return true;
      }
      // Read to the next bullet, so a member cannot borrow the marker from
      // the one after it.
      const next = doc.indexOf("\n * - ", bullet);
      return !doc
        .slice(bullet, next === -1 ? undefined : next)
        .includes(MARKER);
    });

    assert.deepEqual(
      undefined_,
      [],
      `run kinds without a definition (${JSON.stringify(MARKER)} in their own bullet): ${undefined_.join(", ")}; every member is decided by a producer-checkable fact (onsager-ai/ethogram#64)`,
    );
  });
});
