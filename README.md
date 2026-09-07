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

**Run lifecycle and agent observation implemented.** The version 1 envelope,
its `run.*` lifecycle, and the six `agent.*` observations exist in both SDKs.
The conformance corpus remains empty until the first real capture lands,
because an invented fixture would become an immutable guess.

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

A *run* is one harness session, or one process that observes them. Loops,
handoffs, and relays are kinds of run, not separate concepts.

## Run lifecycle

| type | required payload | optional payload | meaning |
|---|---|---|---|
| `run.started` | `kind`, `actor`, `harness` | `model`, `parentRunId`, `parentToolUseId`, `schedule`, `repository`, `workOrder`, `ceilings` | Opens one run and records the harness identity and any declared parent or bounds. |
| `run.finished` | `outcome`, `durationMs` | `reason`, `truncated`, `costUsd`, `usage`, `estimated` | Closes one run; failures use `outcome: "failed"` and `reason` so every run has one terminal event shape. |

`kind` is one of `loop`, `handoff`, `subagent`, `session`, `judgment`, or
`relay`; a relay is a long-lived process that observes other runs and emits on
its own run.
`outcome` is one of `completed`, `failed`, `no-op`, `timed-out`, `interrupted`,
`permission-denied`, `canceled`, or `capped`. `capped` means a non-time ceiling
such as tokens, cost, or turns was reached, with `reason` naming which;
`timed-out` remains wall-clock or idle timeout, and `canceled` remains
operator-only. These sets are closed at validation but open and retaining at
parse: an unfamiliar member is carried and forwarded as its exact raw string,
and `validate` reports it as unknown so a sink may refuse it.

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

**Payloads are tolerant at read and retaining on forward (issue #12).** An
unknown payload field is never rejected and never dropped: a sink that
forwards an event it does not fully understand must be byte-preserving, or the
stream loses data silently at exactly the boundary this protocol exists to
cross. What stays strict at parse is the envelope (an unknown envelope field is
still rejected), representability, and the required payload fields in the
table — a `run.finished` without `durationMs` is malformed no matter what else
it carries. Closed-union membership and capture bounds are enforced by
`validate`, not parsing, so a forwarder can faithfully carry a producer's
invalid event. Unknown event `type`s remain open, as they always were.

**On narration.** This protocol carries what an agent said and did — assistant
text, tool inputs, tool outputs. Every such field is excerpted at capture and
carries an explicit truncation flag; nothing is silently elided. The bound is
16,384 Unicode scalar values for `agent.text` and 4,096 for tool input and
result excerpts, finish reasons, and warning messages. Excerpting counts code
points, not UTF-8 bytes or UTF-16 code units, and cuts only on a code point
boundary; it may still divide a grapheme cluster such as a combining sequence
or joined emoji. The other limit adopted with these bounds is not expressible
here: **consumers are expected to keep
narration away from anything that decides** — a classification, a gate, a
verdict. This repository defines the transport and cannot enforce that; a
consumer that renders narration and also acts on it has broken a constraint
this format assumes.

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
conformance/   canonical fixtures both SDKs must serialise identically
packages/      TypeScript SDK
crates/        Rust SDK
```

## Licence

MIT.
