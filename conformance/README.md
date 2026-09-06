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

The one property this drops is key ordering, which is not a wire property —
JSON objects are unordered by specification, and no consumer may depend on the
order it receives. After exercising each SDK's compact serialiser, both
harnesses re-parse that output and recursively sort object keys by their UTF-8
bytes, leaving array order untouched, before the cross-language diff. That
canonical comparison removes only object-key order while retaining differences
in field presence, values, and arrays.

Number formatting, by contrast, is no longer a free variable (issue #9):
**integral-valued numbers serialise without a fractional part.** `1.0` is `1`
on the wire. Left to each language's own JSON writer, two conforming producers
disagree on bytes for a value they agree on numerically — JavaScript's
`JSON.stringify` already collapses `1.0` to `1`, while Rust's `serde_json`
writes `1.0`. The Rust SDK now canonicalises before compact serialisation: any
`f64` whose fractional part is zero and whose magnitude is below 2^53 is
emitted as an integer, recursively, throughout the event including inside
`payload`. This is sound precisely because the ruling assumes no payload ever
needs to distinguish `1` from `1.0`; a field that does needs that distinction
carried as a string instead, not as a number that happens to look integral.

`v1/` exists empty in this change because the harness is part of the envelope
contract, while the first immutable fixture must wait for the first real event.
