# Handwritten validation inputs — not captured events

These six JSON files are invalid inputs for `validate`, separate from the
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
  check rather than one of the many per-field "must be a string"-style
  messages: those are hand-written independently in each SDK and never
  promised to agree word for word, while the universal check's message is
  deliberately authored identically in both, so this is the representation
  failure this harness can compare byte-for-byte without inventing new
  cross-SDK wording.
