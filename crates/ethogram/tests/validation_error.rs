use ethogram::{
    AGENT_COMPLETED, AGENT_TEXT, AGENT_TOOL_RESULT, AGENT_TOOL_USE, AGENT_WARNING, CAPTURE_REFUSED,
    CONTROL_APPLIED, CONTROL_REQUESTED, DECISION_ANSWERED, DECISION_REQUESTED, MAX_EXCERPT_SCALARS,
    MAX_PAYLOAD_BYTES, MAX_TEXT_SCALARS, RUN_FINISHED, RUN_STARTED, ValidationErrorKind,
    serialise_validation_error, validate,
};
use serde::Serialize;
use serde_json::{Value, json};

fn request() -> Value {
    json!({
        "decisionId": "d", "kind": "permission",
        "dossier": {
            "question": "?", "optionsRuledOut": ["no"],
            "recommendedAction": "ask", "blastRadius": "one run"
        },
        "options": [{ "id": "allow", "label": "Allow" }]
    })
}

#[test]
fn over_bound_reports_capture_and_universal_paths_and_scalar_counts() {
    let over = "😀".repeat(MAX_EXCERPT_SCALARS + 1);
    let mut cases = vec![
        (
            RUN_FINISHED,
            json!({ "outcome": "completed", "durationMs": 1, "reason": over }),
            "payload.reason",
            MAX_EXCERPT_SCALARS,
        ),
        (
            AGENT_TOOL_USE,
            json!({ "tool": "t", "inputExcerpt": over }),
            "payload.inputExcerpt",
            MAX_EXCERPT_SCALARS,
        ),
        (
            AGENT_TOOL_RESULT,
            json!({ "tool": "t", "resultExcerpt": over }),
            "payload.resultExcerpt",
            MAX_EXCERPT_SCALARS,
        ),
        (
            AGENT_WARNING,
            json!({ "message": over }),
            "payload.message",
            MAX_EXCERPT_SCALARS,
        ),
        (
            CONTROL_REQUESTED,
            json!({ "controlId": "c", "kind": "steer", "by": "a", "text": over }),
            "payload.text",
            MAX_EXCERPT_SCALARS,
        ),
        (
            CONTROL_APPLIED,
            json!({ "controlId": "c", "ok": false, "reason": over }),
            "payload.reason",
            MAX_EXCERPT_SCALARS,
        ),
        (
            CAPTURE_REFUSED,
            json!({ "cause": "malformed", "sourceRunId": "r", "detail": over }),
            "payload.detail",
            MAX_EXCERPT_SCALARS,
        ),
        (
            AGENT_TEXT,
            json!({ "text": "😀".repeat(MAX_TEXT_SCALARS + 1) }),
            "payload.text",
            MAX_TEXT_SCALARS,
        ),
        (
            AGENT_TEXT,
            json!({ "text": "ok", "extra": [ { "note": "😀".repeat(MAX_TEXT_SCALARS + 1) } ] }),
            "payload.extra[0].note",
            MAX_TEXT_SCALARS,
        ),
        (
            "future.happened",
            json!("😀".repeat(MAX_TEXT_SCALARS + 1)),
            "payload",
            MAX_TEXT_SCALARS,
        ),
    ];
    for (pointer, path) in [
        ("/dossier/question", "payload.dossier.question"),
        (
            "/dossier/optionsRuledOut/0",
            "payload.dossier.optionsRuledOut[0]",
        ),
        (
            "/dossier/recommendedAction",
            "payload.dossier.recommendedAction",
        ),
        ("/dossier/blastRadius", "payload.dossier.blastRadius"),
        ("/options/0/label", "payload.options[0].label"),
    ] {
        let mut payload = request();
        *payload.pointer_mut(pointer).unwrap() = json!(over);
        cases.push((DECISION_REQUESTED, payload, path, MAX_EXCERPT_SCALARS));
    }
    for (event_type, payload, path, max) in cases {
        assert_eq!(
            validate(event_type, &payload).unwrap_err().kind,
            ValidationErrorKind::OverBound {
                path: path.to_owned(),
                count: max + 1,
                max,
            }
        );
    }
}

#[test]
fn payload_too_large_counts_canonical_utf8_payload_bytes() {
    // Two individually valid strings exceed the byte budget together.
    let payload = json!(["😀".repeat(MAX_TEXT_SCALARS), "😀".repeat(MAX_TEXT_SCALARS)]);
    let error = validate("future.happened", &payload).unwrap_err();
    assert_eq!(
        error.kind,
        ValidationErrorKind::PayloadTooLarge {
            bytes: 131_079,
            max: MAX_PAYLOAD_BYTES
        }
    );
    assert_eq!(
        serialise_validation_error(&error).unwrap(),
        r#"{"bytes":131079,"kind":"PayloadTooLarge","max":131072}"#
    );
}

#[test]
fn unknown_member_reports_each_closed_union_and_retains_the_value() {
    let mut decision = request();
    decision["kind"] = json!("unknown\"😀");
    for (event_type, payload, path) in [
        (
            RUN_STARTED,
            json!({ "kind": "unknown\"😀", "actor": "a", "harness": "h" }),
            "payload.kind",
        ),
        (
            RUN_FINISHED,
            json!({ "outcome": "unknown\"😀", "durationMs": 1 }),
            "payload.outcome",
        ),
        (
            CONTROL_REQUESTED,
            json!({ "controlId": "c", "kind": "unknown\"😀", "by": "a" }),
            "payload.kind",
        ),
        (
            CAPTURE_REFUSED,
            json!({ "cause": "unknown\"😀", "sourceRunId": "r" }),
            "payload.cause",
        ),
        (DECISION_REQUESTED, decision, "payload.kind"),
    ] {
        let error = validate(event_type, &payload).unwrap_err();
        assert_eq!(
            error.kind,
            ValidationErrorKind::UnknownMember {
                path: path.to_owned(),
                value: "unknown\"😀".to_owned()
            }
        );
    }
}

#[test]
fn missing_field_reports_required_fields_including_nested_and_array_paths() {
    for (event_type, payload, fields) in [
        (
            RUN_STARTED,
            json!({ "kind": "loop", "actor": "a", "harness": "h" }),
            vec!["kind", "actor", "harness"],
        ),
        (
            RUN_FINISHED,
            json!({ "outcome": "completed", "durationMs": 1 }),
            vec!["outcome", "durationMs"],
        ),
        (AGENT_TEXT, json!({ "text": "x" }), vec!["text"]),
        (AGENT_TOOL_USE, json!({ "tool": "t" }), vec!["tool"]),
        (AGENT_TOOL_RESULT, json!({ "tool": "t" }), vec!["tool"]),
        (AGENT_WARNING, json!({ "message": "m" }), vec!["message"]),
        (
            CONTROL_REQUESTED,
            json!({ "controlId": "c", "kind": "interrupt", "by": "a" }),
            vec!["controlId", "kind", "by"],
        ),
        (
            CONTROL_APPLIED,
            json!({ "controlId": "c", "ok": true }),
            vec!["controlId", "ok"],
        ),
        (
            CAPTURE_REFUSED,
            json!({ "cause": "gap", "sourceRunId": "r" }),
            vec!["cause", "sourceRunId"],
        ),
        (
            DECISION_REQUESTED,
            request(),
            vec!["decisionId", "kind", "dossier", "options"],
        ),
        (
            DECISION_ANSWERED,
            json!({ "decisionId": "d", "optionId": "allow", "by": "a" }),
            vec!["decisionId", "optionId", "by"],
        ),
    ] {
        for field in fields {
            let mut candidate = payload.clone();
            candidate.as_object_mut().unwrap().remove(field);
            assert_eq!(
                validate(event_type, &candidate).unwrap_err().kind,
                ValidationErrorKind::MissingField {
                    path: format!("payload.{field}")
                }
            );
        }
    }
    for (pointer, prefix, fields) in [
        (
            "/dossier",
            "payload.dossier",
            vec![
                "question",
                "optionsRuledOut",
                "recommendedAction",
                "blastRadius",
            ],
        ),
        ("/options/0", "payload.options[0]", vec!["id", "label"]),
    ] {
        for field in fields {
            let mut payload = request();
            payload
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(field);
            let error = validate(DECISION_REQUESTED, &payload).unwrap_err();
            assert_eq!(
                error.kind,
                ValidationErrorKind::MissingField {
                    path: format!("{prefix}.{field}")
                }
            );
            assert_eq!(error.to_string(), format!("missing field `{field}`"));
        }
    }
}

#[test]
fn policy_reports_steer_and_all_three_timeout_rules() {
    let mut cases = vec![];
    for text in [None, Some("")] {
        let mut payload = json!({ "controlId": "c", "kind": "steer", "by": "a" });
        if let Some(text) = text {
            payload["text"] = json!(text);
        }
        cases.push((CONTROL_REQUESTED, payload, "payload.text", "ControlRequestedPayload.text is required and must not be empty when kind is \"steer\": a steer with nothing to say is a producer error"));
    }
    for (kind, on_timeout, message) in [
        (
            "tripwire",
            "deny",
            "DecisionRequestedPayload.onTimeout is permitted only when kind is \"permission\"; received kind \"tripwire\"",
        ),
        (
            "permission",
            "allow",
            "DecisionRequestedPayload.onTimeout must be \"deny\" when kind is \"permission\"; received \"allow\"",
        ),
        (
            "permission",
            "deny",
            "DecisionRequestedPayload.onTimeout must name one of the request's options[].id; received \"deny\"",
        ),
    ] {
        let mut payload = request();
        payload["kind"] = json!(kind);
        payload["onTimeout"] = json!(on_timeout);
        cases.push((DECISION_REQUESTED, payload, "payload.onTimeout", message));
    }
    for (event_type, payload, path, message) in cases {
        assert_eq!(
            validate(event_type, &payload).unwrap_err().kind,
            ValidationErrorKind::Policy {
                path: path.to_owned(),
                message: message.to_owned()
            }
        );
    }
}

#[test]
fn representation_failures_carry_the_authored_message_and_path() {
    for (event_type, payload, path, message) in [
        (
            AGENT_COMPLETED,
            json!({ "sessionId": 7 }),
            "payload.sessionId",
            "AgentCompletedPayload.sessionId must be a string when present",
        ),
        (
            AGENT_COMPLETED,
            json!({ "usage": { "inputTokens": -1 } }),
            "payload.usage.inputTokens",
            "AgentCompletedPayload.usage.inputTokens must be a non-negative safe integer when present",
        ),
        (
            AGENT_COMPLETED,
            json!({ "extra": [9007199254740992_u64] }),
            "payload.extra[0]",
            "payload.extra[0] is an integral number whose magnitude exceeds the safe integer bound: actual 9007199254740992; maximum 9007199254740991; a value that needs more precision must be carried as a string",
        ),
        (
            AGENT_TEXT,
            json!(false),
            "payload",
            "AgentTextPayload must be an object",
        ),
    ] {
        let error = validate(event_type, &payload).unwrap_err();
        assert_eq!(error.to_string(), message);
        assert_eq!(
            error.kind,
            ValidationErrorKind::Malformed {
                path: path.to_owned(),
                message: message.to_owned()
            }
        );
    }
    // An invalid present value must win over a missing field, just as serde
    // did before this change; a separate missing-field preflight would drift.
    let payload = json!({ "actor": 7 });
    let error = validate(RUN_STARTED, &payload).unwrap_err();
    assert_eq!(
        error.to_string(),
        "RunStartedPayload.actor must be a string"
    );
    assert!(matches!(error.kind, ValidationErrorKind::Malformed { .. }));

    struct Unserialisable;
    impl Serialize for Unserialisable {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("cannot serialise"))
        }
    }
    assert_eq!(
        validate(AGENT_TEXT, &Unserialisable).unwrap_err().kind,
        ValidationErrorKind::Malformed {
            path: "payload".to_owned(),
            message: "cannot serialise".to_owned()
        }
    );
}

#[test]
fn malformed_reports_the_expected_path_and_message() {
    // A wrong-typed value on a known type's field: the value cannot be
    // represented at all, distinct from a stated rule broken by an
    // otherwise representable one.
    let payload =
        json!({ "kind": "loop", "actor": "a", "harness": "h", "note": 9007199254740992_u64 });
    let error = validate(RUN_STARTED, &payload).unwrap_err();
    assert_eq!(
        error.kind,
        ValidationErrorKind::Malformed {
            path: "payload.note".to_owned(),
            message: "payload.note is an integral number whose magnitude exceeds the safe integer bound: actual 9007199254740992; maximum 9007199254740991; a value that needs more precision must be carried as a string".to_owned(),
        }
    );
}

#[test]
fn policy_still_reports_the_stated_rules_after_the_malformed_split() {
    // Pinned in both directions: the previous test asserts the
    // representation failures that moved to `Malformed`; this one asserts
    // that the stated-rule violations that stayed `Policy` still do.
    let steer = json!({ "controlId": "c", "kind": "steer", "by": "a" });
    assert!(matches!(
        validate(CONTROL_REQUESTED, &steer).unwrap_err().kind,
        ValidationErrorKind::Policy { .. }
    ));

    let mut on_timeout = request();
    on_timeout["kind"] = json!("tripwire");
    on_timeout["onTimeout"] = json!("deny");
    assert!(matches!(
        validate(DECISION_REQUESTED, &on_timeout).unwrap_err().kind,
        ValidationErrorKind::Policy { .. }
    ));
}

#[test]
fn conversion_keeps_serde_result_question_mark_callers_working() {
    fn legacy_caller() -> serde_json::Result<()> {
        validate(AGENT_TEXT, &json!({}))?;
        Ok(())
    }
    let original = validate(AGENT_TEXT, &json!({})).unwrap_err();
    let message = original.to_string();
    let converted: serde_json::Error = original.into();
    assert_eq!(converted.to_string(), message);
    assert_eq!(legacy_caller().unwrap_err().to_string(), message);
}

#[test]
fn error_serialisation_sorts_keys_and_omits_compatibility_metadata() {
    let error = validate(
        AGENT_TEXT,
        &json!({ "text": "😀".repeat(MAX_TEXT_SCALARS + 1) }),
    )
    .unwrap_err();
    assert_eq!(
        serialise_validation_error(&error).unwrap(),
        r#"{"count":16385,"kind":"OverBound","max":16384,"path":"payload.text"}"#
    );
    let error = validate(AGENT_TEXT, &json!({})).unwrap_err();
    assert_eq!(
        serialise_validation_error(&error).unwrap(),
        r#"{"kind":"MissingField","path":"payload.text"}"#
    );
}
