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

### Three descriptions corrected by the audit (#70)

`foldRun` was documented as a reference implementation "not a consumer-facing
run model". It is consumed: ostrom-hub folds runs with it in three source
files and three test files. Its doc now says so, because a change to a
consumer's rendering should not look like a change to an example.

`conformance/README.md` now states that the third of the harness's three
comparisons — Rust typed against TypeScript — is redundant by transitivity
and kept for the diagnostic. The two columns are not redundant; dropping the
typed one would lose the only check of Rust's retention on forward.

And a new fixture must now say, in the PR adding it, what it is the first to
pin. Five existing fixtures pin a shape another already pins; they stay,
because immutability binds a real capture whether or not it is redundant.

Documentation only. No wire byte, fixture or behaviour changed.

### The `non_exhaustive` question, answered once (#67)

The README now states the rule the `by` change raised: payload structs are
deliberately **not** `#[non_exhaustive]`, so an additive wire field is a
source-visible change for a Rust producer building a payload by literal. That
is the intended prompt — a producer must decide what the new field holds, and
the compile error is the only moment that decision is unavoidable. Consumers
that read or deserialise are unaffected, and nothing on the wire changes.

Documentation only. It exists so the next additive field does not re-argue
it, and so the reasoning is not left in a review thread.

### `control.applied` gains a known `by` (#67)

`by` names the principal identity that applied the control, with the same
meaning as on `control.requested`: an identity a consumer renders and never
interprets. Optional, because a runtime echoing a control it does not support
may have no separate applier to name.

**Additive, and no byte moved.** ostrom's bridge was already emitting it, so
the first captured `control.applied` — `control-applied-answer.json`, now
immutable — carried it through `extra` under the tolerance rule. Declaring it
changes where a consumer meets the field, not what is on the wire: both Rust
columns serialise that fixture identically before and after, verified by
diffing the harness output across the change rather than by argument.

A consumer repinning gets `by` as a typed optional field instead of an
unknown one. Code that read it out of the extension map keeps compiling; it
will simply stop finding it there, since a declared field no longer lands in
`extra`.

**Breaking for Rust code that builds `ControlAppliedPayload` with a struct
literal.** Adding a field to a public struct without `#[non_exhaustive]`
breaks every `ControlAppliedPayload { … }` initialiser, which must now name
`by`. This is not speculation: four literals in this repository's own suites —
three in the library tests, one in `tests/control_answer.rs` — failed to
compile on the change, and were the first sign of it. Reading
and deserialising are unaffected. Whether these payload structs should be
`#[non_exhaustive]`, so that a future additive field is a wire change and not
a source change, is a separate question this entry does not settle.

A handwritten validation input pins the new field's diagnostic across both
SDKs (`ControlAppliedPayload.by must be a string when present`), 23 error
cases now. That input is doing more work than it looks: adding a field to one
SDK and not the other changes **nothing** observable on the wire, because both
retain an unknown field and emit the same bytes for it. The wrong-type
diagnostic is where a one-sided change becomes visible.

### The first real permission exchange enters the corpus (ostrom#541)

Five fixtures from two captures of ostrom's permission bridge, taken from the
wire as the sink stamped them: an ungranted tool call raised as
`decision.requested{kind: "permission"}`, the supervisor's
`control.requested{kind: "answer"}`, the `decision.answered` recording it, the
`control.applied` echo, and — from a separate capture with its own `runId` —
the `decision.answered{byTimeout: true}` that denies when nobody answers. The
corpus now holds 30 fixtures.

Two of the five handwritten answer-verb agreement inputs are superseded by
those captures and removed, leaving six. The three negative echoes stay:
`no-such-decision`, `already-answered` and `option-not-offered` have no capture
yet, because reaching them needs a supervisor that answers wrongly during a
real pass. Retiring them on the supersession rule would have traded real
coverage for nothing.

`control-applied-answer.json` is **the first capture in the corpus carrying an
unknown payload field**. `by` is not a known member of `ControlAppliedPayload`,
so it rides as a payload extension, and both SDKs now prove on real bytes that
they preserve one — a property the corpus previously tested only from
hand-written input.

### The six run kinds are defined, and ordered (#64)

`RunKind` enumerated six members and defined one. `relay` had a sentence;
`loop`, `handoff`, `subagent`, `session` and `judgment` had only their names,
in the README, in both SDKs, and in #5's ruling. A consumer choosing between
them was reasoning from connotation, which is how two producers come to
disagree about the same run.

Each member now carries a definition and **the producer-checkable fact that
decides it** — `parentRunId`, observing-only, `schedule`, an interactive user,
a verdict-only product, a dispatched work order — on the Rust variants, in the
TypeScript union's doc comment, and in a README table. They are **ordered**,
applied top down, so no run fits two: a scheduled gatekeeper pass is a `loop`
because a schedule outranks what the run produces, while the same evaluation
run on demand is a `judgment`.

This writes down what ostrom already does rather than inventing: `pass.rs` →
`loop`, `implement.rs` → `handoff`, `decision_answers.rs` → `judgment`,
`run_events.rs` → `relay`.

A source scan in each SDK asserts every member carries a definition and fails
by name when one does not. The Rust scan cross-checks the variant list against
`as_str`'s arms, so a reflow that hides a variant from it fails as a
disagreement rather than as a smaller check.

**Timing, not tidiness.** No captured fixture has ever carried a
`run.started`, so no member has been observed in a real capture. The first
producer to emit one supplies the first permanent evidence of what its kind
means, under the immutability rule. These words land before that capture
rather than after it.

No wire byte, fixture or behaviour changed; a member's meaning is now written
where a consumer meets it.

### A runId belongs to exactly one capture (#62)

#46 ruled two things and #51 mechanised one. The test rejected two fixtures
sharing `(runId, seq)`; nothing stopped a new capture reusing a `runId`
already in the corpus at a fresh `seq`. Confirmed by probe before the change:
a fixture with `runId: "sweep"` — already present — and `seq: 97` passed.

`every_run_id_belongs_to_exactly_one_capture` replaces that test. It pins
every `runId` carrying more than one fixture together with its exact
`(fixture, seq)` set and asserts disk equals the list in both directions, so
a new capture reusing a `runId` fails, a fixture joining a group fails, and a
withdrawal that leaves an entry stale fails. The `(runId, seq)` test is
retired rather than kept alongside it: any collision requires sharing a
`runId`, so the group check catches it first, and two overlapping lists could
drift.

**Fixtures from one capture still share a `runId`** — a selection from that
run's stream, as `conformance/README.md` has always said.
`run-claude-subagent` carries eight, `run-claude-control-interrupt` four. A
rule of one `runId` per *fixture* would have forced a real multi-event capture
to give its events different ids, falsifying the capture.

### Which agreement inputs a capture supersedes (#62)

`handwritten-agreement-inputs/README.md` now separates the five answer-verb
shapes, which a real capture replaces, from three that no producer can
supersede: the notation-band cost, the unknown-field payload, and the
unrecognised type. Each holds a property the corpus cannot.
`the_permanent_agreement_inputs_are_still_present` fails by file name, with
the reason, if one goes missing — so retiring all eight on the supersession
rule cannot pass green.

No wire byte, fixture or SDK behaviour changed.

### An input for the branch the counts only claimed (#60)

The three-column harness (#57) sends an input whose `type` Rust does not
recognise down an untyped-only path with one comparison instead of three.
No input had ever taken it: all 32 use a recognised `type`, so the run
reported `0 stayed untyped-only` and the branch was proved by arithmetic.

`handwritten-agreement-inputs/unrecognised-type.json` carries the `type`
`x-conformance.unrecognised-by-design`. Unknown types are open by design
(#12) — such an event must parse, canonicalise and forward byte-for-byte,
which is what lets the vocabulary grow without a version bump — and this is
now the one input that makes the harness prove it. It is also the only
cross-SDK check that an unrecognised type passes `validate` cleanly rather
than being refused.

Counts read 32 typed and 1 untyped-only. A driver that wrongly produced a
typed column for it is caught by name: `typed file
agreement/unrecognised-type.json exists on disk but the inventory does not
record it as typed`.

No wire byte, fixture or behaviour changed.

### A third column proves Rust's typed layer against itself (#57)

Implements the design-lane ruling on #57 (2026-09-09). The harness compared
Rust's untyped path — `parse_event` deserialises into
`Event<serde_json::Value>` — against TypeScript's typed one, for every
corpus fixture and agreement input alike. Rust *does* construct
`RunStartedPayload` and its siblings on the way, inside
`check_known_payload_representation`, but drops the result: the typed value
never reached a serialiser, so what that layer would **write** — its
`#[serde(flatten)] extra` retention above all — was in no cross-SDK
comparison. Reverting that retention to `#[serde(skip)]` on
`RunStartedPayload.extra` left `./conformance/run.sh` green.

For every fixture and agreement input whose `type` the Rust driver
recognises, it now also deserialises the payload into its typed struct,
re-serialises through the same canonicaliser, and writes that as a third
column. The harness compares all three byte-for-byte per input: TypeScript,
Rust untyped, Rust typed. This both closes the #12 tolerance clause and
asserts a stronger thing: Rust's typed round-trip equals its own untyped
one, so the typed layer can never quietly hold a different opinion of a
payload than the wire does. An unrecognised type stays untyped-only, and the
driver's own inventory (`_typed.json` in the typed output directory) and
`run.sh`'s per-input log line say so explicitly rather than by omission.

A reach assertion — the third of this shape in the repository, after the
lesson of #55 — runs on both sides: the driver checks that every input its
own bookkeeping calls "typed" actually has a typed file on disk, and
`run.sh` independently re-derives the same fact from the typed output
directory's contents. Either one fails, naming the input, if a future
change quietly stops writing a known type's typed column while still
claiming to.

`run-started-unknown-fields.json` gained four number shapes to its unknown
payload keys — a large integer just inside the safe bound, a small integer,
an integral-valued float, and a non-integral value in the divergent
`[1e-6, 1e-5)` band — the same four the Rust `unknown_payload_numbers_round_trip_byte_identically`
unit test and its TypeScript twin `"unknown payload numbers round-trip
byte-identically"` pinned by hand. With the typed column now reaching those
numbers through `RunStartedPayload`'s own `#[serde(flatten)]` layer (checked
by reverting it and confirming `./conformance/run.sh` fails, before removing
either test), both twins are retired: the harness covers what they covered.

No corpus fixture and no wire byte changed. `run-started-unknown-fields.json`
is an agreement input, not a corpus fixture, and is explicitly outside the
immutability rule; a consumer repin takes the four additional payload keys
on that one input, the `_typed.json` inventory file the Rust conformance
binary now writes alongside its existing output, and the binary's new
second (typed-output-directory) argument.

### The notation band enters the harness (#53)

Two hand-written agreement inputs. `run-finished-band-cost.json` carries a
`costUsd` of `2.5e-6`, inside the one decade where `serde_json` and
ECMAScript disagreed about notation for the same double (#9, class 4); both
SDKs must emit `0.0000025`, and reverting Rust's notation branch now fails
the cross-SDK byte diff instead of only two unit tests. No corpus fixture
can carry this value — fixtures are captures, and no capture has produced a
cost in the band.

`run-started-unknown-fields.json` carries unfamiliar payload keys sorting
before the first known key and after the last, plus a nested object and an
array.

**A boundary this exposed, now written down.** Rust's `parse_event` returns
an `Event` whose payload is a `serde_json::Value`; it constructs the typed
payload structs only to check representability and drops the result, so
nothing they would write is observable. TypeScript's `parseEvent` routes a known `type`
through its typed parser. So the harness — for agreement inputs and for
every corpus fixture — compares Rust's untyped path against TypeScript's
typed one. Canonicalisation is compared in full; Rust's
`#[serde(flatten)]` retention of unknown fields is not reachable from it,
and remains covered by its own suite. Issue #57 carries that gap, and the
tolerance clause owed from #12 is still owed.

No wire byte, fixture or behaviour changed. A consumer repin takes two more
harness inputs and nothing else.

### Negative zero asserted on both sides (#52)

Class 2 of #9 — negative zero serialises as `0` — was pinned in Rust and
nowhere in TypeScript, which took it from `JSON.stringify(-0)` with no test
naming the rule. Two TypeScript tests now assert it, at every depth and for
a `-0` arriving on the wire rather than only one built in the test file.

**Nothing to adapt to.** No wire byte, fixture or behaviour changed; the
SDKs already agreed. Recorded because the rule now has an assertion on both
sides instead of one, which is the state the ruling asked for.

### Consumer rule for unknown members, stated once (#54)

The README's tolerant-reader paragraph and all six retaining unions'
doc comments — `RunKind`, `ControlKind`, `CaptureRefusalCause`,
`DecisionKind`, `RunOutcome`, `ControlAppliedReason`, in both SDKs — now
state the same three-clause rule (ruled on #12) a consumer must follow for
an unfamiliar member: render it with its raw value, never map it onto a
known member, and, when acting on it (a sink deciding a run is finished, a
hub deciding a decision is pending), treat it as "not this", never as a
default. `ControlAppliedReason`'s note says plainly that an unfamiliar
reason there is the expected case, not the exceptional one, since it stays
open at validation too.

A source-scan test in each SDK discovers every such union from its own
declaration shape — `enum … Unknown(String)` in Rust, `KnownX | (string &
{})` in TypeScript — and fails, naming the union, if its doc comment lacks
the rule. No hand-maintained list of union names; a seventh union added
later without the sentence fails the same way. No exemption was needed.

No fixture, wire string, or SDK behaviour changed. Pin the revision
introducing this entry to take the documentation and the two scan tests; a
consumer repin requires no adaptation beyond reading the now-stated rule.

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
