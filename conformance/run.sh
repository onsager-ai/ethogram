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
typescript_count="$(find "$typescript_output" -maxdepth 1 -type f -name '*.json' ! -name '_harness.json' | wc -l)"
typescript_count="${typescript_count//[[:space:]]/}"
rust_count="$(find "$rust_output" -maxdepth 1 -type f -name '*.json' ! -name '_harness.json' | wc -l)"
rust_count="${rust_count//[[:space:]]/}"
expected_manifest="{\"fixtures\":$fixture_count,\"schemaVersion\":1}"
test "$(<"$typescript_output/_harness.json")" = "$expected_manifest"
test "$(<"$rust_output/_harness.json")" = "$expected_manifest"
test "$typescript_count" = "$fixture_count"
test "$rust_count" = "$fixture_count"

diff --recursive --unified "$typescript_output" "$rust_output"

if [[ "$fixture_count" == "0" ]]; then
  echo "Conformance: compared 0 fixtures across TypeScript and Rust; v1 is intentionally empty until #5."
else
  echo "Conformance: compared $fixture_count fixtures across TypeScript and Rust."
fi
