# Conformance corpus

Canonical event fixtures. **Both SDKs must agree on the compact canonical
serialisation of every fixture in this directory**, and both CIs run the corpus.

This is the mechanism behind principle 1. Two hand-written implementations in
different languages are only one definition if something mechanical proves they
agree; this directory is that proof, and it is why neither SDK is generated
from the other.

## Rules

- One fixture is one JSON document holding one complete, sink-stamped `Event`
  envelope. A fixture set may also retain the `EventDraft` from which an event
  was stamped, allowing an SDK to prove that stamping reproduces the event
  modulo the sink-owned `seq` and `ts` values.
- **This corpus is empty until the vocabulary is extracted.** A fixture must
  accompany the first real event, not precede it: a fixture written before the
  vocabulary is settled is a guess, and the immutability rule below would then
  preserve the guess forever.
- Fixtures are grouped by schema version: `v1/`, `v2/`, and so on. Version 1 is
  the first protocol version; `v0/` never exists.
- A fixture is **immutable once published**. Correcting a fixture changes what
  agreement means, retroactively, in both SDKs at once. Add a new one instead.
- Every payload variant gets at least one fixture. A payload with no fixture is
  a payload the two implementations have never been shown to agree about.
- Narration fields carry placeholder content only. The corpus is public and
  permanent; real transcripts are neither.

## Round-trip

Parse the fixture into the SDK's own type, serialise it back **compactly**, and
compare against the other SDK's compact serialisation of the same fixture. That
catches field renames, optionality drift, and number and timestamp formatting —
the failures that otherwise surface at the wire, in production, on the far side.

It deliberately does **not** compare bytes against the file as stored. The
fixtures are pretty-printed for review; production serialises compactly, into
JSONL and SSE frames. Asserting byte-identity against the stored form would
oblige both SDKs to carry a pretty-printer that exists only to satisfy this
corpus, and would make the real assertion *"both pretty-print alike"*.

Key ordering is not a wire property in general — JSON objects are unordered by
specification, and no consumer may depend on the order it receives. What has
changed is that the *producers* are now deterministic about it: both SDKs
commit to a canonical form, so key order is no longer a free variable the
harness has to normalise away.

- **Envelope keys come out in declared order**: `v`, `type`, `runId`, `seq`,
  `ts`, `payload`, `capturedAt`. The Rust SDK gets this from the `Event`
  struct's field declaration order; the TypeScript SDK gets it from always
  constructing an event in this order.
- **Payload object keys are sorted recursively by UTF-8 byte order**, with
  array order left alone (though objects nested inside an array are
  themselves sorted). Both SDKs sort by UTF-8 bytes specifically, not by each
  language's default string comparison — JavaScript's `<` on strings compares
  UTF-16 code units, which diverges from Rust's byte-wise `String` ordering
  for characters outside the Basic Multilingual Plane, and the two producers
  would otherwise disagree on astral-plane payload keys.

Because this canonical form is now a property of the SDKs, the harness compares
the exact bytes each SDK's production serialiser emits, with no re-parsing or
re-sorting step of its own, rather than diffing a form the harness constructed.
A consumer still must not depend on the order it receives — that has not
changed, and nothing prevents a future non-canonicalising producer from
existing outside this repository — only that both SDKs here are now
deterministic producers rather than merely equivalent up to reordering.

Number formatting is also not a free variable (issue #9):

- **Integral-valued numbers serialise without a fractional part.** `1.0` is
  `1` on the wire. Left to each language's own JSON writer, two conforming
  producers disagree on bytes for a value they agree on numerically —
  JavaScript's `JSON.stringify` already collapses `1.0` to `1`, while Rust's
  `serde_json` writes `1.0`. The Rust SDK canonicalises before compact
  serialisation: any `f64` whose fractional part is zero and whose magnitude
  is below 2^53 is emitted as an integer, recursively, throughout the event
  including inside `payload`.
- **Negative zero serialises as `0`.** `-0.0` and `0` are not a distinction
  either SDK's wire format preserves, matching `JSON.stringify(-0)` in
  JavaScript.
- **An integral value's magnitude is bounded by 2^53 − 1**
  (`Number.MAX_SAFE_INTEGER`). Both SDKs reject an out-of-range integral
  number in a payload at parse time — recursively, through nested objects and
  arrays — exactly as they already reject an out-of-range `Event.seq`; nothing
  rounds. Non-integral values are not bounded by this rule, however large
  their magnitude. A value that needs more precision than the safe-integer
  range allows must be carried as a string instead of a number.

This is sound precisely because the ruling assumes no payload ever needs to
distinguish `1` from `1.0`, and that any integer a payload cannot afford to
lose precision on either fits the safe-integer range or is carried as a
string; a payload that needs otherwise carries that value as a string
instead of a number that happens to look integral.

`v1/` exists empty in this change because the harness is part of the envelope
contract, while the first immutable fixture must wait for the first real event.
