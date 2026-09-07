import assert from "node:assert/strict";
import { test } from "node:test";
import {
  AGENT_COMPLETED, AGENT_TEXT, AGENT_TOOL_RESULT, AGENT_TOOL_USE, AGENT_WARNING,
  CAPTURE_REFUSED, CONTROL_APPLIED, CONTROL_REQUESTED, DECISION_ANSWERED,
  DECISION_REQUESTED, MAX_EXCERPT_SCALARS, MAX_PAYLOAD_BYTES, MAX_TEXT_SCALARS,
  RUN_FINISHED, RUN_STARTED, ValidationError, serialiseValidationError, validate,
} from "./index.js";

function failure(type: string, payload: unknown): ValidationError {
  try {
    validate(type, payload);
  } catch (error) {
    assert.ok(error instanceof ValidationError);
    return error;
  }
  assert.fail("expected a validation failure");
}

function request() {
  return {
    decisionId: "d", kind: "permission",
    dossier: {
      question: "?", optionsRuledOut: ["no"],
      recommendedAction: "ask", blastRadius: "one run",
    },
    options: [{ id: "allow", label: "Allow" }],
  };
}

test("OverBound reports capture and universal paths and scalar counts", () => {
  const over = "😀".repeat(MAX_EXCERPT_SCALARS + 1);
  const cases: [string, unknown, string, number][] = [
    [RUN_FINISHED, { outcome: "completed", durationMs: 1, reason: over }, "payload.reason", MAX_EXCERPT_SCALARS],
    [AGENT_TOOL_USE, { tool: "t", inputExcerpt: over }, "payload.inputExcerpt", MAX_EXCERPT_SCALARS],
    [AGENT_TOOL_RESULT, { tool: "t", resultExcerpt: over }, "payload.resultExcerpt", MAX_EXCERPT_SCALARS],
    [AGENT_WARNING, { message: over }, "payload.message", MAX_EXCERPT_SCALARS],
    [CONTROL_REQUESTED, { controlId: "c", kind: "steer", by: "a", text: over }, "payload.text", MAX_EXCERPT_SCALARS],
    [CONTROL_APPLIED, { controlId: "c", ok: false, reason: over }, "payload.reason", MAX_EXCERPT_SCALARS],
    [CAPTURE_REFUSED, { cause: "malformed", sourceRunId: "r", detail: over }, "payload.detail", MAX_EXCERPT_SCALARS],
    [AGENT_TEXT, { text: "😀".repeat(MAX_TEXT_SCALARS + 1) }, "payload.text", MAX_TEXT_SCALARS],
    [AGENT_TEXT, { text: "ok", extra: [{ note: "😀".repeat(MAX_TEXT_SCALARS + 1) }] }, "payload.extra[0].note", MAX_TEXT_SCALARS],
    ["future.happened", "😀".repeat(MAX_TEXT_SCALARS + 1), "payload", MAX_TEXT_SCALARS],
  ];
  for (const field of ["question", "recommendedAction", "blastRadius"] as const) {
    const payload = request();
    payload.dossier[field] = over;
    cases.push([DECISION_REQUESTED, payload, `payload.dossier.${field}`, MAX_EXCERPT_SCALARS]);
  }
  const ruledOut = request();
  ruledOut.dossier.optionsRuledOut[0] = over;
  cases.push([DECISION_REQUESTED, ruledOut, "payload.dossier.optionsRuledOut[0]", MAX_EXCERPT_SCALARS]);
  const option = request();
  option.options[0]!.label = over;
  cases.push([DECISION_REQUESTED, option, "payload.options[0].label", MAX_EXCERPT_SCALARS]);
  for (const [type, payload, path, max] of cases) {
    assert.deepEqual(failure(type, payload).details, { kind: "OverBound", path, count: max + 1, max });
  }
});

test("PayloadTooLarge counts canonical UTF-8 payload bytes", () => {
  const error = failure("future.happened", ["😀".repeat(MAX_TEXT_SCALARS), "😀".repeat(MAX_TEXT_SCALARS)]);
  assert.deepEqual(error.details, { kind: "PayloadTooLarge", bytes: 131079, max: MAX_PAYLOAD_BYTES });
  assert.equal(serialiseValidationError(error), '{"bytes":131079,"kind":"PayloadTooLarge","max":131072}');
});

test("UnknownMember reports each closed union and retains the value", () => {
  const decision = request();
  decision.kind = 'unknown"😀';
  const cases: [string, unknown, string][] = [
    [RUN_STARTED, { kind: 'unknown"😀', actor: "a", harness: "h" }, "payload.kind"],
    [RUN_FINISHED, { outcome: 'unknown"😀', durationMs: 1 }, "payload.outcome"],
    [CONTROL_REQUESTED, { controlId: "c", kind: 'unknown"😀', by: "a" }, "payload.kind"],
    [CAPTURE_REFUSED, { cause: 'unknown"😀', sourceRunId: "r" }, "payload.cause"],
    [DECISION_REQUESTED, decision, "payload.kind"],
  ];
  for (const [type, payload, path] of cases) {
    assert.deepEqual(failure(type, payload).details, { kind: "UnknownMember", path, value: 'unknown"😀' });
  }
});

test("MissingField reports required fields including nested and array paths", () => {
  const cases: [string, Record<string, unknown>, string[]][] = [
    [RUN_STARTED, { kind: "loop", actor: "a", harness: "h" }, ["kind", "actor", "harness"]],
    [RUN_FINISHED, { outcome: "completed", durationMs: 1 }, ["outcome", "durationMs"]],
    [AGENT_TEXT, { text: "x" }, ["text"]],
    [AGENT_TOOL_USE, { tool: "t" }, ["tool"]],
    [AGENT_TOOL_RESULT, { tool: "t" }, ["tool"]],
    [AGENT_WARNING, { message: "m" }, ["message"]],
    [CONTROL_REQUESTED, { controlId: "c", kind: "interrupt", by: "a" }, ["controlId", "kind", "by"]],
    [CONTROL_APPLIED, { controlId: "c", ok: true }, ["controlId", "ok"]],
    [CAPTURE_REFUSED, { cause: "gap", sourceRunId: "r" }, ["cause", "sourceRunId"]],
    [DECISION_REQUESTED, request(), ["decisionId", "kind", "dossier", "options"]],
    [DECISION_ANSWERED, { decisionId: "d", optionId: "allow", by: "a" }, ["decisionId", "optionId", "by"]],
  ];
  for (const [type, payload, fields] of cases) {
    for (const field of fields) {
      const candidate = { ...payload };
      delete candidate[field];
      assert.deepEqual(failure(type, candidate).details, { kind: "MissingField", path: `payload.${field}` });
    }
  }
  for (const field of ["question", "optionsRuledOut", "recommendedAction", "blastRadius"]) {
    const payload = request();
    delete (payload.dossier as Record<string, unknown>)[field];
    assert.deepEqual(failure(DECISION_REQUESTED, payload).details, { kind: "MissingField", path: `payload.dossier.${field}` });
  }
  for (const field of ["id", "label"]) {
    const payload = request();
    delete (payload.options[0] as Record<string, unknown>)[field];
    assert.deepEqual(failure(DECISION_REQUESTED, payload).details, { kind: "MissingField", path: `payload.options[0].${field}` });
  }
});

test("Policy reports steer and all three timeout rules", () => {
  const cases: [string, unknown, string, string][] = [];
  for (const text of [undefined, ""]) {
    cases.push([CONTROL_REQUESTED, { controlId: "c", kind: "steer", by: "a", ...(text === undefined ? {} : { text }) }, "payload.text", 'ControlRequestedPayload.text is required and must not be empty when kind is "steer": a steer with nothing to say is a producer error']);
  }
  for (const [kind, onTimeout, message] of [
    ["tripwire", "deny", 'DecisionRequestedPayload.onTimeout is permitted only when kind is "permission"; received kind "tripwire"'],
    ["permission", "allow", 'DecisionRequestedPayload.onTimeout must be "deny" when kind is "permission"; received "allow"'],
    ["permission", "deny", 'DecisionRequestedPayload.onTimeout must name one of the request\'s options[].id; received "deny"'],
  ] as const) {
    cases.push([DECISION_REQUESTED, { ...request(), kind, onTimeout }, "payload.onTimeout", message]);
  }
  for (const [type, payload, path, message] of cases) {
    assert.deepEqual(failure(type, payload).details, { kind: "Policy", path, message });
  }
});

test("Malformed retains a path and the original diagnostic for representation failures", () => {
  const cases: [string, unknown, string, string][] = [
    [AGENT_COMPLETED, { sessionId: 7 }, "payload.sessionId", "AgentCompletedPayload.sessionId must be a string when present"],
    [AGENT_COMPLETED, { usage: { inputTokens: -1 } }, "payload.usage.inputTokens", "AgentCompletedPayload.usage.inputTokens must be a non-negative safe integer when present"],
    [AGENT_COMPLETED, { extra: [9007199254740992] }, "payload.extra[0]", "payload.extra[0] is an integral number whose magnitude exceeds the safe integer bound: actual 9007199254740992; maximum 9007199254740991; a value that needs more precision must be carried as a string"],
    [AGENT_TEXT, false, "payload", "AgentTextPayload must be an object"],
  ];
  for (const [type, payload, path, message] of cases) {
    assert.deepEqual(failure(type, payload).details, { kind: "Malformed", path, message });
  }
  const error = failure("future.happened", { value: 1n });
  assert.equal(error.kind, "Malformed");
  assert.equal(error.message, "Do not know how to serialize a BigInt");
});

test("Policy still reports only the stated rules after the Malformed split", () => {
  // Pinned in both directions: the previous test asserts the representation
  // failures that moved to Malformed; this one asserts that the stated-rule
  // violations stay Policy.
  const steer = failure(CONTROL_REQUESTED, { controlId: "c", kind: "steer", by: "a" });
  assert.equal(steer.kind, "Policy");

  const onTimeout = failure(DECISION_REQUESTED, { ...request(), kind: "tripwire", onTimeout: "deny" });
  assert.equal(onTimeout.kind, "Policy");
});

test("ValidationError keeps TypeError inheritance and its existing name", () => {
  const error = failure(AGENT_TEXT, {});
  assert.ok(error instanceof TypeError);
  assert.ok(error instanceof ValidationError);
  assert.equal(error.name, "TypeError");
  assert.throws(() => validate(AGENT_TEXT, {}), TypeError);
  assert.throws(() => validate(AGENT_TEXT, {}), new TypeError("AgentTextPayload is missing required field: text"));
  // Compile-time narrowing gives a sink the exact fields for this kind.
  const details = error.details;
  if (details.kind === "MissingField") {
    const path: string = details.path;
    assert.equal(path, "payload.text");
  } else {
    assert.fail("expected MissingField");
  }
});

test("error serialisation sorts keys and omits stack and compatibility metadata", () => {
  const error = failure(AGENT_TEXT, { text: "😀".repeat(MAX_TEXT_SCALARS + 1) });
  assert.equal(serialiseValidationError(error), '{"count":16385,"kind":"OverBound","max":16384,"path":"payload.text"}');
  assert.deepEqual(JSON.parse(JSON.stringify(error)), error.details);
  assert.equal(serialiseValidationError(failure(AGENT_TEXT, {})), '{"kind":"MissingField","path":"payload.text"}');
});
