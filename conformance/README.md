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
- **This corpus contains captures, not invented examples.** A fixture must
  accompany the first real capture of an event variant, not precede it: a
  fixture written from an unobserved scenario is a guess, and the immutability
  rule below would then preserve the guess forever.
- Fixtures are grouped by schema version: `v1/`, `v2/`, and so on. Version 1 is
  the first protocol version; `v0/` never exists.
- A fixture's `runId`, `seq` and `ts` may be **synthesised** when the capture it
  came from never passed through a sink — a normaliser's drafts carry none of
  the three, and they are sink-owned by definition (#3). Where they are
  synthesised, `seq` is the event's true position in its capture's normalised
  stream, so `seq` values are deliberately **not contiguous** across the
  fixtures drawn from one capture: they are a selection from a run, not a
  complete stream, and a reader should not infer a gap from them.
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
- **A non-integral number's notation follows ECMAScript's, not
  `serde_json`'s** (issue #9). Left alone, the two SDKs choose the same
  shortest round-tripping decimal digits for a given value but disagree on
  when to lay them out in plain decimal versus exponential form —
  `serde_json` switches to exponential notation at `1e-6`, while
  JavaScript's `Number.prototype.toString` keeps plain decimal down to
  `1e-5`, so a value such as `2.5e-6` would serialise as `2.5e-6` from one
  SDK and `0.0000025` from the other if nothing intervened. The Rust SDK is
  the one that moves: it re-lays the digits `serde_json` already produced
  according to the ECMA-262 `Number::toString` rule — plain decimal when the
  value's decimal exponent falls in `[-6, 21)`, exponential otherwise —
  rather than recomputing them, since the digits themselves already agree. That last clause holds only because the
  Rust SDK enables `serde_json`'s `float_roundtrip` feature. Without it,
  `serde_json`'s default float parser is correctly rounded for most inputs but
  not all: it reads `0.09765190000000001` — a real `costUsd` from the first
  captured fixture — as the f64 one ULP below the one JavaScript parses, and
  then faithfully re-emits that different value as `0.0976519`. The digits then
  disagree because the *numbers* disagree, which no amount of re-laying the
  notation can repair. The feature is a correctness requirement here, not a
  performance trade.
  This applies uniformly to every number the 2^53 rule below leaves as a
  float, including an integral value at or beyond that bound: such a value
  no longer keeps the trailing `.0` `serde_json` would otherwise append,
  because ECMAScript's notation rule does not distinguish a whole number
  from any other by how it happens to be represented internally.
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

Unlike that safe-integer bound, two further bounds are policy rather than
representability (issue #28), so they belong to `validate` and not to
`parseEvent`/`parse_event`: every string leaf anywhere in a payload, at any
depth, is at most 16,384 Unicode scalar values (`MAX_TEXT_SCALARS`), and the
payload's own canonical serialisation is at most 131,072 bytes / 128 KiB
(`MAX_PAYLOAD_BYTES`), measured the same way this document's canonical form
is measured. Both apply regardless of whether `validate` recognises the
event's `type`, which is why a fixture in this corpus is never used to prove
either one — they hold no matter what a fixture's payload shape is, rather
than being one more thing two implementations could disagree about how to
serialise.

This is sound precisely because the ruling assumes no payload ever needs to
distinguish `1` from `1.0`, and that any integer a payload cannot afford to
lose precision on either fits the safe-integer range or is carried as a
string; a payload that needs otherwise carries that value as a string
instead of a number that happens to look integral.

`v1/` was held empty until the first real capture, because the first immutable
fixture had to record observed producer output rather than an invented example.
It now holds fixtures derived from real captures and still grows only that way.
Waiting cost time once; an invented fixture would have been wrong permanently.

## Library views

The `ethogram-corpus` crate and `@onsager-ai/ethogram-corpus` package expose
the canonical files to consumers without requiring them to know this
repository's layout. They are separate libraries that depend on the SDKs; the
SDKs never depend on the corpus.

Their committed generated modules are views of this directory, not independent
fixture definitions. After adding a captured `v1/*.json` fixture, run:

```sh
pnpm run generate:corpus
```

The generator sorts file names by UTF-8 bytes and rewrites both language
modules deterministically. CI reruns it and rejects any working-tree diff, so
a directory fixture missing from either module and a stale module entry whose
file was removed both fail the same check.
