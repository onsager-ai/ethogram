# Handwritten agreement inputs

These are hand-written, valid events for cross-SDK agreement before a producer
has emitted the corresponding vocabulary. They are **not captures**, are not
part of `v1/`, and are **not subject to the corpus immutability rule**. They
must be superseded by real captured fixtures when a producer emits these
events; the first expected producer for the answer control verb is ostrom#510.

Five inputs cover an answer request, a positive control echo, and negative
echoes for `no-such-decision`, `already-answered`, and `option-not-offered`.
Both SDK test suites also hand-build these shapes and pin identical canonical
JSON literals independently of these files.

Two more exist to put a claim into the byte diff that the two suites had only
been asserting separately (#53):

- **`run-finished-band-cost.json`** carries `costUsd` of `2.5e-6`, inside the
  one decade — `[1e-6, 1e-5)` — where `serde_json` and ECMAScript disagree
  about notation for the same double (#9, class 4). It is stored as `2.5e-6`
  and both SDKs must emit `0.0000025`, which is also a reminder that the
  stored form of an input is never the wire form. No corpus fixture can carry
  this value: fixtures are captures, and no capture has produced a cost in
  the band. Revert Rust's notation branch and this input fails.

- **`run-started-unknown-fields.json`** carries unfamiliar payload keys
  sorting both before the first known key (`0alpha`) and after the last
  (`zzzTail`), plus a nested object and an array, so key ordering is compared
  across the boundary rather than only at the end.

  **Read what this second one proves narrowly.** It catches TypeScript
  dropping an unknown field, because `parseEvent` there routes a known `type`
  through its typed payload parser. It does **not** catch Rust dropping one:
  Rust's `parse_event` keeps the payload as a `serde_json::Value` and never
  constructs `RunStartedPayload`, so its `#[serde(flatten)]` retention is not
  on this path at all. Removing that retention leaves this input green. The
  tolerance clause owed from #12 is therefore still owed, and issue #57
  carries the harness gap behind it.

The conformance harness asserts that every input here validates cleanly, then
compares both SDKs' production canonical serialisation byte for byte under
`agreement/`. It reports this input count separately from captured fixtures
and validation error cases. The corpus generator never reads this directory.

`../handwritten-validation-inputs/` has the opposite invariant: every input
there must produce its declared validation error. Keep the two sets separate
so an unexpected pass or failure remains visible.
