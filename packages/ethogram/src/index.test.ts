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
