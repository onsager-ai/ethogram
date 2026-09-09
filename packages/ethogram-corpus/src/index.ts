/**
 * Embedded access to ethogram's canonical conformance corpus.
 *
 * This package exposes a view of `conformance/v1`, never a second
 * independently maintained copy. CI regenerates the embedded module from the
 * canonical directory and rejects drift in either direction.
 *
 * The corpus is a set of independent envelopes, not a stream. Fixtures sharing
 * a `runId` are separate captures that happened to synthesise the same id;
 * grouping the corpus by `runId` and calling `foldRun` is an unsupported misuse.
 * The inventory test `no_two_fixtures_share_a_run_id_and_seq` in
 * `crates/ethogram-corpus/tests/corpus.rs` records the surviving historical
 * collisions.
 *
 * That test is deliberately the only one, and this package has no twin of it.
 * It inventories the corpus *files*, not either SDK's behaviour, so principle 1
 * is already satisfied by there being one definition. A TypeScript copy would
 * mean a second exception list free to drift from the first — which is the
 * failure the list exists to prevent.
 */

import { parseEvent, type Event } from "@onsager-ai/ethogram";

import { V1_FIXTURES } from "./corpus.js";

/** One canonical event fixture from the version 1 corpus. */
export interface CorpusFixture {
  /** The fixture's file name within `conformance/v1`. */
  readonly name: string;
  /** The fixture file's exact UTF-8 text, including presentation whitespace. */
  readonly rawJson: string;
}

/**
 * Every version 1 fixture in UTF-8 byte-order by file name.
 *
 * These are independent envelopes, not a stream. Fixtures sharing a `runId`
 * are separate captures that happened to synthesise the same id; grouping this
 * collection by `runId` and calling `foldRun` is an unsupported misuse.
 * The inventory test `no_two_fixtures_share_a_run_id_and_seq` in
 * `crates/ethogram-corpus/tests/corpus.rs` records the surviving historical
 * collisions.
 */
export const v1Fixtures: readonly CorpusFixture[] = V1_FIXTURES;

/** Parse a corpus fixture through the protocol SDK. */
export function parseFixture(fixture: CorpusFixture): Event {
  return parseEvent(JSON.parse(fixture.rawJson));
}
