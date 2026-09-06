import assert from "node:assert/strict";
import { describe, test } from "node:test";

import {
  EVENT_SCHEMA_VERSION,
  InMemorySink,
  parseEvent,
  serialiseEvent,
  stamp,
  type Event,
  type EventDraft,
} from "./index.js";

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
});

describe("stamp", () => {
  test("retains future payload-map correlation", () => {
    assertFuturePayloadCorrelation({
      type: "test.happened",
      payload: { ok: true },
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
