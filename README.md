# ethogram

**The event protocol for harness observation.** A versioned vocabulary of agent
behaviours, plus the machinery to record them — shared by
[`chreode`](https://github.com/onsager-ai/chreode),
[`ostrom`](https://github.com/onsager-ai/ostrom) and
`ostrom-hub`, so one viewer can render either system's runs.

In ethology an *ethogram* is the catalogued inventory of the discrete behaviours
an organism performs, compiled from systematic observation. That is what this
is: not a log format, but the enumerated set of things an agent can be observed
doing, named once so that two systems mean the same thing by the same word.

## Status

**The version 1 vocabulary is complete.** The envelope, its `run.*` lifecycle,
the six `agent.*` observations, the two `control.*` events, `capture.refused`,
and the two `decision.*` events exist in both SDKs. The conformance corpus
(`conformance/v1/`) is seeded from real captures, and grows only that way —
never from an invented example, because an invented fixture would become an
immutable guess. The corpus is the count; this sentence deliberately is not,
since a number here would go stale every time a capture lands.

## Why it is a separate repository

Chreode already holds the mature implementation — the event envelope, the
`agent.*` payload map, and the normalisers that isolate per-harness CLI churn at
the edge. The obvious move would be to publish it from there.

That inverts the dependency. Chreode sits downstream of the Onsager substrate; a
protocol that `ostrom` and `ostrom-hub` pin cannot live inside a consumer of
theirs without making them depend on their own consumer. Standalone lets all
three pin it by version, which matters precisely because absorbing harness churn
is the entire point of the thing.

## Why it is public

It is a wire format, not a moat. A protocol nobody outside can read has failed
at the only job a protocol has.

## The founding decision: two implementations, one corpus

Two SDK surfaces are planned — TypeScript under `packages/`, Rust under
`crates/`. A repository holding the same vocabulary twice must say how the two
are held together, before either exists.

**Rejected: a JSON Schema as the single source of truth.** A schema plus two
hand-written SDKs is three definitions wearing one hat. Each drifts from the
others invisibly, and the drift surfaces where it costs most — at the wire, in
production, when a payload fails to deserialise on the far side.

**Rejected: generate one language from the other.** This is genuinely one
definition, and it was close. It loses because it forces one language's type
system onto the other, and the generated side ends up unidiomatic in a library
whose whole value is being pleasant to depend on.

**Chosen: both hand-written, with a conformance corpus that proves they agree.**
`conformance/` holds canonical event fixtures that both SDKs must agree on the
compact canonical serialisation of, exercised in both CIs. Not byte-identity
against the file as stored — the fixtures are pretty-printed for review, while
production serialises compactly; see `conformance/README.md` for why that
distinction is the whole assertion. Two idiomatic implementations, one
mechanical proof of agreement.

The corpus is load-bearing and must exist from the first event, not be added
later: a corpus written after two implementations already disagree has to be
reverse-engineered from the disagreement, and will encode it.

<!-- Source: principal, 2026-09-06, on the creation of this repository, from a
     design exchange between two sessions. Preconditions: assumes two SDK
     surfaces in different languages, both worth writing idiomatically. Invalid
     if a third language is added — at which point pairwise hand-writing stops
     scaling and generation from one definition becomes the cheaper shape. -->

## The envelope

The protocol has two related shapes because capture and durable observation
have different responsibilities. A producer emits an `EventDraft` containing
only `type`, `payload`, and optional `capturedAt`; a sink turns it into an
`Event` by adding the fields a reader must be able to rely on. `capturedAt`,
when supplied from the producer's clock, is preserved unchanged and is the only
optional field on the stored envelope.

| field | stamped by | meaning |
|---|---|---|
| `v` | sink | schema version, `1` |
| `type` | producer | dot-namespaced `domain.past_tense`, e.g. `agent.tool_use` |
| `runId` | sink, from the run the draft was submitted to | one harness session or observing process |
| `seq` | sink | gapless per run, from 1 |
| `ts` | sink | ISO-8601 from the sink's clock at append |
| `payload` | producer | correlated with `type` |
| `capturedAt` | producer, optionally | ISO-8601 from the producer's clock at capture |

An `Event` always has the first six fields. A reader can therefore replay and
then follow a run, fold it, and prove it is gapless without handling an
unstamped intermediate shape. When one sink receives already-stamped events
from another, it preserves `seq` and `ts` and rejects a gap rather than
renumbering it. This assumes each producer submits drafts to exactly one sink
per run; concurrent sinks would require `seq` to gain a partition.

**An optional field is either absent or has a value; an explicit `null` is a
parse error (onsager-ai/ostrom-hub#146).** Absence is the only spelling of absence. The likeliest
source of a stray `null` is a JavaScript producer: `JSON.stringify` omits an
`undefined` field but preserves a `null` one, so a producer that initialises a
field to `null` rather than leaving it unset would put a value meaning "no
value" on the wire — a second spelling of absence that both SDKs would then
have to agree about. Refusing it at parse keeps one spelling. This is a
**parse** rule, not a `validate` policy like the bounds below: an explicit
`null` cannot populate an `Option<T>`/optional field faithfully, so it is a
representability question rather than a producer-conduct one.

A *run* is one harness session, or one process that observes them. Loops,
handoffs, and relays are kinds of run, not separate concepts.

## Run lifecycle

| type | required payload | optional payload | meaning |
|---|---|---|---|
| `run.started` | `kind`, `actor`, `harness` | `model`, `parentRunId`, `parentToolUseId`, `schedule`, `repository`, `workOrder`, `ceilings` | Opens one run and records the harness identity and any declared parent or bounds. |
| `run.finished` | `outcome`, `durationMs` | `reason`, `truncated`, `costUsd`, `usage`, `estimated` | Closes one run; failures use `outcome: "failed"` and `reason` so every run has one terminal event shape. |

`kind` is one of `loop`, `handoff`, `subagent`, `session`, `judgment`, or
`relay`. **Each is decided by a fact a producer can check, not by what its
name suggests, and the checks are ordered so that no run fits two** (onsager-ai/ethogram#64).
Apply them top down and take the first that holds:

| order | kind | the run | the check |
|---|---|---|---|
| 1 | `subagent` | started by another run and observed under it | `parentRunId` present, with `parentToolUseId` when a tool call spawned it |
| 2 | `relay` | a long-lived process that observes other runs and emits on its own run, changing nothing itself | it emits about other runs; no work order, no repository change |
| 3 | `loop` | started by a schedule the operator declared, recurring at that cadence | `schedule` present, and the scheduler started it rather than a person or a dispatch |
| 4 | `session` | an interactive harness session in which the harness's own user initiates the turns | a person started it at the harness, with `actor` naming the harness's notion of that user; no `schedule`, no work order |
| 5 | `judgment` | its product is a decision or verdict record and nothing else — an answer to a queued item, a gate evaluated on demand | it emits `decision.*` or a verdict and changes no repository; a principal's or operator's command started it |
| 6 | `handoff` | an orchestrator or principal dispatches a work order to an agent to carry out unattended, once | **otherwise** — none of the five above holds. This row is the default arm, so a run reaching it is a `handoff` whether or not it carries `workOrder` or an equivalent intent reference; that field is typical of a handoff, not a condition for being one |

The order is doing real work. A gatekeeper pass that runs on a schedule is a
`loop`, because a schedule outranks what the run produces; the same evaluation
run on demand is a `judgment`. Without the ordering both readings would be
defensible, and two producers would disagree about the same run.

`outcome` is one of `completed`, `failed`, `no-op`, `timed-out`, `interrupted`,
`permission-denied`, `canceled`, `capped`, `blocked`, or `unstarted`. `capped`
means a non-time ceiling such as tokens, cost, or turns was reached, with
`reason` naming which; `timed-out` remains wall-clock or idle timeout, and
`canceled` remains operator-only. `blocked` ends a run that may not proceed
until something outside the run changes; **it is not an error of the run**, so
a consumer that colours it as a failure is misreporting. `unstarted` means the
run was recorded — `run.started` was emitted, the schedule fired — but its
process never ran at all, with `reason` naming why (a spawn failure, a
disarmed loop, a missing binary), and is distinct from `failed`, where the
process ran and did not succeed. These sets are closed at validation but open
and retaining at parse: an unfamiliar member is carried and forwarded as its
exact raw string, and `validate` reports it as unknown so a sink may refuse
it.

`ceilings` may carry `costUsd`, `tokens`, `wallMs`, `idleMs`, and `turns`. Every
declared ceiling is enforced; an absent ceiling means unbounded and unenforced,
not defaulted. `wallMs` and `idleMs` both end a run as `timed-out`, and the idle
cap is suspended during an in-flight tool call. `ceilings.turns` is the bound;
`agent.completed.turns` is the actual. `usage` may carry `inputTokens`,
`outputTokens`, `cacheReadTokens`, `cacheCreationTokens`, and `unit`; an absent
`unit` means tokens, while a present value prevents consumers from summing
unlike harness units.

## Agent observation

| type | required payload | optional payload | meaning |
|---|---|---|---|
| `agent.started` | — | `stage`, `model`, `sessionId`, `pid` | Records that an agent began, with the harness's own session identifier providing the route back to its raw local transcript. |
| `agent.text` | `text` | `stage`, `truncated`, `parentToolUseId` | Carries bounded assistant narration and, when nested beneath a tool call, names that parent. |
| `agent.tool_use` | `tool` | `stage`, `inputExcerpt`, `truncated`, `toolUseId`, `parentToolUseId` | Records a tool invocation with bounded input and identifiers that preserve nesting. |
| `agent.tool_result` | `tool` | `stage`, `isError`, `resultExcerpt`, `truncated`, `toolUseId`, `parentToolUseId` | Records a bounded tool response and associates it with the corresponding invocation. |
| `agent.completed` | — | `stage`, `turns`, `sessionId`, `costUsd`, `model`, `usage`, `durationMs`, `estimated` | Records completion and any harness-reported totals without requiring metrics the harness does not expose. |
| `agent.warning` | `message` | `stage` | Carries a non-terminal harness warning without promoting it to a run outcome. |

`stage` is an open string on every agent observation. Pipeline stages belong to
the producing harness, so closing this field here would import one consumer's
pipeline vocabulary into the shared protocol and exclude stages used by other
consumers. `agent.completed.usage` has the same shape and meaning as
`run.finished.usage`; it is one wire shape rather than two coincidentally
similar declarations.

**Payloads are tolerant at read and retaining on forward (issue onsager-ai/ethogram#12).** An
unknown payload field is never rejected and never dropped: a sink that
forwards an event it does not fully understand must be byte-preserving, or the
stream loses data silently at exactly the boundary this protocol exists to
cross. What stays strict at parse is the envelope (an unknown envelope field is
still rejected), representability, and the required payload fields in the
table — a `run.finished` without `durationMs` is malformed no matter what else
it carries. Closed-union membership and capture bounds are enforced by
`validate`, not parsing, so a forwarder can faithfully carry a producer's
invalid event. Unknown event `type`s remain open, as they always were.

**Additive on the wire is deliberately not additive in Rust source (ruled on
onsager-ai/ethogram#67).** The payload structs are not `#[non_exhaustive]`, so adding an optional
field breaks any `Payload { … }` literal until it names the new field. That is
the intent, not an oversight: a **producer** building a payload by literal
must decide what the new field should hold, and a compile error is the only
moment that decision is unavoidable. A **consumer** that reads or
deserialises is unaffected, and so is anything on the wire — an older
producer that never sets the field emits exactly what it emitted before. The
rule holds for every payload field added from here, so it does not need
re-arguing each time.

**Opening those unions this way only stays safe because of a consumer rule
(ruled on onsager-ai/ethogram#12), and this repository is where that rule has to be written
down.** An unfamiliar member of any union that retains unknown strings — every
closed union named above, and the open `ControlAppliedReason` — is rendered
with its raw value, never mapped onto a known member; a consumer that must act on a
member — a sink deciding a run is finished, a hub deciding a decision is
pending — treats an unfamiliar one as "not this", never as a default. This
repository defines the wire and cannot enforce what a consumer's own code
does with it (principle 2), so stating the rule here is the only enforcement
available to it. It is what replaced the strictness that used to fail loudly
at the parse boundary while these unions were closed: opening them moved that
failure to the consumer's own match arm, and it only fires there if the
consumer was told to write one.

**On narration.** This protocol carries what an agent said and did — assistant
text, tool inputs, tool outputs. Every such field is excerpted at capture and
carries an explicit truncation flag; nothing is silently elided. The bound is
16,384 Unicode scalar values for `agent.text` and 4,096 for tool input and
result excerpts, finish reasons, warning messages, control text and reason,
decision dossier narration, and decision option labels. Excerpting counts code
points, not UTF-8 bytes or UTF-16 code units, and cuts only on a code point
boundary; it may still divide a grapheme cluster such as a combining sequence
or joined emoji. The other limit adopted with these bounds is not expressible
here: **consumers are expected to keep
narration away from anything that decides** — a classification, a gate, a
verdict. This repository defines the transport and cannot enforce that; a
consumer that renders narration and also acts on it has broken a constraint
this format assumes.

**Two bounds apply universally, under the per-field ones above (issue onsager-ai/ethogram#28).**
The named bounds just described exist only for the types this SDK knows;
without a floor beneath them, an unrecognised `type` — the one shape neither
per-field check above ever runs against — could carry an unbounded payload
and still validate cleanly. `validate` therefore also enforces, for every
event regardless of whether its `type` is recognised: every string leaf
anywhere in the payload, at any depth, is at most 16,384 Unicode scalar
values (`MAX_TEXT_SCALARS`), and the payload's serialised size is at most
131,072 bytes / 128 KiB (`MAX_PAYLOAD_BYTES`) — the payload alone, never the
envelope. The byte bound is doubled past the naive 64 KiB one might expect
from `16,384 × 4`-byte astral scalars, specifically so it never collides with
the scalar bound on an `agent.text` built correctly from emoji-heavy input.
Both are validation policy, not parsing policy, exactly like the per-field
bounds: `parseEvent`/`parse_event` still carry a payload that exceeds either,
because a forwarder must still be able to relay it.

## Control

| type | required payload | optional payload | meaning |
|---|---|---|---|
| `control.requested` | `controlId`, `kind`, `by` | `decisionId`, `optionId`, `text`, `truncated` | Records a request to interrupt or steer the run, or answer a waiting decision, naming the requesting principal. |
| `control.applied` | `controlId`, `ok` | `by`, `reason`, `truncated`, `landedIn` | Records whether the runtime honoured the request, who applied it, and where a hard kill landed. |

The known `kind` values are `interrupt`, `steer`, and `answer`. Unfamiliar
strings are retained exactly at parse and reported as `UnknownMember` at
validation. A known verb must use its known variant:
Rust's `Unknown(s)` is rejected by `validate` as `Malformed` when `s` spells
any known control kind. JSON strings always receive the rules of the kind
they spell; TypeScript has no distinct runtime `Unknown` wrapper. There is
deliberately no `pause` member: no harness the operator uses can pause
headlessly, and a verb the runtime cannot honour is a lie in a type.

`answer` delivers the principal's choice to a waiting pass. `decisionId` and
`optionId` are optional in the payload type but both are required by
`validate` for `answer` (`MissingField` if absent). Both must be absent for
every other kind (`Policy` if present). `text` must be absent on `answer`,
including an empty string (`Policy` if present). These are validation rules;
parsing still carries every representable request. The identifiers are not
excerpt-bounded, though the universal payload bounds apply.

`control.applied.reason` is an open string union: `no-such-decision`,
`already-answered`, `option-not-offered`, `unsupported`, `not-live`, and
`rejected`, with unfamiliar strings retained exactly. The existing 4,096-scalar
excerpt bound still applies to unknown reasons, where producer prose can
arrive. `validate` requires a reason when `ok` is `false` (`MissingField` if
absent), and permits a reason when `ok` is `true` so a runtime can explain a
positive echo. Parsing does not enforce this presence rule.

`steer` is **between turns**. Mid-turn injection is not available headlessly
on Claude Code or Codex, and the protocol does not pretend otherwise; a
runtime honours `steer` by resuming the session with `text` as the next user
turn. `interrupt` is a process-group termination with grace; the runtime
emits `run.finished` with `outcome: "interrupted"` **after** `control.applied`,
never before. Both `control.*` events are emitted by **the run's runtime,
never by the console** — a console that shows a run as interrupted before
`control.applied` arrives has misread the protocol. `landedIn` records the
`toolUseId` a hard kill landed inside, when there was one.

## Capture

| type | required payload | optional payload | meaning |
|---|---|---|---|
| `capture.refused` | `cause`, `sourceRunId` | `sourceSeq`, `sourceType`, `field`, `count`, `max`, `detail`, `truncated` | Records an event a relay or capturing runtime refused, on that runtime's own run rather than the source run whose sequence it cannot touch. |

`cause` is one of `over_bound`, `gap`, `duplicate`, `finished`, or `malformed`,
closed at validation and open and retaining at parse like the other closed
unions. An `over_bound` refusal carries the bounded field and its measured
`count` and `max`, never the content that exceeded the bound. A `malformed`
refusal may carry an excerpted parser or validation message in `detail`, with
`truncated` recording whether it was cut.

### Sink guide: refusing a validation error

`validate` now returns `Result<(), ValidationError>` in Rust and throws
`ValidationError` in TypeScript. Use its structured kind to choose the
`capture.refused.cause` and copy the fields below; no message parsing is needed.

| kind | error fields | `capture.refused.cause` and fields to copy |
|---|---|---|
| `OverBound` | `path`, `count`, `max` | `over_bound`: `field = path`, `count`, `max` |
| `PayloadTooLarge` | `bytes`, `max` | `over_bound`: `field = "payload"`, `count = bytes`, `max` |
| `UnknownMember` | `path`, `value` | `malformed`: excerpt the error message into `detail` |
| `MissingField` | `path` | `malformed`: excerpt the error message into `detail` |
| `Policy` | `path`, `message` | `malformed`: excerpt `message` into `detail` |
| `Malformed` | `path`, `message` | `malformed`: excerpt `message` into `detail` |

Paths start at `payload`, such as `payload.dossier.question` or
`payload.options[0].label`. `count` measures Unicode scalar values for
`OverBound`; `bytes` measures the payload's canonical UTF-8 serialisation for
`PayloadTooLarge`. For `malformed`, use `excerpt(message, MAX_EXCERPT_SCALARS)`
and copy its text and truncation flag to `detail` and `truncated`. Stamp the
refusal on the capturing runtime's run and identify the source using
`sourceRunId`, and `sourceSeq` / `sourceType` when available.

In Rust, match `error.kind`, a closed `ValidationErrorKind` enum carrying the
fields above. `error.to_string()` preserves the original diagnostic exactly;
`From<ValidationError> for serde_json::Error` keeps callers using `?` in a
`serde_json::Result` working. In TypeScript, switch on `error.details.kind`
to narrow the discriminated union and read its fields; `error.kind` also
exposes the tag. The class extends `TypeError`, retaining its `name` and
original `message`, so existing `TypeError` checks and message matches hold.

`Policy` means a producer broke a stated rule: steer without nonempty text,
answer text, answer-only identifiers on another control kind,
and all existing `onTimeout` checks (permission only, deny only, and
membership in the request's options). `Malformed` means the opposite kind of
defect — the value sent could not be represented at all, distinct from a
producer that broke a known rule with an otherwise representable value:
missing fields aside (their own `MissingField` kind), a wrong-typed value, an
unsafe integer, or an input that cannot be serialised each carry a
`Malformed` path and their original diagnostic. Parsing still checks
representability without applying capture bounds or producer policy.

**Representability and closedness are separate rules.** Representability
requires **every union carrying a typed `Unknown`**, open or closed, to use the
known variant for a known spelling. An in-memory `Unknown("rejected")` cannot
round-trip as itself: parsing its string yields the known variant, so
`validate` reports `Malformed`. Today this applies to `RunKind`, `RunOutcome`,
`ControlKind`, `ControlAppliedReason`, `CaptureRefusalCause` and `DecisionKind`.

Closedness instead determines which strings the protocol accepts: an
*unfamiliar* string is `UnknownMember` only for the five closed unions,
`RunKind`, `RunOutcome`, `ControlKind`, `CaptureRefusalCause` and `DecisionKind`.
`ControlAppliedReason` stays open: unfamiliar strings are accepted and retained
exactly, subject to the 4,096-scalar excerpt bound.

The registration in
[`union_unknown_validation`](crates/ethogram/src/union_unknown_validation.rs)
declares both sets. Every entry receives the representability check and must
explicitly choose `Open` or `Closed` membership. A source test scans `lib.rs`
for every enum declaring `Unknown(String)` and requires its registration or a
named exemption with a reason, so adding a union cannot silently escape the
check. No enums are currently exempt.

The typed representability check runs only in validation, before JSON
conversion would erase the distinction. Serialisation still emits the exact
string. TypeScript has no typed `Unknown` wrapper; its strings receive the
rules of the member they spell.

**A consequence worth knowing before you test this rule: it cannot be observed
through JSON.** Converting the payload first erases the `Unknown`, so a check
that round-trips will pass and look as though the rule never fires.

```rust
// Sees the rule: the typed value still knows it is an Unknown.
validate(CONTROL_APPLIED, &payload)                          // -> Malformed

// Cannot see it: conversion already collapsed Unknown("rejected")
// into something indistinguishable from the known Rejected variant.
validate(CONTROL_APPLIED, &serde_json::to_value(&payload)?)  // -> Ok(())
```

Both answers are right for what was asked. The second is validating a document,
and that document is exactly what a well-behaved producer would have written —
which is the point of the rule rather than a hole in it. The defect is
unobservable on the wire, so it has to be refused before the value reaches it.

`serialise_validation_error` / `serialiseValidationError` emits only the kind
and its fields in canonical JSON, using the event payload serialiser's UTF-8
key ordering and number notation. The compatibility diagnostic is separate
from those fields, except for `Policy.message` and `Malformed.message`.

## Decisions

| type | required payload | optional payload | meaning |
|---|---|---|---|
| `decision.requested` | `decisionId`, `kind`, `dossier`, `options` | `subject`, `expiresAt`, `onTimeout` | Opens a producer-assigned decision and carries the bounded escalation dossier plus the options a human may choose. |
| `decision.answered` | `decisionId`, `optionId`, `by` | `byTimeout`, `reversal`, `requestedRunId` | Records the answer, emitted by the invocation that applies it on its own run — usually a different, later run than the one that requested the decision — distinguishing a timeout from a principal's choice. |

`kind` is one of `permission`, `tripwire`, `gate_inconclusive`,
`human_decides`, or `budget`, closed at validation and open and retaining at
parse like the other closed unions. The required `dossier` carries `question`,
`optionsRuledOut` (an array of strings), `recommendedAction`, and `blastRadius`;
those fields and every `options[].label` are bounded at 4,096 Unicode scalar
values. A single optional `dossier.truncated` flag records whether any of that
narration was excerpted. Option ids, `decisionId`, `reversal`, and
`requestedRunId` are identifiers and are not excerpt-bounded. `subject` is a
reference such as a PR URL, issue number, or tool name, never the subject's
content.

`onTimeout` is allowed only for a `permission` and must be `deny`. Its absence
leaves the decision open. This is validation policy rather than parsing policy:
a forwarder may carry a representable invalid request, while a producer cannot
quietly give a tripwire an auto-proceed path that violates the "never
auto-proceed" rule.

**`decision.answered` is emitted by the invocation that applies the answer, on
its own run — not by the run that requested the decision, which has usually
already finished by the time a human responds.** The two events are
correlated only by `decisionId`, never by sharing a `runId`, and
`decision.answered` is never emitted by a console that merely collected the
answer. This is a correction (ruled on onsager-ai/ethogram#7): a run has at most one
`run.finished`, and a sink refuses every append to a closed run, so an answer
emitted "on the owning run" minutes or hours later would be refused by the
sink — the previous wording described something the protocol's own rules
forbid. `requestedRunId` carries the run that emitted the corresponding
`decision.requested`, since a consumer holding only the answer cannot
otherwise find the asking run now that the two events routinely live on
different runs. `by` is a resolvable principal identity rather than a display
name. `byTimeout` is semantically material: a permission that expired
unanswered is nobody deciding, not a decision with a long response gap.

`reversal`, when present, names the identifier that would undo this answer,
in one of two forms (ruled on onsager-ai/ethogram#7): an offered `options[].id`, or a
`<verb>:<subject>` **action id** — `revoke:required_checks` undoes
`excuse:required_checks`, even though `revoke:required_checks` was never
among the options offered to the human, because those options were about
whether to excuse, not about how to later revoke. Either form is meaningful
only because **the producer accepts its own reversal ids as a subsequent
`optionId` on this decision** — that acceptance is what makes an unoffered id
legible rather than arbitrary, and it is why `reversal` is not checked against
the request's options: only the producer knows which action ids it accepts,
and requiring `options[].id` membership would refuse a legitimate undo the
producer will honour.

Each SDK exposes a separate cross-event helper that checks matching
`decisionId` values and the chosen option (an offered option, or the
request's `onTimeout` value when the answer is by timeout) when both payloads
are available; it is not part of parsing because the two events remain
independent on the wire, and it checks neither `requestedRunId` nor
`reversal`.

## Decisions before the first extraction

The scaffold recorded three questions that are expensive to change once a
fixture exists. All three are now settled and remain here with their reasons:

1. **Where the version starts.** Chreode's `EVENT_SCHEMA_VERSION` is already
   `1`, with persisted events behind it. Starting this protocol at `0` would
   force a renumbering of a live wire; starting at `1` adopts chreode's
   numbering as the shared one.
2. **Whether `stage` is open or closed — settled.** It stays an open string.
   Chreode's `StageName` is a closed union of its own pipeline stages, but
   closing the protocol field would leave other consumers' loops with no stage
   to name; chreode's enum is therefore a consumer-side refinement.
3. **Which envelope fields are required — settled.** Producers emit the
   deliberately incomplete `EventDraft`; sinks store only complete `Event`
   values. Making `seq` or `ts` optional on stored events would force every
   reader to handle an object that cannot support replay-then-follow, folding,
   or proof of gaplessness.

## Layout

```
conformance/   captured versioned fixtures and separate handwritten validation/agreement inputs
packages/      TypeScript SDK
crates/        Rust SDK
```

## Licence

MIT.
