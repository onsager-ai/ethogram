# Handwritten validation inputs — not captured events

These JSON files are invalid inputs for `validate`, separate from the
immutable real captures in `../v1/`. They are not corpus fixtures and are
not included in either generated corpus library.

Each file contains `type`, `payload`, and `expectedKind`. Both harnesses must
observe an error of that kind; clean validation is a harness failure. They
write canonical errors into their existing output directories, where
`../run.sh` compares the SDKs' exact bytes.

- `over-bound.json`: a dossier question containing 4,097 emoji scalars,
  exceeding the field bound of 4,096 while staying below the universal bound.
- `payload-too-large.json`: two strings of 16,384 emoji scalars each. Each
  string meets the scalar bound; together they exceed the byte bound. The
  numbers also exercise integral floats, negative zero, and small-decimal
  notation in the canonical size measurement. The strings are literal JSON
  input so the harness needs no recipe language or SDK-specific generator.
- `unknown-member.json`: an unfamiliar run outcome.
- `missing-field.json`: a dossier missing its required question.
- `policy.json`: a steer without text.
- `malformed.json`: a `run.started` carrying an unrecognised extra field whose
  value is a number one past the safe-integer magnitude bound
  (`Number.MAX_SAFE_INTEGER + 1`, still exactly representable in both a JS
  double and a Rust `u64`). This exercises the universal number-magnitude
  check.
- `wrong-type-required-string.json` and `wrong-type-optional-string.json`:
  integers in `run.started.actor` and `parentRunId`, pinning the presence
  suffix only on the optional field.
- `wrong-type-required-boolean.json` and `wrong-type-optional-boolean.json`:
  strings in `control.applied.ok` and `agent.text.truncated`.
- `wrong-type-finite-number.json`, `wrong-type-required-integer.json`, and
  `wrong-type-optional-integer.json`: strings in `run.finished.costUsd`,
  `run.finished.durationMs`, and `agent.started.pid`.
- `wrong-type-nested-field.json`: a string in `run.started.ceilings.tokens`,
  retaining the parent payload name and the optional integer suffix.
- `wrong-type-optional-object.json` and `wrong-type-required-object.json`:
  integers in `run.started.ceilings` and `decision.requested.dossier`.
  TypeScript's object message omits the presence suffix in both cases.
- `wrong-type-array.json` and `wrong-type-string-array.json`: objects in
  `decision.requested.options` and `dossier.optionsRuledOut`.
- `wrong-type-array-item.json`, `wrong-type-array-item-field.json`, and
  `wrong-type-string-array-item.json`: integers in `options[1]`,
  `options[0].label`, and `dossier.optionsRuledOut[1]`, retaining array indices
  in both the path and the diagnostic.
- `wrong-type-payload.json`: a boolean instead of an `agent.text` object.

The sixteen wrong-type cases close the gap described in onsager-ai/ethogram#42: Rust now authors
the same messages as TypeScript, so the harness compares their `kind`, `path`,
and `message` byte-for-byte. Returning serde's diagnostic again must fail the
recursive diff with the offending input's name. These messages may become
`capture.refused.detail`, which remains non-authoritative despite this byte
agreement; see [the spec](../README.md#handwritten-validation-inputs).
