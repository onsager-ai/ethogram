//! Embedded access to ethogram's canonical conformance corpus.
//!
//! This crate exposes a view of `conformance/v1`, never a second independently
//! maintained copy of it. The generated fixture registry uses `include_str!`
//! to embed the canonical files themselves, and CI regenerates the registry
//! from the directory and rejects any drift in either direction.
//!
//! The corpus is a set of independent envelopes, not a stream. Fixtures sharing
//! a `runId` are separate captures that happened to synthesise the same id;
//! folding the corpus by `runId` is an unsupported misuse. The inventory test
//! `no_two_fixtures_share_a_run_id_and_seq` in `tests/corpus.rs` records the
//! surviving historical collisions.

mod corpus;

/// One canonical event fixture from the version 1 corpus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Fixture {
    /// The fixture's file name within `conformance/v1`.
    pub name: &'static str,
    /// The fixture file's exact UTF-8 text, including presentation whitespace.
    pub raw_json: &'static str,
}

impl Fixture {
    /// Parses the fixture through the protocol SDK.
    ///
    /// The committed corpus is expected to make this infallible. The result is
    /// retained so a malformed generated or canonical input is never hidden.
    pub fn parse(&self) -> serde_json::Result<ethogram::Event> {
        ethogram::parse_event(self.raw_json)
    }
}

/// Returns every version 1 fixture in UTF-8 byte-order by file name.
///
/// These are independent envelopes, not a stream. Fixtures sharing a `runId`
/// are separate captures that happened to synthesise the same id; folding this
/// collection by `runId` is an unsupported misuse. The inventory test
/// `no_two_fixtures_share_a_run_id_and_seq` in `tests/corpus.rs` records the
/// surviving historical collisions.
#[must_use]
pub fn v1_fixtures() -> &'static [Fixture] {
    corpus::V1_FIXTURES
}
