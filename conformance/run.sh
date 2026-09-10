#!/usr/bin/env bash

set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_directory="$(mktemp -d /tmp/ethogram-conformance.XXXXXX)"
typescript_output="$work_directory/typescript"
rust_output="$work_directory/rust"
rust_typed_output="$work_directory/rust-typed"

cleanup() {
  if [[ "$work_directory" == /tmp/ethogram-conformance.* ]]; then
    rm -rf -- "$work_directory"
  fi
}
trap cleanup EXIT

pnpm --dir "$repository_root/packages/ethogram" run conformance "$typescript_output"
# The crate's own manifest, not the tree root's. Cargo resolves the enclosing
# workspace upward from a crate manifest, so this holds whether that workspace
# is this tree's, a parent repository's after a fold, or none at all. Naming
# the workspace root instead would assume this tree root *is* one, which is an
# assumption a fold removes.
cargo run --quiet --manifest-path "$repository_root/crates/ethogram/Cargo.toml" --bin conformance -- "$rust_output" "$rust_typed_output"

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

# Third column (issue onsager-ai/ethogram#57): for every fixture and agreement input whose type
# Rust recognises, the driver also deserialises the payload into its typed
# struct and re-serialises it through the same canonicaliser, writing the
# result under $rust_typed_output. `_typed.json` there is the driver's own
# inventory of which inputs took that path; the block below re-derives the
# same fact from the directory on disk and fails if the two disagree, so a
# driver bug that quietly skips a known type's typed column (the onsager-ai/ethogram#55 lesson:
# a step that goes blind must not fail open) is caught here too, not just
# inside the driver.
typed_verifier="$work_directory/verify-typed-inventory.js"
cat >"$typed_verifier" <<'NODE_EOF'
'use strict';
const fs = require('node:fs');
const path = require('node:path');

const typedDirectory = process.argv[2];
const inventoryPath = path.join(typedDirectory, '_typed.json');

let raw;
try {
  raw = fs.readFileSync(inventoryPath, 'utf8');
} catch (error) {
  console.error(`typed inventory ${inventoryPath} is missing or unreadable: ${error.message}`);
  process.exit(1);
}

let inventory;
try {
  inventory = JSON.parse(raw);
} catch (error) {
  console.error(`typed inventory ${inventoryPath} is not valid JSON: ${error.message}`);
  process.exit(1);
}

if (!inventory || typeof inventory !== 'object' || !Array.isArray(inventory.inputs)) {
  console.error(`typed inventory ${inventoryPath} is malformed: expected an "inputs" array`);
  process.exit(1);
}

function listJsonFiles(directory, prefix) {
  let results = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    if (entry.name === '_typed.json') continue;
    const entryPath = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) {
      results = results.concat(listJsonFiles(path.join(directory, entry.name), entryPath));
    } else if (entry.isFile() && entry.name.endsWith('.json')) {
      results.push(entryPath);
    }
  }
  return results;
}

const onDisk = new Set(listJsonFiles(typedDirectory, ''));
const typedInInventory = new Set();
let failed = false;

for (const entry of inventory.inputs) {
  if (
    entry === null ||
    typeof entry !== 'object' ||
    typeof entry.path !== 'string' ||
    typeof entry.type !== 'string' ||
    typeof entry.typed !== 'boolean'
  ) {
    console.error(`typed inventory entry malformed: ${JSON.stringify(entry)}`);
    failed = true;
    continue;
  }
  if (entry.typed) {
    typedInInventory.add(entry.path);
    if (!onDisk.has(entry.path)) {
      console.error(
        `typed column missing for input ${entry.path} (${entry.type}): the inventory records it as typed but no file exists on disk for it`,
      );
      failed = true;
    }
  }
}

for (const diskPath of onDisk) {
  if (!typedInInventory.has(diskPath)) {
    console.error(
      `typed file ${diskPath} exists on disk but the inventory does not record it as typed`,
    );
    failed = true;
  }
}

if (failed) {
  process.exit(1);
}

for (const entry of inventory.inputs) {
  process.stdout.write(`${entry.typed ? 'typed' : 'untyped'}\t${entry.path}\t${entry.type}\n`);
}
NODE_EOF

typed_inventory_report="$work_directory/typed-inventory.tsv"
node "$typed_verifier" "$rust_typed_output" >"$typed_inventory_report"

typed_line_count="$(grep -c $'^typed\t' "$typed_inventory_report" || true)"
untyped_line_count="$(grep -c $'^untyped\t' "$typed_inventory_report" || true)"
typed_disk_count="$(find "$rust_typed_output" -type f -name '*.json' ! -name '_typed.json' | wc -l)"
typed_disk_count="${typed_disk_count//[[:space:]]/}"

test "$typed_disk_count" = "$typed_line_count"
test "$((typed_line_count + untyped_line_count))" = "$((fixture_count + agreement_count))"

while IFS=$'\t' read -r comparison_kind relative_path event_type; do
  if [[ "$comparison_kind" == "typed" ]]; then
    diff --unified "$typescript_output/$relative_path" "$rust_typed_output/$relative_path"
    diff --unified "$rust_output/$relative_path" "$rust_typed_output/$relative_path"
    echo "Conformance: $relative_path ($event_type) — compared 3 ways: typescript vs rust untyped, typed vs rust untyped, typed vs typescript."
  else
    echo "Conformance: $relative_path ($event_type) — compared 1 way: typescript vs rust untyped; type is unrecognised, no typed column."
  fi
done <"$typed_inventory_report"

if [[ "$fixture_count" == "0" ]]; then
  echo "Conformance: compared 0 fixtures across TypeScript and Rust; v1 is intentionally empty until the first real capture."
else
  echo "Conformance: compared $fixture_count fixtures across TypeScript and Rust."
fi
echo "Conformance: compared $error_count validation error cases across TypeScript and Rust."
echo "Conformance: compared $agreement_count agreement inputs across TypeScript and Rust."
echo "Conformance: $typed_line_count input(s) took the typed path, $untyped_line_count stayed untyped-only, across TypeScript and Rust."
