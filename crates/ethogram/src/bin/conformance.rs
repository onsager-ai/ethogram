use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use ethogram::{
    EVENT_SCHEMA_VERSION, parse_event, serialise_event, serialise_validation_error, validate,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ValidationInput {
    #[serde(rename = "type")]
    event_type: String,
    payload: Value,
    expected_kind: String,
}

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
    let error_directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/handwritten-validation-inputs");
    let error_cases = fixture_paths(&error_directory)?;

    fs::create_dir_all(&output_directory)?;
    for fixture in &fixtures {
        let source = fs::read_to_string(fixture)?;
        let event = parse_event(&source)?;
        let compact = serialise_event(&event)?;
        let output_name = fixture.file_name().ok_or("fixture path has no file name")?;
        fs::write(output_directory.join(output_name), compact)?;
    }

    fs::create_dir_all(output_directory.join("errors"))?;
    for case in &error_cases {
        let input: ValidationInput = serde_json::from_str(&fs::read_to_string(case)?)?;
        let error = validate(&input.event_type, &input.payload)
            .err()
            .ok_or_else(|| {
                format!(
                    "validation input {} unexpectedly validated cleanly",
                    case.display()
                )
            })?;
        let actual_kind = serde_json::to_value(&error)?;
        if actual_kind["kind"] != input.expected_kind {
            return Err(format!(
                "validation input {} expected {}, received {}",
                case.display(),
                input.expected_kind,
                actual_kind["kind"]
            )
            .into());
        }
        let output_name = case
            .file_name()
            .ok_or("validation input has no file name")?;
        fs::write(
            output_directory.join("errors").join(output_name),
            serialise_validation_error(&error)?,
        )?;
    }

    fs::write(
        output_directory.join("_harness.json"),
        serde_json::to_string(&json!({
            "errorCases": error_cases.len(),
            "fixtures": fixtures.len(),
            "schemaVersion": EVENT_SCHEMA_VERSION
        }))?,
    )?;
    println!(
        "Rust conformance: prepared {} fixture{} and {} error cases for comparison.",
        fixtures.len(),
        if fixtures.len() == 1 { "" } else { "s" },
        error_cases.len()
    );
    Ok(())
}
