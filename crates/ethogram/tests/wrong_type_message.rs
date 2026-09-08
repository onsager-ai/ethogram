use ethogram::{Event, ValidationErrorKind, parse_event, validate};
use serde_json::{Value, json};

fn assert_wrong_type(event_type: &str, payload: Value, path: &str, message: &str) {
    let error = validate(event_type, &payload).unwrap_err();
    assert_eq!(error.to_string(), message);
    assert_eq!(
        error.kind,
        ValidationErrorKind::Malformed {
            path: path.to_owned(),
            message: message.to_owned(),
        }
    );
    // Relays may refuse at parse time too. Both entry points must author
    // the same diagnostic before it is excerpted into capture.refused.detail.
    let input = serde_json::to_string(&Event {
        v: 1,
        event_type: event_type.to_owned(),
        run_id: "r".to_owned(),
        seq: 1,
        ts: "2026-09-08T00:00:00.000Z".to_owned(),
        payload,
        captured_at: None,
    })
    .unwrap();
    assert_eq!(parse_event(&input).unwrap_err().to_string(), message);
}

#[test]
fn handwritten_wrong_type_inputs_match_measured_typescript_messages() {
    // Exact output measured from the unchanged TypeScript SDK for each input.
    // The harness separately compares production serialise_validation_error
    // bytes, so a change to either language's message cannot silently pass.
    for (name, path, message) in [
        (
            "wrong-type-array-item-field.json",
            "payload.options[0].label",
            "DecisionRequestedPayload.options[0].label must be a string",
        ),
        (
            "wrong-type-array-item.json",
            "payload.options[1]",
            "DecisionRequestedPayload.options[1] must be an object",
        ),
        (
            "wrong-type-array.json",
            "payload.options",
            "DecisionRequestedPayload.options must be an array",
        ),
        (
            "wrong-type-finite-number.json",
            "payload.costUsd",
            "RunFinishedPayload.costUsd must be a finite number when present",
        ),
        (
            "wrong-type-nested-field.json",
            "payload.ceilings.tokens",
            "RunStartedPayload.ceilings.tokens must be a non-negative safe integer when present",
        ),
        (
            "wrong-type-optional-boolean.json",
            "payload.truncated",
            "AgentTextPayload.truncated must be a boolean when present",
        ),
        (
            "wrong-type-optional-integer.json",
            "payload.pid",
            "AgentStartedPayload.pid must be a non-negative safe integer when present",
        ),
        (
            "wrong-type-optional-object.json",
            "payload.ceilings",
            "RunStartedPayload.ceilings must be an object",
        ),
        (
            "wrong-type-optional-string.json",
            "payload.parentRunId",
            "RunStartedPayload.parentRunId must be a string when present",
        ),
        (
            "wrong-type-payload.json",
            "payload",
            "AgentTextPayload must be an object",
        ),
        (
            "wrong-type-required-boolean.json",
            "payload.ok",
            "ControlAppliedPayload.ok must be a boolean",
        ),
        (
            "wrong-type-required-integer.json",
            "payload.durationMs",
            "RunFinishedPayload.durationMs must be a non-negative safe integer",
        ),
        (
            "wrong-type-required-object.json",
            "payload.dossier",
            "DecisionRequestedPayload.dossier must be an object",
        ),
        (
            "wrong-type-required-string.json",
            "payload.actor",
            "RunStartedPayload.actor must be a string",
        ),
        (
            "wrong-type-string-array-item.json",
            "payload.dossier.optionsRuledOut[1]",
            "DecisionRequestedPayload.dossier.optionsRuledOut[1] must be a string",
        ),
        (
            "wrong-type-string-array.json",
            "payload.dossier.optionsRuledOut",
            "DecisionRequestedPayload.dossier.optionsRuledOut must be an array",
        ),
    ] {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../conformance/handwritten-validation-inputs")
                .join(name),
        )
        .unwrap();
        let input: Value = serde_json::from_str(&source).unwrap();
        assert_wrong_type(
            input["type"].as_str().unwrap(),
            input["payload"].clone(),
            path,
            message,
        );
    }
}

#[test]
fn optional_nulls_retain_the_expected_type_and_presence_suffix() {
    for (event_type, payload, path, message) in [
        (
            "run.started",
            json!({"kind": "loop", "actor": "a", "harness": "h", "parentRunId": null}),
            "payload.parentRunId",
            "RunStartedPayload.parentRunId must be a string when present",
        ),
        (
            "agent.text",
            json!({"text": "hello", "truncated": null}),
            "payload.truncated",
            "AgentTextPayload.truncated must be a boolean when present",
        ),
        (
            "agent.completed",
            json!({"turns": null}),
            "payload.turns",
            "AgentCompletedPayload.turns must be a non-negative safe integer when present",
        ),
        (
            "agent.completed",
            json!({"costUsd": null}),
            "payload.costUsd",
            "AgentCompletedPayload.costUsd must be a finite number when present",
        ),
        (
            "run.finished",
            json!({"outcome": "completed", "durationMs": 1, "usage": null}),
            "payload.usage",
            "RunFinishedPayload.usage must be an object",
        ),
    ] {
        assert_wrong_type(event_type, payload, path, message);
    }
}

#[test]
fn shared_usage_keeps_the_parent_label_and_integer_errors_keep_their_type() {
    for invalid in [
        json!(-1),
        json!(1.5),
        json!("1"),
        Value::Null,
        json!([]),
        json!({}),
        json!(true),
    ] {
        for (event_type, mut payload, name) in [
            (
                "run.finished",
                json!({"outcome": "completed", "durationMs": 1}),
                "RunFinishedPayload",
            ),
            ("agent.completed", json!({}), "AgentCompletedPayload"),
        ] {
            payload["usage"] = json!({"inputTokens": invalid});
            assert_wrong_type(
                event_type,
                payload,
                "payload.usage.inputTokens",
                &format!(
                    "{name}.usage.inputTokens must be a non-negative safe integer when present"
                ),
            );
        }
        assert_wrong_type(
            "run.finished",
            json!({"outcome": "completed", "durationMs": invalid}),
            "payload.durationMs",
            "RunFinishedPayload.durationMs must be a non-negative safe integer",
        );
    }
}

#[test]
fn every_known_payload_authors_its_object_message() {
    let cases = [
        ("run.started", "RunStartedPayload"),
        ("run.finished", "RunFinishedPayload"),
        ("agent.started", "AgentStartedPayload"),
        ("agent.text", "AgentTextPayload"),
        ("agent.tool_use", "AgentToolUsePayload"),
        ("agent.tool_result", "AgentToolResultPayload"),
        ("agent.completed", "AgentCompletedPayload"),
        ("agent.warning", "AgentWarningPayload"),
        ("control.requested", "ControlRequestedPayload"),
        ("control.applied", "ControlAppliedPayload"),
        ("capture.refused", "CaptureRefusedPayload"),
        ("decision.requested", "DecisionRequestedPayload"),
        ("decision.answered", "DecisionAnsweredPayload"),
    ];
    assert_eq!(cases.len(), ethogram::KNOWN_TYPES.len());
    for (event_type, name) in cases {
        for invalid in [
            Value::Null,
            json!(1),
            json!(true),
            json!("object"),
            json!([]),
        ] {
            assert_wrong_type(
                event_type,
                invalid,
                "payload",
                &format!("{name} must be an object"),
            );
        }
    }
}
