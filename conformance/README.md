# Conformance corpus

Canonical event fixtures. **Both SDKs must round-trip every fixture in this
directory byte-identically**, and both CIs run the corpus.

This is the mechanism behind principle 1. Two hand-written implementations in
different languages are only one definition if something mechanical proves they
agree; this directory is that proof, and it is why neither SDK is generated
from the other.

## Rules

- One fixture is one JSON document holding one complete envelope.
- Fixtures are grouped by schema version: `v0/`, `v1/`, …
- A fixture is **immutable once published**. Correcting a fixture changes what
  agreement means, retroactively, in both SDKs at once. Add a new one instead.
- Every payload variant gets at least one fixture. A payload with no fixture is
  a payload the two implementations have never been shown to agree about.
- Narration fields carry placeholder content only. The corpus is public and
  permanent; real transcripts are neither.

## Round-trip

Parse the fixture into the SDK's own type, serialise it back, and compare bytes
against the file. That catches field renames, optionality drift, number and
timestamp formatting, and key ordering — the failures that otherwise surface at
the wire, in production, on the far side.
