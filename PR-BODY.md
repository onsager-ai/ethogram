fix(validation): reject known spellings in typed unknown members

A typed `RunKind::Unknown("relay")` previously validated successfully even
though parsing its wire string yields `RunKind::Relay`. Apply the same
`Malformed` rule already enforced for `ControlKind` to `RunKind`,
`RunOutcome`, and `CaptureRefusalCause`, with exact `payload.<field>` paths
and the existing diagnostic form.

Generalise the Serde probe into `union_unknown_validation`. One declarative
registration per union generates its transparent Unknown marker and connects
its event, payload field, and existing deserializer to a shared check. A new
union needs a registration rather than another validation implementation.
The probe runs on the typed payload before `serde_json::to_value` erases the
variant. Tests show typed Unknown values fail while their identical JSON
values and known variants validate successfully.

Each union has its own tests for all known members and for an unfamiliar
Unicode string. Unfamiliar strings still report `UnknownMember` with exact
paths and retain their bytes. Existing cross-SDK literal tests also assert
that known members and Unknown spellings emit the same pinned event bytes.
TypeScript behaviour and all 24 captured fixtures remain unchanged.

Add the revision-oriented Unreleased changelog entry and correct the README's
stale claim that unfamiliar control kinds validate successfully. The code
also already contains `DecisionKind`; this change follows #41's explicit
scope of the four named unions and does not change decision validation.

Validation passed:

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
- `cargo test --features preserve-order-probe`
- `pnpm -r run typecheck`
- `pnpm -r test`
- `./conformance/run.sh`: 24 fixtures, 22 validation error cases, 5 agreement inputs
- `pnpm run generate:corpus` followed by `git diff --exit-code`

Mutation verification: bypassed each event's registered probe in turn,
keeping all other rules active, and ran the entire `union_unknown` test
binary. Every mutation produced exactly 8 passes and 1 failure, naming the
corresponding test below. All four mutations were restored before the full
verification suite.

| Disabled union | Failing test | Quoted panic |
| --- | --- | --- |
| `RunKind` | `run_kind_unknown_cannot_spell_known_member` | `RunKind::Unknown spelling a known member must be Malformed: ()` |
| `RunOutcome` | `run_outcome_unknown_cannot_spell_known_member` | `RunOutcome::Unknown spelling a known member must be Malformed: ()` |
| `ControlKind` | `control_kind_unknown_cannot_spell_known_member` | `ControlKind::Unknown spelling a known member must be Malformed: ()` |
| `CaptureRefusalCause` | `capture_refusal_cause_unknown_cannot_spell_known_member` | `CaptureRefusalCause::Unknown spelling a known member must be Malformed: ()` |

Fixes #41.
