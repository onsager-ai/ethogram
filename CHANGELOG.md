# Changelog

Consumers pin this repository **by git revision**, not by version — see the
`rev = "…"` in ostrom's and umwelt's `Cargo.toml`. So the useful unit here is
the commit, and each entry names the revision a consumer would pin to reach it.

Nothing has been published to crates.io or npm. Both crates carry
`publish = false` and both packages `"private": true`, and the package and crate
names remain provisional pending the principal's confirmation before any first
publish (recorded on #10). The version below has therefore never been released;
it is the version a first release would carry, not a marker that one happened.

## Unreleased

### Independent corpus envelopes (#46)

The corpus libraries now state at their collection APIs that fixtures are
independent envelopes, not a stream: shared synthesised `runId` values do not
make separate captures safe to fold together with `foldRun`. One Rust inventory
test records the three historical `(runId, seq)` collisions with their exact
fixture-name sets and reasons, rejecting new collisions, changed sets, and
stale exceptions.

No fixture, wire string, or SDK behaviour changed. Pin the revision introducing
this entry to take the documentation and corpus-inventory test; a consumer
repin requires no adaptation beyond the clarified sentence.

### Representability independent of closedness (#48)

Rust now rejects `ControlAppliedReason::Unknown` spelling a known reason as
`Malformed`, because it cannot round-trip as itself. Representability applies
to every union carrying a typed `Unknown`, including open unions.
`ControlAppliedReason` still accepts unfamiliar strings within the excerpt
bound; the five closed unions still report unfamiliar strings as `UnknownMember`.

The registration explicitly selects open or closed membership for each union,
and a source test discovers every enum declaring `Unknown(String)` to catch
missing registrations. Any exemption must name the enum and give a reason;
none is needed today.

TypeScript behaviour, the 25 captured fixtures, and all serialised wire strings
are unchanged. Pin the revision introducing this entry to take this change.

### Typed `Unknown` across five unions (#41)

Rust now rejects a typed `Unknown` spelling a known member of `RunKind`,
`RunOutcome`, `CaptureRefusalCause`, or `DecisionKind` as `Malformed`,
matching `ControlKind`. Such a value cannot round-trip as its named variant:
parsing its wire string yields the known member. One registered Serde probe
checks all five before JSON conversion erases that distinction.

`DecisionKind` was not part of the original ruling on this issue, which
enumerated four unions; it is a closed union of exactly the same shape and
was left unguarded until this revision added it through the same
declaration.

Unfamiliar strings remain `UnknownMember` at validation. TypeScript behaviour,
the 24 captured fixtures, and all serialised wire strings are unchanged. Pin
the revision introducing this entry to take this change.

### Shared wrong-type diagnostics (#42)

Rust now authors wrong-typed payload messages in TypeScript's existing form,
including nested paths and optional-field wording, so relays refusing the
same field can emit identical `capture.refused.detail`. Detail remains
non-authoritative; consumers count the typed fields, never the prose.

Sixteen new handwritten validation inputs bring wrong-type failures into the
harness's byte comparison. The 24 captured fixtures and event wire bytes are
unchanged. Pin the revision introducing this entry to take this change.

**Breaking for a consumer test that pins the old wording.** Event bytes did not
move, so nothing on the wire changed — but the *message* text did, at `9be1654`.
A consumer asserting on serde's phrasing (`invalid type: integer \`1\`, expected
a string`) sees the new form (`RunStartedPayload.parentRunId must be a string
when present`) and fails on repin. That failure is a test needing the new text,
not a defect. Nothing else about the repin is affected.

The entries below are available from revision `9e3cd37`.

### The `answer` control verb (#38, #39)

`ControlKind` gains `Answer`, so a hub can deliver a principal's decision to a
waiting pass. Until now ostrom refused `kind: "answer"` as
`capture.refused{malformed}`, which is the defect ostrom#510 exists to remove.

`control.requested` gains `decisionId` and `optionId`, **required when the kind
is `answer` and forbidden otherwise**, with `text` forbidden on an answer.
Enforced at `validate`, never at parse.

`Unknown(String)` is deliberately not a transport for a verb a runtime
understands. A typed `Unknown` spelling a known kind is now `Malformed`, because
it cannot round-trip as the variant it names.

**`ControlKind` stays closed at `validate`.** An unfamiliar kind still parses,
still round-trips byte-for-byte, and is still reported as `UnknownMember` — the
same rule the other three unions follow. A revision of #39 briefly opened it;
that was a regression, caught in review and restored before merge.

### A `reason` on a negative echo (#38, #39)

`control.applied.reason` becomes an open union: `no-such-decision`,
`already-answered`, `option-not-offered`, plus the existing `unsupported`,
`not-live` and `rejected`, with an unknown value retained and still bounded.
`validate` requires it when `ok` is false, and deliberately does not forbid it
when `ok` is true — a runtime may explain a positive echo.

### Everything else since the corpus crate

- **`decision.*`** (#24), and its emitter corrected: an answer is emitted by the
  invocation that applies it, on its own run, because the raising run has usually
  finished and a sink refuses appends to a closed run (#34). The same change adds
  **`decision.answered.requestedRunId`**, optional, naming the run that emitted
  the corresponding `decision.requested` — needed precisely because the two
  events now provably sit on different runs, which turned finding the asking run
  from an edge case into the common one. `reversal` accepts an action id such as
  `revoke:required_checks`, not only an offered option (#35).
- **`capture.refused`** (#22), carrying the bound and the count but never the
  content that breached it.
- **`control.*`** (#21) and **`run.*`** outcomes `blocked` and `unstarted` (#32).
- **A structured `ValidationError`** (#33) with `OverBound`, `PayloadTooLarge`,
  `UnknownMember`, `MissingField`, `Policy` and `Malformed`, so a sink fills
  `capture.refused` without parsing English.
- **Universal bounds in `validate`** (#30): every string leaf at most
  `MAX_TEXT_SCALARS`, every payload at most `MAX_PAYLOAD_BYTES`, applied before
  the known-type branch so an unrecognised type is bounded too.
- **The corpus** grew to 24 fixtures, every one from a real capture, plus
  hand-written validation and agreement inputs kept deliberately outside it.

## Provenance

Fixtures in `conformance/v1/` come only from real captures. Hand-written events
live in `conformance/handwritten-validation-inputs/` and
`conformance/handwritten-agreement-inputs/`, which are not the corpus and are not
immutable. That distinction is why this repository has twice withdrawn a fixture
rather than correcting it.
