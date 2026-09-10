# ethogram

GitHub slug: **`onsager-ai/ethogram`** (public, MIT).

The shared event protocol for harness observation. Read the README first for
what this is and why it stands alone; this file carries the rules that bind
changes here.

## Repo principles

**1. One definition, or a mechanical proof that two agree.** Two SDK surfaces
hold the same vocabulary. They are held together by `conformance/`, not by
discipline. A vocabulary change that lands in one language without a fixture
proving the other agrees is a defect, however obviously correct it looks.

<!-- Source: repo scaffold, 2026-09-06, from the founding decision in the
     README. Preconditions: assumes exactly two hand-written implementations.
     Invalid if a third language is added. -->

**2. The wire is the contract, and it is public.** Anything that cannot be
written down as an envelope and a payload does not belong in this repository.
No consumer-specific rendering, no storage concerns, no hosted substrate named
anywhere. Consumers depend on this; this depends on nothing of theirs.

**3. A version is a promise.** Consumers pin by version precisely so that
harness churn is absorbed here rather than in three codebases. A breaking
change to an existing payload is a version bump, never an edit in place.

**4. Narration is carried, and it is bounded at capture.** This protocol
transports what an agent said and did — assistant text, tool inputs, tool
outputs. That is the point of it. Every such field is excerpted at capture with
an explicit truncation flag, never silently elided, and this repository states
no opinion about what a consumer may do with it beyond that.

<!-- Source: onsager-ai/ostrom-hub#131, 2026-09-05, which amended that repository's
     principle 4 from prohibition to bounded record. This repository defines
     the transport; the constraint that narration never reaches anything that
     decides is a consumer's to enforce, and is expected of every consumer.
     Deliberately not phrased as "consumer X already enforces it": the first
     draft asserted that of ostrom-hub's principle 3, which at the time
     constrained only where judgment logic may live and said nothing about
     what that logic is fed. A citation to a specific consumer's rules is
     false the moment that consumer edits them, and this repository would not
     find out.
     Preconditions: assumes bounding happens at capture, where the producer
     knows what it truncated. Invalid if a payload appears whose meaning is
     destroyed by excerpting, which is a signal to model it differently. -->

## Always-spec surfaces

Regardless of diff size, these get a spec issue:

- **The event vocabulary** — adding, renaming or removing a `type`. Every
  consumer's rendering depends on it.
- **The envelope** — a field's presence, name or meaning.
- **The conformance corpus** — changing what a fixture asserts, which changes
  what "the two agree" means.
