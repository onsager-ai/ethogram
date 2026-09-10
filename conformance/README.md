# Conformance corpus

Canonical event fixtures. **Both SDKs must agree on the compact canonical
serialisation of every fixture in the versioned corpus directories**, and both
CIs run the corpus. `handwritten-validation-inputs/` and
`handwritten-agreement-inputs/` are separate harness input sets, described
below, and are not part of the captured corpus.

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
  the three, and they are sink-owned by definition (onsager-ai/ethogram#3). Where they are
  synthesised, `seq` is the event's true position in its capture's normalised
  stream, so `seq` values are deliberately **not contiguous** across the
  fixtures drawn from one capture: they are a selection from a run, not a
  complete stream, and a reader should not infer a gap from them.
- **The corpus is a set of independent envelopes, not a stream**, and folding
  it by `runId` is an unsupported misuse. **A `runId` belongs to exactly one
  capture**: several fixtures may share one, as a selection from that run's
  stream, and a new capture never reuses a `runId` already here. Two
  historical groups break the second half — `sweep` and
  `judgment-20300102T030405000Z-fixture-0` each hold separate captures whose
  synthesised id collided — which is why the fixtures under one `runId` are
  still not a stream even when they do come from one run. The inventory test
  `every_run_id_belongs_to_exactly_one_capture` in
  [`crates/ethogram/tests/corpus_inventory.rs`](../crates/ethogram/tests/corpus_inventory.rs)
  pins every `runId` carrying more than one fixture, with its exact
  `(fixture, seq)` set, and fails by name on a new capture reusing one, on a
  fixture joining a group, and on an entry left stale by a withdrawal.
- A fixture is **immutable once published**. Correcting a fixture changes what
  agreement means, retroactively, in both SDKs at once. Add a new one instead.
- Every payload variant gets at least one fixture. A payload with no fixture is
  a payload the two implementations have never been shown to agree about. The
  harness reading `conformance/v1` directly, rather than a generated view of
  it, is now the only mechanical check that a fixture was not forgotten, and
  it is a stronger one than the regenerate-and-diff step it replaced: that
  step could only prove a fixture's name appeared in two committed copies of
  a list, while the harness proves both SDKs actually parse and agree on the
  file itself. A fixture that reaches the harness cannot be forgotten,
  because it is compared.
- **A new fixture says what it is the first to pin**, in the PR that adds it:
  a payload shape, a union member, a boundary no existing fixture reaches, or
  **a producer's rendering of an existing field**, where both forms conform
  and the pair proves a consumer still reads each. That fourth item is not a
  footnote. The rule first listed only the other three and would have excluded
  a fixture pinning something nothing else does: onsager-ai/ostrom#552 changes
  `decision.requested.expiresAt` from nanoseconds with a numeric offset to
  milliseconds with `Z`, and since neither this repository nor either SDK
  constrains a payload timestamp's format, both renderings conform and the
  difference is invisible to every check except a fixture carrying it.
  The corpus already holds five fixtures that pin a shape another fixture
  pins — three `decision.requested` at `kind: "tripwire"`, two at
  `human_decides`, and three `agent.completed` differing only in `turns`
  (onsager-ai/ethogram#70). Those are real captures and stay: immutability binds them, and a
  withdrawal is for a fixture that was a guess, never for one that is merely
  redundant. The rule is forward-looking, and it exists because captures
  arrive several fixtures at a time — 31 fixtures span 22 distinct
  `(type, payload-key-set)` shapes, and without a stated bar that ratio only
  falls.
- **The corpus deliberately holds more than one shape of the same event type.**
  When what a producer emits changes and both forms conform — a payload gains
  an optional field, or an existing field's rendering moves, as
  `decision.requested.expiresAt` does in onsager-ai/ostrom#552 — the earlier capture
  stays and a new capture joins it, so the corpus keeps proving that both SDKs
  still read what an older producer wrote. **Presence and rendering both
  count**; the rule was written for the first and the second arrived anyway,
  which is the usual way a rule discovers it was stated too narrowly.

  That is **backward compatibility**, and it is a different property from the
  cross-SDK agreement above: agreement asks whether two implementations read
  one document alike, backward compatibility asks whether one implementation
  still reads a document its own producer would no longer write. A corpus
  holding only the newest shape of each type silently stops testing the
  second. `decision-answered-excuse.json` and
  `decision-answered-excuse-requested-run.json` are the first such pair,
  differing by one line — a field's presence. A pair differing by rendering
  alone has not been captured yet.
- **An absent optional field records what one producer did on one run, and
  nothing more.** Every optional field in this corpus is omitted rather than
  nulled, so absence is the ordinary form and carries no emphasis. It is
  therefore easy to read a missing field as "this producer never emits it",
  which a fixture cannot establish and is frequently false. `run-started-handoff.json`
  omits `ceilings` because the capture's manifest declared no caps — not
  because a pass omits them; its producer does call `wire_ceilings`. Where a
  fixture's absence invites that reading, say so here, because the corpus is
  the artefact someone cites years later without the context that produced it.
  This one also omits `schedule` and `workOrder`: those are genuinely unset by
  the producer, which is a different fact from the first and is why both are
  worth stating rather than neither.
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

Number formatting is also not a free variable (issue onsager-ai/ethogram#9):

- **Integral-valued numbers serialise without a fractional part.** `1.0` is
  `1` on the wire. Left to each language's own JSON writer, two conforming
  producers disagree on bytes for a value they agree on numerically —
  JavaScript's `JSON.stringify` already collapses `1.0` to `1`, while Rust's
  `serde_json` writes `1.0`. The Rust SDK canonicalises before compact
  serialisation: any `f64` whose fractional part is zero and whose magnitude
  is below 2^53 is emitted as an integer, recursively, throughout the event
  including inside `payload`.
- **A non-integral number's notation follows ECMAScript's, not
  `serde_json`'s** (issue onsager-ai/ethogram#9). Left alone, the two SDKs choose the same
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
representability (issue onsager-ai/ethogram#28), so they belong to `validate` and not to
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

This directory's fixtures are also compiled into the `ethogram` crate itself
and reachable as `ethogram::v1_fixtures()`, returning `&'static [Fixture]`
with `name` and `raw_json` for each fixture and a `parse()` method that runs
it through `ethogram::parse_event`. There is no separate corpus crate and no
committed generated file: `crates/ethogram/build.rs` reads this directory at
build time, sorted by UTF-8 byte order like everywhere else in this repository,
and compiles each file in with `include_str!`. A test,
`crates/ethogram/tests/corpus_accessor.rs`, asserts the compiled-in set is
exactly equal to this directory — same names, same bytes, same order, same
count — so a fixture the build silently dropped or substituted fails loudly
rather than passing unnoticed.

## Handwritten validation inputs

`handwritten-validation-inputs/*.json` exercises `validate` with inputs for
every structured error kind and wrong-typed payload fields. These are
deliberately invented invalid inputs, not captured events: the path explicitly
says handwritten inputs, is outside
every versioned corpus directory, and is never read by the corpus generator.
Each contains `type`, `payload`, and `expectedKind`, without a stamped event
envelope. The harness passes the payload directly to `validate` so a missing
required field reaches validation rather than failing in the event parser.

Each SDK must produce a `ValidationError` of the declared kind for every
input. A clean validation result fails the harness explicitly, as does an
unexpected kind. Each error is written to `errors/<input-name>.json` under
the SDK's existing output directory using `serialise_validation_error` /
`serialiseValidationError`. These production helpers emit only `kind` and
its fields, with the same recursive UTF-8 key sorting and number handling
used for event payloads. The original diagnostic is included only where it
is a field of the kind (`Policy.message` or `Malformed.message`); stacks and
SDK-specific display metadata are excluded.

`capture.refused.detail` remains **non-authoritative**: the countable facts
are the typed fields, per onsager-ai/ethogram#15, and consumers must not key on diagnostic prose.
But where both SDKs produce `detail`, they produce the **same bytes** (onsager-ai/ethogram#42).
The harness compares the structured diagnostic messages from which a relay
excerpts that detail, including wrong-type failures.

Rust authors wrong-type messages in TypeScript's existing form:
`<Payload>.<field> must be a <type>`, with ` when present` for optional
strings, booleans, finite numbers, and non-negative safe integers. Objects
use `must be an object` without the suffix even when optional; arrays use
`must be an array`. Nested labels retain the parent payload name and array
indices, such as `DecisionRequestedPayload.options[1].label`. A wrong-typed
payload itself uses `<Payload> must be an object`. Serde's wrong-type prose
must never replace these messages in the compared output.

`run.sh` checks separate fixture, error-output, and agreement-input counts in
each manifest, then runs its existing recursive byte diff across both output
directories.
A kind, path, count, or other field disagreement therefore fails through
the same comparison as an event serialisation disagreement. The driver
reports captured fixtures, validation error cases, and agreement inputs as
three separate counts.

## Handwritten agreement inputs

[`handwritten-agreement-inputs/`](handwritten-agreement-inputs/README.md)
contains valid hand-written event envelopes for vocabulary without a producer
capture yet. These are not captures, are not subject to corpus immutability,
and must be superseded by real fixtures when a producer emits the events.
The corpus generator does not read them.

Every input must validate cleanly in both SDKs, the mirror of the validation
directory's required-error assertion. Each SDK writes its production
canonical serialisation to `agreement/<input-name>.json`; the same recursive
byte diff used for the corpus compares these outputs. Counts are checked
independently, so dropping an input from one set cannot be hidden by adding
an input to another. The first five shapes cover the answer control verb and
its echoes (onsager-ai/ethogram#38); both SDK suites also pin their exact bytes from hand-built
typed events. Two more cover number notation and unfamiliar payload keys
(onsager-ai/ethogram#53), described in that directory's README.

**Rust produces a third column here, and for the corpus (issue onsager-ai/ethogram#57).**
`parse_event` in Rust returns an `Event` whose payload is a
`serde_json::Value`. It *does* construct `RunStartedPayload` or its sibling
on the way, to check representability — but drops the result, so the typed
value never reaches a serialiser and what that layer would **write** was
never observable. TypeScript's `parseEvent` always routes a known `type`
through its typed payload parser, so it has only ever had one column to
compare. For
every input whose `type` Rust recognises, the Rust conformance binary also
deserialises the payload into that type's struct and re-serialises it
through the same canonicaliser, writing the result to a second, typed
output directory that mirrors the untyped one's layout. `run.sh` then
compares three ways per such input — TypeScript, Rust untyped, Rust
typed — rather than two: TypeScript vs. Rust untyped still proves the two
hand-written implementations agree on the wire; Rust typed vs. Rust untyped
additionally proves Rust's typed payload layer, including every payload's
`#[serde(flatten)] extra` retention, can never quietly hold a different
opinion of a payload than the wire does; and Rust typed vs. TypeScript
closes the loop.

**That third comparison is redundant, deliberately (onsager-ai/ethogram#70).** Byte equality is
transitive, so if TypeScript equals Rust untyped and Rust typed equals Rust
untyped, Rust typed equals TypeScript — the third comparison can never be the
only one to fail, and detects nothing the first two miss. It is kept for the
diagnostic: when a run goes red, three results say which column is the odd one
out without the reader deriving it. The two *columns* are not redundant, and
the distinction matters — dropping the typed column would lose the only check
of Rust's retention on forward, which is what onsager-ai/ethogram#57 added it for. Stated here
so the next reader neither removes the comparison as dead weight nor credits
it with proving something it cannot.

An input whose `type` Rust does not recognise stays untyped-only by
design — there is no typed struct to deserialise it into — and this is
reported per input rather than left to be inferred from a count: the Rust
binary's `_typed.json` inventory in the typed output directory records,
for every fixture and agreement input, its path, its `type`, and whether a
typed column was produced, and `run.sh` prints one line per input naming
which comparisons ran. A reach assertion on both sides — inside the driver,
and again in `run.sh` from the typed output directory's actual contents —
fails by input name if that bookkeeping and the typed output directory ever
disagree, rather than silently comparing fewer columns than an input
warrants.

That untyped-only branch is exercised by a real input rather than only by
the counts: `handwritten-agreement-inputs/unrecognised-type.json` carries a
`type` no vocabulary will take, and is the one input the harness compares a
single way (onsager-ai/ethogram#60). Unknown types are open by design, which makes this both
the shape the protocol promises most about and the one a consumer is
likeliest to meet from a newer producer — worth an input rather than an
arithmetic identity.
