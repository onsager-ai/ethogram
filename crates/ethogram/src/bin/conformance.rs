use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use ethogram::{
    AGENT_COMPLETED, AGENT_STARTED, AGENT_TEXT, AGENT_TOOL_RESULT, AGENT_TOOL_USE, AGENT_WARNING,
    AgentCompletedPayload, AgentStartedPayload, AgentTextPayload, AgentToolResultPayload,
    AgentToolUsePayload, AgentWarningPayload, CAPTURE_REFUSED, CONTROL_APPLIED, CONTROL_REQUESTED,
    CaptureRefusedPayload, ControlAppliedPayload, ControlRequestedPayload, DECISION_ANSWERED,
    DECISION_REQUESTED, DecisionAnsweredPayload, DecisionRequestedPayload, EVENT_SCHEMA_VERSION,
    Event, KNOWN_TYPES, RUN_FINISHED, RUN_STARTED, RunFinishedPayload, RunStartedPayload,
    parse_event, serialise_event, serialise_validation_error, validate,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

#[derive(serde::Deserialize)]
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

/// Deserialises `event.payload` into `T`, builds an `Event<T>` carrying the
/// same envelope fields, and serialises it through the same canonicaliser
/// `serialise_event` already uses for the untyped column. A deserialisation
/// failure here is a hard error: a known type whose payload will not load
/// into its own struct is exactly the defect this column exists to find, so
/// it must fail the run rather than being skipped.
fn write_typed_event<T>(event: &Event, path: &Path) -> Result<(), Box<dyn Error>>
where
    T: DeserializeOwned + Serialize,
{
    let typed_payload: T = serde_json::from_value(event.payload.clone())?;
    let typed_event = Event {
        v: event.v,
        event_type: event.event_type.clone(),
        run_id: event.run_id.clone(),
        seq: event.seq,
        ts: event.ts.clone(),
        payload: typed_payload,
        captured_at: event.captured_at.clone(),
    };
    let compact = serialise_event(&typed_event)?;
    fs::write(path, compact)?;
    Ok(())
}

/// Dispatches a known-type event to its typed payload struct and writes the
/// typed column to `path`.
///
/// Written as an `if`/`else if` chain against the exported `KNOWN_TYPES`
/// constants, not a `match`: `match s { RUN_STARTED => … }` would bind a
/// fresh variable named `RUN_STARTED` rather than compare against the
/// constant, and the compiler does not reliably catch that mistake.
/// `check_known_payload_representation` in `lib.rs` is deliberately written
/// the same way; this mirrors it so the two dispatches can't drift apart in
/// kind, only in whether they happen to list the same 13 arms.
///
/// The caller is expected to have already confirmed `event_type` is one of
/// `KNOWN_TYPES`. The trailing `else` is unreachable in a correctly
/// maintained dispatch — it exists only to fail loudly, naming the type,
/// if this chain and `KNOWN_TYPES` are ever allowed to drift apart, rather
/// than silently doing nothing.
fn write_typed_payload(event: &Event, path: &Path) -> Result<(), Box<dyn Error>> {
    let event_type = event.event_type.as_str();
    if event_type == RUN_STARTED {
        write_typed_event::<RunStartedPayload>(event, path)
    } else if event_type == RUN_FINISHED {
        write_typed_event::<RunFinishedPayload>(event, path)
    } else if event_type == AGENT_STARTED {
        write_typed_event::<AgentStartedPayload>(event, path)
    } else if event_type == AGENT_TEXT {
        write_typed_event::<AgentTextPayload>(event, path)
    } else if event_type == AGENT_TOOL_USE {
        write_typed_event::<AgentToolUsePayload>(event, path)
    } else if event_type == AGENT_TOOL_RESULT {
        write_typed_event::<AgentToolResultPayload>(event, path)
    } else if event_type == AGENT_COMPLETED {
        write_typed_event::<AgentCompletedPayload>(event, path)
    } else if event_type == AGENT_WARNING {
        write_typed_event::<AgentWarningPayload>(event, path)
    } else if event_type == CONTROL_REQUESTED {
        write_typed_event::<ControlRequestedPayload>(event, path)
    } else if event_type == CONTROL_APPLIED {
        write_typed_event::<ControlAppliedPayload>(event, path)
    } else if event_type == CAPTURE_REFUSED {
        write_typed_event::<CaptureRefusedPayload>(event, path)
    } else if event_type == DECISION_REQUESTED {
        write_typed_event::<DecisionRequestedPayload>(event, path)
    } else if event_type == DECISION_ANSWERED {
        write_typed_event::<DecisionAnsweredPayload>(event, path)
    } else {
        Err(format!(
            "internal error: {event_type} is in KNOWN_TYPES but write_typed_payload has no dispatch arm for it"
        )
        .into())
    }
}

/// Writes the typed column for one known-type input and records an inventory
/// entry for it, or records an untyped-only entry for an unrecognised type
/// without writing anything. `relative_path` is the input's path relative to
/// the typed output directory root (for example `run-started-basic.json` for
/// a fixture, `agreement/run-started-unknown-fields.json` for an agreement
/// input), and is also the key the run.sh reach assertion joins on.
fn record_typed_column(
    event: &Event,
    typed_output_directory: &Path,
    relative_path: &str,
    typed_entries: &mut Vec<Value>,
) -> Result<bool, Box<dyn Error>> {
    let is_known = KNOWN_TYPES.contains(&event.event_type.as_str());
    if is_known {
        let typed_path = typed_output_directory.join(relative_path);
        if let Some(parent) = typed_path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Name the input in the diagnostic. Serde's own error says only
        // which field it wanted ("missing field `textt`") and leaves a
        // reader with 32 inputs and no way to tell which one failed.
        //
        // This is defensive rather than a common path: a payload that will
        // not deserialise has already failed inside `parse_event`, whose
        // `check_known_payload_representation` decodes the same struct and
        // drops the result. What reaches here is a serialisation failure,
        // or a type this chain handles that that one does not — and if the
        // two dispatches ever drift, this is where it will surface.
        write_typed_payload(event, &typed_path).map_err(|error| {
            format!(
                "typed column failed for input {relative_path} (type {}): {error}",
                event.event_type
            )
        })?;
    }
    typed_entries.push(json!({
        "path": relative_path,
        "type": event.event_type,
        "typed": is_known,
    }));
    Ok(is_known)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let output_directory = args
        .next()
        .map(PathBuf::from)
        .ok_or("usage: conformance <output-directory> <typed-output-directory>")?;
    let typed_output_directory = args
        .next()
        .map(PathBuf::from)
        .ok_or("usage: conformance <output-directory> <typed-output-directory>")?;
    let corpus_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/v1");
    let fixtures = fixture_paths(&corpus_directory)?;
    let error_directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/handwritten-validation-inputs");
    let error_cases = fixture_paths(&error_directory)?;
    let agreement_directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/handwritten-agreement-inputs");
    let agreement_inputs = fixture_paths(&agreement_directory)?;

    fs::create_dir_all(&output_directory)?;
    fs::create_dir_all(&typed_output_directory)?;
    let mut typed_entries: Vec<Value> = Vec::new();
    let mut typed_count: usize = 0;
    let mut untyped_only_count: usize = 0;

    for fixture in &fixtures {
        let source = fs::read_to_string(fixture)?;
        let event = parse_event(&source)?;
        validate(&event.event_type, &event.payload)?;
        let compact = serialise_event(&event)?;
        let output_name = fixture.file_name().ok_or("fixture path has no file name")?;
        fs::write(output_directory.join(output_name), compact)?;

        let relative_path = output_name
            .to_str()
            .ok_or("fixture file name is not valid UTF-8")?;
        if record_typed_column(
            &event,
            &typed_output_directory,
            relative_path,
            &mut typed_entries,
        )? {
            typed_count += 1;
        } else {
            untyped_only_count += 1;
        }
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

    fs::create_dir_all(output_directory.join("agreement"))?;
    for input in &agreement_inputs {
        let event = parse_event(&fs::read_to_string(input)?)?;
        validate(&event.event_type, &event.payload).map_err(|error| {
            format!(
                "agreement input {} failed validation: {error}",
                input.display()
            )
        })?;
        let output_name = input
            .file_name()
            .ok_or("agreement input has no file name")?;
        fs::write(
            output_directory.join("agreement").join(output_name),
            serialise_event(&event)?,
        )?;

        let relative_path = format!(
            "agreement/{}",
            output_name
                .to_str()
                .ok_or("agreement input file name is not valid UTF-8")?
        );
        if record_typed_column(
            &event,
            &typed_output_directory,
            &relative_path,
            &mut typed_entries,
        )? {
            typed_count += 1;
        } else {
            untyped_only_count += 1;
        }
    }

    // The reach assertion (issue onsager-ai/ethogram#57, following onsager-ai/ethogram#55's lesson): bookkeeping
    // that says a typed column was produced must be checked against the
    // typed output directory actually holding that file, independently of
    // whatever the write step above believed it did. A driver bug that
    // quietly skips writing a known type's typed column must fail the run,
    // by input name, rather than pass with a smaller comparison.
    for entry in &typed_entries {
        if entry["typed"] == json!(true) {
            let relative_path = entry["path"]
                .as_str()
                .expect("typed inventory entry always carries a string path");
            if !typed_output_directory.join(relative_path).is_file() {
                return Err(format!(
                    "reach assertion failed: typed column missing for input {relative_path} \
                     (type {}); its type is in KNOWN_TYPES but no typed file was written",
                    entry["type"].as_str().unwrap_or("<unknown>")
                )
                .into());
            }
        }
    }

    fs::write(
        typed_output_directory.join("_typed.json"),
        serde_json::to_string(&json!({
            "inputs": typed_entries,
            "totals": {
                "typed": typed_count,
                "untypedOnly": untyped_only_count,
                "total": typed_count + untyped_only_count,
            },
        }))?,
    )?;

    fs::write(
        output_directory.join("_harness.json"),
        serde_json::to_string(&json!({
            "agreementInputs": agreement_inputs.len(),
            "errorCases": error_cases.len(),
            "fixtures": fixtures.len(),
            "schemaVersion": EVENT_SCHEMA_VERSION
        }))?,
    )?;
    println!(
        "Rust conformance: prepared {} fixture{}, {} validation error cases, and {} agreement inputs for comparison.",
        fixtures.len(),
        if fixtures.len() == 1 { "" } else { "s" },
        error_cases.len(),
        agreement_inputs.len()
    );
    println!(
        "Rust conformance: {typed_count} input{} took the typed path, {untyped_only_count} \
         input{} stayed untyped-only (unrecognised type).",
        if typed_count == 1 { "" } else { "s" },
        if untyped_only_count == 1 { "" } else { "s" },
    );
    Ok(())
}
