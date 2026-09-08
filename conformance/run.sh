#!/usr/bin/env bash

set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_directory="$(mktemp -d /tmp/ethogram-conformance.XXXXXX)"
typescript_output="$work_directory/typescript"
rust_output="$work_directory/rust"

cleanup() {
  if [[ "$work_directory" == /tmp/ethogram-conformance.* ]]; then
    rm -rf -- "$work_directory"
  fi
}
trap cleanup EXIT

pnpm --dir "$repository_root/packages/ethogram" run conformance "$typescript_output"
cargo run --quiet --manifest-path "$repository_root/Cargo.toml" --package ethogram --bin conformance -- "$rust_output"

test -f "$typescript_output/_harness.json"
test -f "$rust_output/_harness.json"

fixture_count="$(find "$repository_root/conformance/v1" -maxdepth 1 -type f -name '*.json' | wc -l)"
fixture_count="${fixture_count//[[:space:]]/}"
error_count="$(find "$repository_root/conformance/handwritten-validation-inputs" -maxdepth 1 -type f -name '*.json' | wc -l)"
error_count="${error_count//[[:space:]]/}"
agreement_count="$(find "$repository_root/conformance/handwritten-agreement-inputs" -maxdepth 1 -type f -name '*.json' | wc -l)"
agreement_count="${agreement_count//[[:space:]]/}"
typescript_count="$(find "$typescript_output" -maxdepth 1 -type f -name '*.json' ! -name '_harness.json' | wc -l)"
typescript_count="${typescript_count//[[:space:]]/}"
rust_count="$(find "$rust_output" -maxdepth 1 -type f -name '*.json' ! -name '_harness.json' | wc -l)"
rust_count="${rust_count//[[:space:]]/}"
typescript_error_count="$(find "$typescript_output/errors" -maxdepth 1 -type f -name '*.json' | wc -l)"
typescript_error_count="${typescript_error_count//[[:space:]]/}"
rust_error_count="$(find "$rust_output/errors" -maxdepth 1 -type f -name '*.json' | wc -l)"
rust_error_count="${rust_error_count//[[:space:]]/}"
typescript_agreement_count="$(find "$typescript_output/agreement" -maxdepth 1 -type f -name '*.json' | wc -l)"
typescript_agreement_count="${typescript_agreement_count//[[:space:]]/}"
rust_agreement_count="$(find "$rust_output/agreement" -maxdepth 1 -type f -name '*.json' | wc -l)"
rust_agreement_count="${rust_agreement_count//[[:space:]]/}"
expected_manifest="{\"agreementInputs\":$agreement_count,\"errorCases\":$error_count,\"fixtures\":$fixture_count,\"schemaVersion\":1}"
test "$(<"$typescript_output/_harness.json")" = "$expected_manifest"
test "$(<"$rust_output/_harness.json")" = "$expected_manifest"
test "$typescript_count" = "$fixture_count"
test "$rust_count" = "$fixture_count"
test "$error_count" -gt 0
test "$typescript_error_count" = "$error_count"
test "$rust_error_count" = "$error_count"
test "$agreement_count" -gt 0
test "$typescript_agreement_count" = "$agreement_count"
test "$rust_agreement_count" = "$agreement_count"

diff --recursive --unified "$typescript_output" "$rust_output"

if [[ "$fixture_count" == "0" ]]; then
  echo "Conformance: compared 0 fixtures across TypeScript and Rust; v1 is intentionally empty until the first real capture."
else
  echo "Conformance: compared $fixture_count fixtures across TypeScript and Rust."
fi
echo "Conformance: compared $error_count validation error cases across TypeScript and Rust."
echo "Conformance: compared $agreement_count agreement inputs across TypeScript and Rust."
