/**
 * Embedded access to ethogram's canonical conformance corpus.
 *
 * This package exposes a view of `conformance/v1`, never a second
 * independently maintained copy. CI regenerates the embedded module from the
 * canonical directory and rejects drift in either direction.
 *
 * The corpus is a set of independent envelopes, not a stream, and grouping it
 * by `runId` to call `foldRun` is an unsupported misuse. Several fixtures may
 * share a `runId` as a selection from one capture's stream rather than the
 * whole of it — so a fold sees a run with holes — and two historical groups go
 * further, holding separate captures whose synthesised id collided. The
 * inventory test `every_run_id_belongs_to_exactly_one_capture` in
 * `crates/ethogram-corpus/tests/corpus.rs` pins every such group.
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
 * These are independent envelopes, not a stream, and grouping this collection
 * by `runId` to call `foldRun` is an unsupported misuse. Several fixtures may
 * share a `runId` as a selection from one capture's stream rather than the
 * whole of it, and two historical groups hold separate captures whose
 * synthesised id collided. The inventory test
 * `every_run_id_belongs_to_exactly_one_capture` in
 * `crates/ethogram-corpus/tests/corpus.rs` pins every such group.
 */
export const v1Fixtures: readonly CorpusFixture[] = V1_FIXTURES;

/** Parse a corpus fixture through the protocol SDK. */
export function parseFixture(fixture: CorpusFixture): Event {
  return parseEvent(JSON.parse(fixture.rawJson));
}
