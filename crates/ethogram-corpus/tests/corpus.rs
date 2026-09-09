use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use ethogram::{parse_event, serialise_event};
use ethogram_corpus::v1_fixtures;

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

#[test]
fn no_two_fixtures_share_a_run_id_and_seq() -> Result<(), Box<dyn Error>> {
    struct CollisionException {
        run_id: &'static str,
        seq: u64,
        fixtures: &'static [&'static str],
        reason: &'static str,
    }

    // Rust only: this inventories the corpus files, not either SDK's behaviour.
    // Principle 1 needs one definition here; a TypeScript copy would introduce
    // a second exception list that could drift. Published fixtures are immutable,
    // so retain these historical collisions; the list may only shrink.
    const EXCEPTIONS: &[CollisionException] = &[
        CollisionException {
            run_id: "judgment-20300102T030405000Z-fixture-0",
            seq: 2,
            fixtures: &[
                "decision-answered-excuse-requested-run.json",
                "decision-answered-excuse.json",
            ],
            reason: "backward-compatibility pair documented in conformance/README.md: the same event before and after decision.answered gained requestedRunId, two captures of one shape differing by one line",
        },
        CollisionException {
            run_id: "sweep",
            seq: 2,
            fixtures: &[
                "decision-requested-human-decides-options.json",
                "decision-requested-tripwire.json",
            ],
            reason: "separate captures whose synthesised run id was the literal \"sweep\", taken before this rule existed",
        },
        CollisionException {
            run_id: "sweep",
            seq: 3,
            fixtures: &[
                "decision-requested-human-decides.json",
                "decision-requested-unclassified.json",
            ],
            reason: "separate captures whose synthesised run id was the literal \"sweep\", taken before this rule existed",
        },
    ];

    let mut collisions = BTreeMap::<(String, u64), BTreeSet<String>>::new();
    for (name, raw_json) in disk_fixtures()? {
        let event = parse_event(&raw_json)?;
        collisions
            .entry((event.run_id, event.seq))
            .or_default()
            .insert(name);
    }
    collisions.retain(|_, names| names.len() > 1);

    let mut problems = Vec::new();
    for exception in EXCEPTIONS {
        let key = (exception.run_id.to_owned(), exception.seq);
        let expected = exception
            .fixtures
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>();
        match collisions.remove(&key) {
            None => problems.push(format!(
                "stale collision exception: runId {:?}, seq {}, listed fixtures {:?} no longer collide; remove the entry (reason: {})",
                exception.run_id, exception.seq, expected, exception.reason,
            )),
            Some(actual) if actual != expected => problems.push(format!(
                "collision fixture-name set changed: runId {:?}, seq {}, expected {:?}, found {:?}; new captures must use a distinct key, and withdrawn fixtures must leave the list (reason: {})",
                exception.run_id, exception.seq, expected, actual, exception.reason,
            )),
            Some(_) => {}
        }
    }
    for ((run_id, seq), names) in collisions {
        problems.push(format!(
            "unlisted collision: runId {run_id:?}, seq {seq}, fixtures {names:?}; new captures must use a distinct (runId, seq)",
        ));
    }
    assert!(
        problems.is_empty(),
        "corpus collision inventory does not match the historical exceptions:\n{}",
        problems.join("\n"),
    );

    Ok(())
}

#[test]
fn exposes_the_exact_directory_view() -> Result<(), Box<dyn Error>> {
    let expected = disk_fixtures()?;
    let actual = v1_fixtures();

    assert_eq!(actual.len(), expected.len());
    for (fixture, (expected_name, expected_json)) in actual.iter().zip(expected) {
        assert_eq!(fixture.name, expected_name);
        assert_eq!(fixture.raw_json.as_bytes(), expected_json.as_bytes());
    }

    Ok(())
}

#[test]
fn every_fixture_round_trips_to_the_canonical_file_form() -> Result<(), Box<dyn Error>> {
    for fixture in v1_fixtures() {
        let disk_json = fs::read_to_string(corpus_directory().join(fixture.name))?;
        let canonical_disk = serialise_event(&parse_event(&disk_json)?)?;
        let canonical_exposed = serialise_event(&fixture.parse()?)?;

        assert_eq!(canonical_exposed.as_bytes(), canonical_disk.as_bytes());
    }

    Ok(())
}
