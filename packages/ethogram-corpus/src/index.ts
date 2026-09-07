/**
 * Embedded access to ethogram's canonical conformance corpus.
 *
 * This package exposes a view of `conformance/v1`, never a second
 * independently maintained copy. CI regenerates the embedded module from the
 * canonical directory and rejects drift in either direction.
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

/** Every version 1 fixture in UTF-8 byte-order by file name. */
export const v1Fixtures: readonly CorpusFixture[] = V1_FIXTURES;

/** Parse a corpus fixture through the protocol SDK. */
export function parseFixture(fixture: CorpusFixture): Event {
  return parseEvent(JSON.parse(fixture.rawJson));
}
