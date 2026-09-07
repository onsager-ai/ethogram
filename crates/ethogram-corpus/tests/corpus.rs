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
