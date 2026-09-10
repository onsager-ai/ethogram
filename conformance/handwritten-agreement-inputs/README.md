# Handwritten agreement inputs

These are hand-written, valid events for cross-SDK agreement. They are **not
captures**, are not part of `v1/`, and are **not subject to the corpus
immutability rule**.

**The supersession rule applies to some of them and not others, and the
difference is not cosmetic.** Some stand in for vocabulary a producer has not
emitted yet, and a real captured fixture replaces them when one does. Three
others can never be superseded, because no producer emits their shape:
each exists to hold a property the corpus cannot hold. Retiring all six
together on the supersession rule would leave every remaining test green while
removing the only band pin in the byte diff, the only cross-SDK check of
Rust's typed retention, and the only input on the harness's untyped-only
branch. `the_permanent_agreement_inputs_are_still_present` in
`crates/ethogram/tests/corpus_inventory.rs` fails by file name if one of the
three goes missing, with the reason it was there.

## Superseded by a real capture

Five inputs covered an answer request, a positive control echo, and negative
echoes for `no-such-decision`, `already-answered`, and `option-not-offered`.
Both SDK test suites also hand-build these shapes and pin identical canonical
JSON literals independently of these files.

**Two of the five are now superseded and gone**: onsager-ai/ostrom#541 produced the first
real permission exchange, so `control-requested-answer.json` and
`control-applied-answer.json` in `v1/` replace the hand-written answer request
and positive echo.

**The three negative echoes stay**, and not because the rule spared them: no
capture has produced them. They are refusal paths — an answer naming an unknown
decision, one answering a decision twice, one naming an option that was never
offered — and reaching them needs a supervisor that answers wrongly during a
real pass, which nothing has yet done. They are superseded when a capture
produces them, and not before. Retiring them now would remove the only
cross-SDK coverage of those reasons and replace it with nothing.

## Permanent — no producer can supersede these

Two exist to put a claim into the byte diff that the two suites had only been
asserting separately (onsager-ai/ethogram#53):

- **`run-finished-band-cost.json`** carries `costUsd` of `2.5e-6`, inside the
  one decade — `[1e-6, 1e-5)` — where `serde_json` and ECMAScript disagree
  about notation for the same double (onsager-ai/ethogram#9, class 4). It is stored as `2.5e-6`
  and both SDKs must emit `0.0000025`, which is also a reminder that the
  stored form of an input is never the wire form. No corpus fixture can carry
  this value: fixtures are captures, and no capture has produced a cost in
  the band. Revert Rust's notation branch and this input fails.

- **`run-started-unknown-fields.json`** carries unfamiliar payload keys
  sorting both before the first known key (`0alpha`) and after the last
  (`zzzTail`), plus a nested object and an array, so key ordering is compared
  across the boundary rather than only at the end. It also carries four
  number shapes (issue onsager-ai/ethogram#57): a large integer just inside the safe bound
  (`bigInt`), a small integer (`smallInt`), an integral-valued float
  (`integralFloat`, stored as `2.0`), and a non-integral value inside the
  divergent `[1e-6, 1e-5)` band (`fraction`, stored as `0.000001`).

  **This input now catches Rust dropping an unknown field, too.** The Rust
  conformance binary deserialises this input's payload into
  `RunStartedPayload` and re-serialises it through the same canonicaliser as
  a third, typed column (issue onsager-ai/ethogram#57), compared against both Rust's untyped
  column and TypeScript's. Reverting `RunStartedPayload.extra`'s
  `#[serde(flatten)]` to `#[serde(skip)]` now fails `./conformance/run.sh`,
  naming this input, where it previously stayed green — this input's earlier
  README note said exactly the opposite, and issue onsager-ai/ethogram#57 is what closed the
  gap. The four number shapes above replace two hand-synced unit tests that
  used to be the only thing checking that `#[serde(flatten)]`'s internal
  buffering does not perturb a number's wire representation: Rust's
  `unknown_payload_numbers_round_trip_byte_identically` and TypeScript's
  `"unknown payload numbers round-trip byte-identically"`. Both were removed
  once the typed column was confirmed, by the same revert-and-check above,
  to reach these numbers through this input.

And one to prove the *other* branch of that comparison (onsager-ai/ethogram#60):

- **`unrecognised-type.json`** carries the `type`
  `x-conformance.unrecognised-by-design`, which no vocabulary will ever take.
  Unknown types are open by design (onsager-ai/ethogram#12): such an event must parse,
  canonicalise and forward byte-for-byte, and this is the only input in the
  repository that makes the harness do it. Rust has no struct to deserialise
  it into, so it stays **untyped-only** — `run.sh` reports it as `compared 1
  way` where every other input reports three.

  Before this input the untyped-only branch had never run: all 32 other
  inputs use a recognised `type`, so the counts said `0 stayed untyped-only`
  and the branch was proved by arithmetic rather than by an input. Its
  payload keeps unfamiliar keys at both ends, a nested object, an array and
  an integral-valued float, so untyped canonicalisation is exercised too, not
  merely the routing decision.

  It is also the harness's only cross-SDK check that an unrecognised type
  passes `validate` cleanly rather than being refused — every input here must
  validate, and for an unknown type only the universal bounds from onsager-ai/ethogram#28 apply.

The conformance harness asserts that every input here validates cleanly, then
compares both SDKs' production canonical serialisation byte for byte under
`agreement/`. For an input whose `type` Rust recognises, it also compares a
third, typed Rust column produced the same way as for the corpus (see the
top-level `conformance/README.md`); an input of an unrecognised type is
untyped-only, by design, and is reported as such. It reports this input
count separately from captured fixtures and validation error cases. The
corpus generator never reads this directory.

`../handwritten-validation-inputs/` has the opposite invariant: every input
there must produce its declared validation error. Keep the two sets separate
so an unexpected pass or failure remains visible.
