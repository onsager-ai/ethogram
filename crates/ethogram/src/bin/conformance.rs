use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use ethogram::{EVENT_SCHEMA_VERSION, parse_event, serialise_event};
use serde_json::json;

fn fixture_paths(corpus_directory: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut fixtures = fs::read_dir(corpus_directory)?
        .map(|entry| entry.map(|value| value.path()))
        .collect::<Result<Vec<_>, _>>()?;
    fixtures.retain(|path| path.is_file() && path.extension().is_some_and(|value| value == "json"));
    fixtures.sort();
    Ok(fixtures)
}

fn main() -> Result<(), Box<dyn Error>> {
    let output_directory = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: conformance <output-directory>")?;
    let corpus_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/v1");
    let fixtures = fixture_paths(&corpus_directory)?;

    fs::create_dir_all(&output_directory)?;
    for fixture in &fixtures {
        let source = fs::read_to_string(fixture)?;
        let event = parse_event(&source)?;
        let compact = serialise_event(&event)?;
        let output_name = fixture.file_name().ok_or("fixture path has no file name")?;
        fs::write(output_directory.join(output_name), compact)?;
    }

    fs::write(
        output_directory.join("_harness.json"),
        serde_json::to_string(&json!({
            "fixtures": fixtures.len(),
            "schemaVersion": EVENT_SCHEMA_VERSION
        }))?,
    )?;
    println!(
        "Rust conformance: prepared {} fixture{} for comparison.",
        fixtures.len(),
        if fixtures.len() == 1 { "" } else { "s" }
    );
    Ok(())
}
