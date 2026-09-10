//! Tests for the compiled-in corpus accessor, `ethogram::v1_fixtures()`.
//!
//! This crate's own `build.rs` compiles `conformance/v1` in; these tests
//! prove the compiled set is not merely present but exactly agrees with the
//! directory it was built from. That is a reach assertion on purpose: the
//! build script is a *selection* step, and a selection step that goes blind
//! — silently dropping or substituting a fixture — must fail loudly rather
//! than leave every downstream user of `v1_fixtures()` checking nothing.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use ethogram::v1_fixtures;

fn corpus_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/v1")
}

fn disk_fixtures() -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let mut fixtures = fs::read_dir(corpus_directory())?
        .map(|entry| {
            let path = entry?.path();
            if !path.is_file() || path.extension().is_none_or(|extension| extension != "json") {
                return Ok(None);
            }
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or("fixture file name is not valid UTF-8")?
                .to_owned();
            Ok(Some((name, fs::read_to_string(path)?)))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    fixtures.sort_by(|(left, _), (right, _)| left.as_bytes().cmp(right.as_bytes()));
    Ok(fixtures)
}

/// The important assertion in this file. A `len() == 31` check alone would
/// pass a build that compiled in 31 fixtures none of which were the right
/// ones — a substitution, not just an omission. This instead walks the
/// directory listing and the compiled-in slice in lockstep and names exactly
/// which position and file disagree, so a build-script regression that skips
/// or swaps a fixture fails naming the mismatch rather than a bare count.
#[test]
fn the_compiled_in_set_matches_the_directory_exactly() -> Result<(), Box<dyn Error>> {
    let expected = disk_fixtures()?;
    let actual = v1_fixtures();

    assert_eq!(
        expected.len(),
        31,
        "conformance/v1 on disk does not hold 31 fixtures; this test's own \
         expectation is stale, not the accessor"
    );

    let mismatches = expected
        .iter()
        .enumerate()
        .zip(actual.iter().map(Some).chain(std::iter::repeat(None)))
        .filter_map(|((index, (expected_name, expected_json)), compiled)| match compiled {
            None => Some(format!(
                "position {index}: directory has {expected_name:?} but the compiled-in set has no entry there — the compiled-in set does not match the directory (it is short)"
            )),
            Some(fixture) if fixture.name != expected_name.as_str() => Some(format!(
                "position {index}: directory has {expected_name:?} but the compiled-in set has {:?} — the compiled-in set does not match the directory (a fixture was skipped or substituted)",
                fixture.name
            )),
            Some(fixture) if fixture.raw_json.as_bytes() != expected_json.as_bytes() => Some(format!(
                "position {index} ({expected_name:?}): the compiled-in bytes do not match the file on disk — the compiled-in set does not match the directory (stale or corrupted include_str!)"
            )),
            Some(_) => None,
        })
        .collect::<Vec<_>>();

    let mut problems = mismatches;
    if actual.len() > expected.len() {
        for fixture in &actual[expected.len()..] {
            problems.push(format!(
                "compiled-in set carries {:?}, which is not in the conformance/v1 directory — the compiled-in set does not match the directory (it is long)",
                fixture.name
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "the compiled-in set does not match the directory:\n{}",
        problems.join("\n")
    );

    assert_eq!(
        actual.len(),
        31,
        "ethogram::v1_fixtures() returned {} fixtures, not the expected 31 — \
         the compiled-in set does not match the directory",
        actual.len()
    );

    Ok(())
}

/// Every fixture parses, and the parsed event's `type` is the one the file
/// on disk carries — proving `Fixture::parse()` is not merely infallible but
/// faithful to the fixture's own content.
#[test]
fn every_fixture_parses_to_the_type_its_file_carries() -> Result<(), Box<dyn Error>> {
    for fixture in v1_fixtures() {
        let raw: serde_json::Value = serde_json::from_str(fixture.raw_json)?;
        let expected_type = raw
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("{:?} has no string \"type\" field", fixture.name))?;

        let event = fixture
            .parse()
            .map_err(|error| format!("{:?} failed to parse: {error}", fixture.name))?;

        assert_eq!(
            event.event_type, expected_type,
            "{:?} parsed to type {:?}, but the file carries {:?}",
            fixture.name, event.event_type, expected_type
        );
    }

    Ok(())
}
