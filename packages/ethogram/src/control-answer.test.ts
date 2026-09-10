import assert from "node:assert/strict";
import { test } from "node:test";
import {
  CONTROL_APPLIED, CONTROL_APPLIED_REASONS, CONTROL_REQUESTED,
  MAX_EXCERPT_SCALARS, ValidationError, parseControlAppliedPayload,
  parseControlRequestedPayload, parseEvent, serialiseEvent, stamp, validate,
  type ControlAppliedReason, type ControlRequestedPayload, type Event,
  type EventDraft, type EventPayloadMap,
} from "./index.js";

// Written once and pasted identically into control_answer.rs. These are
// handwritten agreement examples, not captured corpus fixtures (onsager-ai/ethogram#38).
const CONTROL_ANSWER_REQUESTED_WIRE =
  '{"v":1,"type":"control.requested","runId":"run-control-answer","seq":1,"ts":"2026-09-08T00:00:00.000Z","payload":{"by":"principal:user:alice","controlId":"control-answer-1","decisionId":"decision-1","kind":"answer","optionId":"allow"}}';
const CONTROL_ANSWER_APPLIED_WIRE =
  '{"v":1,"type":"control.applied","runId":"run-control-answer","seq":2,"ts":"2026-09-08T00:00:00.000Z","payload":{"controlId":"control-answer-1","ok":true}}';
const CONTROL_NO_SUCH_DECISION_WIRE =
  '{"v":1,"type":"control.applied","runId":"run-control-answer","seq":3,"ts":"2026-09-08T00:00:00.000Z","payload":{"controlId":"control-answer-1","ok":false,"reason":"no-such-decision"}}';
const CONTROL_ALREADY_ANSWERED_WIRE =
  '{"v":1,"type":"control.applied","runId":"run-control-answer","seq":4,"ts":"2026-09-08T00:00:00.000Z","payload":{"controlId":"control-answer-1","ok":false,"reason":"already-answered"}}';
const CONTROL_OPTION_NOT_OFFERED_WIRE =
  '{"v":1,"type":"control.applied","runId":"run-control-answer","seq":5,"ts":"2026-09-08T00:00:00.000Z","payload":{"controlId":"control-answer-1","ok":false,"reason":"option-not-offered"}}';

function event(draft: EventDraft<EventPayloadMap>, seq = 1): Event<EventPayloadMap> {
  return stamp(draft, { runId: "run-control-answer", seq, ts: "2026-09-08T00:00:00.000Z" });
}

function answer(): ControlRequestedPayload {
  return {
    controlId: "control-answer-1", kind: "answer", decisionId: "decision-1",
    optionId: "allow", by: "principal:user:alice",
  };
}

function failure(type: string, payload: unknown): ValidationError {
  try { validate(type, payload); } catch (error) {
    assert.ok(error instanceof ValidationError);
    return error;
  }
  assert.fail("expected validation to fail");
}

function assertParseable(type: string, payload: unknown): void {
  const input = stamp({ type, payload }, { runId: "r", seq: 1, ts: "t" });
  const wire = serialiseEvent(input);
  assert.equal(serialiseEvent(parseEvent(JSON.parse(wire))), wire);
}

test("answer events match the Rust pinned bytes", () => {
  const requested = event({ type: CONTROL_REQUESTED, payload: answer() });
  validate(CONTROL_REQUESTED, requested.payload);
  assert.equal(serialiseEvent(requested), CONTROL_ANSWER_REQUESTED_WIRE);
  assert.deepEqual(parseControlRequestedPayload(JSON.parse(CONTROL_ANSWER_REQUESTED_WIRE).payload), answer());
  assert.equal(serialiseEvent(parseEvent(JSON.parse(CONTROL_ANSWER_REQUESTED_WIRE))), CONTROL_ANSWER_REQUESTED_WIRE);

  const cases: [number, ControlAppliedReason | undefined, string][] = [
    [2, undefined, CONTROL_ANSWER_APPLIED_WIRE],
    [3, "no-such-decision", CONTROL_NO_SUCH_DECISION_WIRE],
    [4, "already-answered", CONTROL_ALREADY_ANSWERED_WIRE],
    [5, "option-not-offered", CONTROL_OPTION_NOT_OFFERED_WIRE],
  ];
  for (const [seq, reason, wire] of cases) {
    const applied = event({ type: CONTROL_APPLIED, payload: {
      controlId: "control-answer-1", ok: reason === undefined,
      ...(reason === undefined ? {} : { reason }),
    } }, seq);
    validate(CONTROL_APPLIED, applied.payload);
    assert.equal(serialiseEvent(applied), wire);
    assert.deepEqual(parseControlAppliedPayload(JSON.parse(wire).payload), applied.payload);
    assert.equal(serialiseEvent(parseEvent(JSON.parse(wire))), wire);
  }
});

test("answer IDs are required only at validation", () => {
  for (const field of ["decisionId", "optionId"] as const) {
    const payload = answer();
    delete payload[field];
    assertParseable(CONTROL_REQUESTED, payload);
    assert.deepEqual(failure(CONTROL_REQUESTED, payload).details,
      { kind: "MissingField", path: `payload.${field}` });
  }
});

test("fields forbidden by the control kind are Policy errors", () => {
  // Deliberately known kinds only: an unfamiliar kind such as "teleport"
  // reports UnknownMember before this field-forbidden rule is ever reached,
  // exactly like every other closed union checks membership before any
  // kind-conditioned rule (see "unfamiliar control kind retains exact bytes
  // and validate reports it" below for the unfamiliar-kind coverage).
  for (const kind of ["interrupt", "steer"]) {
    for (const field of ["decisionId", "optionId"]) {
      const payload = { controlId: "c", kind, by: "a", text: "next turn", [field]: "" };
      assertParseable(CONTROL_REQUESTED, payload);
      assert.deepEqual(failure(CONTROL_REQUESTED, payload).details, {
        kind: "Policy", path: `payload.${field}`,
        message: `ControlRequestedPayload.${field} is permitted only when kind is "answer"`,
      });
    }
  }
  for (const text of ["", "next turn"]) {
    const payload = { ...answer(), text };
    assertParseable(CONTROL_REQUESTED, payload);
    assert.deepEqual(failure(CONTROL_REQUESTED, payload).details, {
      kind: "Policy", path: "payload.text",
      message: 'ControlRequestedPayload.text must be absent when kind is "answer"',
    });
  }
});

test("a known spelling always receives the known control kind rules", () => {
  // TS unions are primitive strings at runtime, with no separate Unknown
  // constructor. Widening a known literal cannot bypass its field policy.
  const kind: string = "answer";
  assert.deepEqual(failure(CONTROL_REQUESTED,
    { controlId: "c", kind, by: "a" }).details,
    { kind: "MissingField", path: "payload.decisionId" });
  const wrapped = failure(CONTROL_REQUESTED,
    { controlId: "c", kind: { Unknown: "answer" }, by: "a" });
  assert.equal(wrapped.kind, "Malformed");
  assert.ok("path" in wrapped.details);
  assert.equal(wrapped.details.path, "payload.kind");
});

test("unfamiliar control kind retains exact bytes and validate reports it", () => {
  const raw = 'future/答😀 e\u0301\n"';
  const payload = { controlId: "c", kind: raw, by: "a" };
  assert.deepEqual(failure(CONTROL_REQUESTED, payload).details, {
    kind: "UnknownMember", path: "payload.kind", value: raw,
  });
  // Reporting the unfamiliar kind at validation does not stop it from
  // parsing and round-tripping byte-for-byte -- that is parseEvent's
  // concern, not validate's.
  const wire = serialiseEvent(event({ type: CONTROL_REQUESTED, payload }));
  const parsed = parseControlRequestedPayload(parseEvent(JSON.parse(wire)).payload);
  assert.deepEqual(Buffer.from(parsed.kind), Buffer.from(raw));
  assert.equal(serialiseEvent(parseEvent(JSON.parse(wire))), wire);
  assert.deepEqual(failure(CONTROL_REQUESTED, parsed).details, {
    kind: "UnknownMember", path: "payload.kind", value: raw,
  });
});

test("reason is required for a negative echo and optional for a positive one", () => {
  const missing = { controlId: "c", ok: false };
  assertParseable(CONTROL_APPLIED, missing);
  assert.deepEqual(failure(CONTROL_APPLIED, missing).details,
    { kind: "MissingField", path: "payload.reason" });
  for (const payload of [
    { controlId: "c", ok: true },
    { controlId: "c", ok: true, reason: "accepted by runtime" },
    { controlId: "c", ok: true, reason: "rejected" },
  ]) { validate(CONTROL_APPLIED, payload); }
});

test("all six reasons and unknown prose round-trip", () => {
  const reasons = ["no-such-decision", "already-answered", "option-not-offered",
    "unsupported", "not-live", "rejected"] as const;
  assert.deepEqual(CONTROL_APPLIED_REASONS, reasons);
  for (const reason of [...reasons, 'future/答😀 e\u0301\n"']) {
    const payload = { controlId: "c", ok: false, reason };
    validate(CONTROL_APPLIED, payload);
    const wire = serialiseEvent(event({ type: CONTROL_APPLIED, payload }));
    const parsed = parseControlAppliedPayload(parseEvent(JSON.parse(wire)).payload);
    assert.deepEqual(Buffer.from(parsed.reason!), Buffer.from(reason));
    assert.equal(serialiseEvent(parseEvent(JSON.parse(wire))), wire);
  }
});

test("unknown reason keeps the excerpt bound for both echo results", () => {
  for (const ok of [false, true]) {
    for (const count of [MAX_EXCERPT_SCALARS, MAX_EXCERPT_SCALARS + 1]) {
      const payload = { controlId: "c", ok, reason: "😀".repeat(count) };
      assertParseable(CONTROL_APPLIED, payload);
      if (count === MAX_EXCERPT_SCALARS) { validate(CONTROL_APPLIED, payload); } else {
        assert.deepEqual(failure(CONTROL_APPLIED, payload).details,
          { kind: "OverBound", path: "payload.reason", count, max: MAX_EXCERPT_SCALARS });
      }
    }
  }
});

test("new optional strings reject null and wrong types at parse", () => {
  const cases: [string, object, string[]][] = [
    [CONTROL_REQUESTED, answer(), ["decisionId", "optionId"]],
    [CONTROL_APPLIED, { controlId: "c", ok: true }, ["reason"]],
  ];
  for (const [type, base, fields] of cases) {
    for (const field of fields) {
      for (const invalid of [null, 7, { Unknown: "answer" }]) {
        const payload = { ...base, [field]: invalid };
        const input = stamp({ type, payload }, { runId: "r", seq: 1, ts: "t" });
        assert.throws(() => parseEvent(input), TypeError);
        const error = failure(type, payload);
        assert.equal(error.kind, "Malformed");
        assert.ok("path" in error.details);
        assert.equal(error.details.path, `payload.${field}`);
      }
    }
  }
});
