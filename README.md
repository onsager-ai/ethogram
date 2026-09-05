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

**Scaffold.** The vocabulary is not yet extracted, and nothing depends on this
repository. The founding decision below is settled; the first extraction is not.

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
`conformance/` holds canonical event fixtures that both SDKs must round-trip
byte-identically, exercised in both CIs. Two idiomatic implementations, one
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

Every durable event carries the same envelope, discriminated on `type`:

| field | meaning |
|---|---|
| `v` | schema version |
| `type` | dot-namespaced `domain.past_tense`, e.g. `agent.tool_use` |
| `runId` | the harness session this belongs to |
| `seq` | gapless per-run sequence, assigned by the server, from 1 |
| `ts` | ISO-8601, stamped by the server |
| `payload` | correlated with `type` |

A *run* is one harness session. Loops and handoffs are kinds of run, not
separate concepts.

**On narration.** This protocol carries what an agent said and did — assistant
text, tool inputs, tool outputs. Every such field is excerpted at capture and
carries an explicit truncation flag; nothing is silently elided. The bound is
the lesser of two limits that were adopted together, and the other one is not
expressible here: **consumers are expected to keep narration away from anything
that decides** — a classification, a gate, a verdict. This repository defines
the transport and cannot enforce that; a consumer that renders narration and
also acts on it has broken a constraint this format assumes.

## Open before the first extraction

Three questions the scaffold deliberately does not answer, each of which is
expensive to change once a fixture exists:

1. **Where the version starts.** Chreode's `EVENT_SCHEMA_VERSION` is already
   `1`, with persisted events behind it. Starting this protocol at `0` would
   force a renumbering of a live wire; starting at `1` adopts chreode's
   numbering as the shared one.
2. **Whether `stage` is open or closed.** Chreode's `StageName` is a closed
   union of its own pipeline stages. If this protocol closes it, ostrom-hub's
   loops have no stage to name; if it stays an open string, chreode's enum
   becomes a consumer-side refinement.
3. **Which envelope fields are required.** Chreode assigns `seq` and `ts`
   server-side; a producer emitting into a different substrate may not have
   either at capture.

## Layout

```
conformance/   canonical fixtures both SDKs must round-trip byte-identically
packages/      TypeScript SDK
crates/        Rust SDK
```

## Licence

MIT.
