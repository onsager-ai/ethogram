use ethogram::{
    CONTROL_APPLIED, CONTROL_REQUESTED, ControlAppliedPayload, ControlAppliedReason, ControlKind,
    ControlRequestedPayload, Event, EventDraft, MAX_EXCERPT_SCALARS, PayloadExtension, StampFields,
    ValidationErrorKind, parse_event, serialise_event, stamp, validate,
};
use serde::Serialize;
use serde_json::{Value, json};

// Written once and pasted identically into control-answer.test.ts. These
// are handwritten agreement examples, not captured corpus fixtures (onsager-ai/ethogram#38).
const CONTROL_ANSWER_REQUESTED_WIRE: &str = r#"{"v":1,"type":"control.requested","runId":"run-control-answer","seq":1,"ts":"2026-09-08T00:00:00.000Z","payload":{"by":"principal:user:alice","controlId":"control-answer-1","decisionId":"decision-1","kind":"answer","optionId":"allow"}}"#;
const CONTROL_ANSWER_APPLIED_WIRE: &str = r#"{"v":1,"type":"control.applied","runId":"run-control-answer","seq":2,"ts":"2026-09-08T00:00:00.000Z","payload":{"controlId":"control-answer-1","ok":true}}"#;
const CONTROL_NO_SUCH_DECISION_WIRE: &str = r#"{"v":1,"type":"control.applied","runId":"run-control-answer","seq":3,"ts":"2026-09-08T00:00:00.000Z","payload":{"controlId":"control-answer-1","ok":false,"reason":"no-such-decision"}}"#;
const CONTROL_ALREADY_ANSWERED_WIRE: &str = r#"{"v":1,"type":"control.applied","runId":"run-control-answer","seq":4,"ts":"2026-09-08T00:00:00.000Z","payload":{"controlId":"control-answer-1","ok":false,"reason":"already-answered"}}"#;
const CONTROL_OPTION_NOT_OFFERED_WIRE: &str = r#"{"v":1,"type":"control.applied","runId":"run-control-answer","seq":5,"ts":"2026-09-08T00:00:00.000Z","payload":{"controlId":"control-answer-1","ok":false,"reason":"option-not-offered"}}"#;

fn event<P>(event_type: &str, payload: P, seq: u64) -> Event<P> {
    stamp(
        EventDraft {
            event_type: event_type.to_owned(),
            payload,
            captured_at: None,
        },
        StampFields {
            run_id: "run-control-answer".to_owned(),
            seq,
            ts: "2026-09-08T00:00:00.000Z".to_owned(),
        },
    )
}

fn answer() -> ControlRequestedPayload {
    ControlRequestedPayload {
        control_id: "control-answer-1".to_owned(),
        kind: ControlKind::Answer,
        decision_id: Some("decision-1".to_owned()),
        option_id: Some("allow".to_owned()),
        text: None,
        truncated: None,
        by: "principal:user:alice".to_owned(),
        extra: PayloadExtension::new(),
    }
}

#[test]
fn answer_events_match_the_typescript_pinned_bytes() {
    let requested = event(CONTROL_REQUESTED, answer(), 1);
    validate(CONTROL_REQUESTED, &requested.payload).unwrap();
    assert_eq!(
        serialise_event(&requested).unwrap(),
        CONTROL_ANSWER_REQUESTED_WIRE
    );
    let parsed: Event<ControlRequestedPayload> =
        serde_json::from_str(CONTROL_ANSWER_REQUESTED_WIRE).unwrap();
    assert_eq!(parsed, requested);
    assert_eq!(
        serialise_event(&parse_event(CONTROL_ANSWER_REQUESTED_WIRE).unwrap()).unwrap(),
        CONTROL_ANSWER_REQUESTED_WIRE
    );

    let mut retained = requested;
    retained.payload.kind = ControlKind::Unknown("answer".to_owned());
    assert_eq!(
        serialise_event(&retained).unwrap(),
        CONTROL_ANSWER_REQUESTED_WIRE
    );

    for (seq, reason, wire) in [
        (2, None, CONTROL_ANSWER_APPLIED_WIRE),
        (
            3,
            Some(ControlAppliedReason::NoSuchDecision),
            CONTROL_NO_SUCH_DECISION_WIRE,
        ),
        (
            4,
            Some(ControlAppliedReason::AlreadyAnswered),
            CONTROL_ALREADY_ANSWERED_WIRE,
        ),
        (
            5,
            Some(ControlAppliedReason::OptionNotOffered),
            CONTROL_OPTION_NOT_OFFERED_WIRE,
        ),
    ] {
        let applied = event(
            CONTROL_APPLIED,
            ControlAppliedPayload {
                control_id: "control-answer-1".to_owned(),
                ok: reason.is_none(),
                by: None,
                reason,
                truncated: None,
                landed_in: None,
                extra: PayloadExtension::new(),
            },
            seq,
        );
        validate(CONTROL_APPLIED, &applied.payload).unwrap();
        assert_eq!(serialise_event(&applied).unwrap(), wire);
        let parsed: Event<ControlAppliedPayload> = serde_json::from_str(wire).unwrap();
        assert_eq!(parsed, applied);
        assert_eq!(serialise_event(&parse_event(wire).unwrap()).unwrap(), wire);
    }
}

// All policy-invalid shapes remain representable, including on the typed path.
fn assert_parseable(event_type: &str, payload: &Value) {
    let wire = serialise_event(&event(event_type, payload, 1)).unwrap();
    assert_eq!(serialise_event(&parse_event(&wire).unwrap()).unwrap(), wire);
    if event_type == CONTROL_REQUESTED {
        serde_json::from_str::<Event<ControlRequestedPayload>>(&wire).unwrap();
    } else {
        serde_json::from_str::<Event<ControlAppliedPayload>>(&wire).unwrap();
    }
}

#[test]
fn answer_ids_are_required_only_at_validation() {
    for field in ["decisionId", "optionId"] {
        let mut payload = serde_json::to_value(answer()).unwrap();
        payload.as_object_mut().unwrap().remove(field);
        assert_parseable(CONTROL_REQUESTED, &payload);
        assert_eq!(
            validate(CONTROL_REQUESTED, &payload).unwrap_err().kind,
            ValidationErrorKind::MissingField {
                path: format!("payload.{field}")
            }
        );
    }
}

#[test]
fn fields_forbidden_by_the_control_kind_are_policy_errors() {
    // Deliberately known kinds only: an unfamiliar kind such as "teleport"
    // reports UnknownMember before this field-forbidden rule is ever
    // reached, exactly like the other three closed unions check membership
    // before any kind-conditioned rule (see `unknown_cannot_spell_any_known_control_kind_even_with_valid_fields`
    // and `unfamiliar_control_kind_retains_exact_bytes_and_validate_reports_it`
    // for the unfamiliar-kind coverage).
    for kind in ["interrupt", "steer"] {
        for field in ["decisionId", "optionId"] {
            let mut payload =
                json!({ "controlId": "c", "kind": kind, "by": "a", "text": "next turn" });
            payload[field] = json!("");
            assert_parseable(CONTROL_REQUESTED, &payload);
            let message = format!(
                "ControlRequestedPayload.{field} is permitted only when kind is \"answer\""
            );
            assert_eq!(
                validate(CONTROL_REQUESTED, &payload).unwrap_err().kind,
                ValidationErrorKind::Policy {
                    path: format!("payload.{field}"),
                    message
                }
            );
        }
    }
    for text in ["", "next turn"] {
        let mut payload = serde_json::to_value(answer()).unwrap();
        payload["text"] = json!(text);
        assert_parseable(CONTROL_REQUESTED, &payload);
        assert_eq!(
            validate(CONTROL_REQUESTED, &payload).unwrap_err().kind,
            ValidationErrorKind::Policy {
                path: "payload.text".to_owned(),
                message: "ControlRequestedPayload.text must be absent when kind is \"answer\""
                    .to_owned(),
            }
        );
    }
}

#[test]
fn unknown_cannot_spell_any_known_control_kind_even_with_valid_fields() {
    for known in [
        ControlKind::Interrupt,
        ControlKind::Steer,
        ControlKind::Answer,
    ] {
        let mut payload = answer();
        payload.kind = known.clone();
        if known != ControlKind::Answer {
            payload.decision_id = None;
            payload.option_id = None;
        }
        if known == ControlKind::Steer {
            payload.text = Some("next turn".to_owned());
        }
        validate(CONTROL_REQUESTED, &payload).unwrap();
        let valid_wire = serialise_event(&event(CONTROL_REQUESTED, &payload, 1)).unwrap();
        payload.kind = ControlKind::Unknown(known.as_str().to_owned());
        assert_eq!(
            validate(CONTROL_REQUESTED, &payload).unwrap_err().kind,
            ValidationErrorKind::Malformed {
                path: "payload.kind".to_owned(),
                message: format!(
                    "ControlRequestedPayload.kind cannot use Unknown for known value: {}",
                    known.as_str()
                ),
            }
        );
        // Serialisation itself is still permissive and byte-identical.
        assert_eq!(
            serialise_event(&event(CONTROL_REQUESTED, &payload, 1)).unwrap(),
            valid_wire
        );
    }
}

#[test]
fn unknown_check_handles_borrowed_struct_map_and_transparent_payloads() {
    #[derive(Serialize)]
    struct Borrowed<'a> {
        control_id: &'a str,
        kind: &'a ControlKind,
        by: &'a str,
    }
    #[derive(Serialize)]
    #[serde(transparent)]
    struct Wrapped<T>(T);
    let kind = ControlKind::Unknown("interrupt".to_owned());
    let payload = Borrowed {
        control_id: "c",
        kind: &kind,
        by: "a",
    };
    assert!(
        matches!(validate(CONTROL_REQUESTED, &Wrapped(payload)).unwrap_err().kind,
        ValidationErrorKind::Malformed { path, .. } if path == "payload.kind")
    );
    let map = std::collections::BTreeMap::from([("kind", kind)]);
    assert!(
        matches!(validate(CONTROL_REQUESTED, &map).unwrap_err().kind,
        ValidationErrorKind::Malformed { path, .. } if path == "payload.kind")
    );
}

#[test]
fn unfamiliar_control_kind_retains_exact_bytes_and_validate_reports_it() {
    let raw = "future/答😀 e\u{301}\n\"";
    let mut payload = answer();
    payload.kind = ControlKind::Unknown(raw.to_owned());
    payload.decision_id = None;
    payload.option_id = None;
    assert_eq!(
        validate(CONTROL_REQUESTED, &payload).unwrap_err().kind,
        ValidationErrorKind::UnknownMember {
            path: "payload.kind".to_owned(),
            value: raw.to_owned(),
        }
    );
    // Reporting the unfamiliar kind at validation does not stop it from
    // parsing and round-tripping byte-for-byte — those are `parse_event`'s
    // concern, not `validate`'s.
    let wire = serialise_event(&event(CONTROL_REQUESTED, payload.clone(), 1)).unwrap();
    let parsed: Event<ControlRequestedPayload> = serde_json::from_str(&wire).unwrap();
    assert_eq!(parsed.payload, payload);
    assert_eq!(parsed.payload.kind.as_str().as_bytes(), raw.as_bytes());
    assert_eq!(serialise_event(&parsed).unwrap(), wire);
    assert_eq!(
        validate(CONTROL_REQUESTED, &parse_event(&wire).unwrap().payload)
            .unwrap_err()
            .kind,
        ValidationErrorKind::UnknownMember {
            path: "payload.kind".to_owned(),
            value: raw.to_owned(),
        }
    );
}

#[test]
fn reason_is_required_for_a_negative_echo_and_optional_for_a_positive_one() {
    let missing = json!({ "controlId": "c", "ok": false });
    assert_parseable(CONTROL_APPLIED, &missing);
    assert_eq!(
        validate(CONTROL_APPLIED, &missing).unwrap_err().kind,
        ValidationErrorKind::MissingField {
            path: "payload.reason".to_owned()
        }
    );
    for payload in [
        json!({ "controlId": "c", "ok": true }),
        json!({ "controlId": "c", "ok": true, "reason": "accepted by runtime" }),
        json!({ "controlId": "c", "ok": true, "reason": "rejected" }),
    ] {
        validate(CONTROL_APPLIED, &payload).unwrap();
    }
}

#[test]
fn all_six_reasons_and_unknown_prose_round_trip() {
    for (raw, reason) in [
        ("no-such-decision", ControlAppliedReason::NoSuchDecision),
        ("already-answered", ControlAppliedReason::AlreadyAnswered),
        ("option-not-offered", ControlAppliedReason::OptionNotOffered),
        ("unsupported", ControlAppliedReason::Unsupported),
        ("not-live", ControlAppliedReason::NotLive),
        ("rejected", ControlAppliedReason::Rejected),
        (
            "future/答😀 e\u{301}\n\"",
            ControlAppliedReason::Unknown("future/答😀 e\u{301}\n\"".to_owned()),
        ),
    ] {
        let payload = json!({ "controlId": "c", "ok": false, "reason": raw });
        validate(CONTROL_APPLIED, &payload).unwrap();
        let wire = serialise_event(&event(CONTROL_APPLIED, payload, 1)).unwrap();
        let parsed: Event<ControlAppliedPayload> = serde_json::from_str(&wire).unwrap();
        assert_eq!(parsed.payload.reason.as_ref(), Some(&reason));
        assert_eq!(reason.as_str().as_bytes(), raw.as_bytes());
        assert_eq!(serialise_event(&parsed).unwrap(), wire);
        assert_eq!(serialise_event(&parse_event(&wire).unwrap()).unwrap(), wire);
    }
}

#[test]
fn unknown_reason_keeps_the_excerpt_bound_for_both_echo_results() {
    for ok in [false, true] {
        for count in [MAX_EXCERPT_SCALARS, MAX_EXCERPT_SCALARS + 1] {
            let payload = json!({ "controlId": "c", "ok": ok, "reason": "😀".repeat(count) });
            assert_parseable(CONTROL_APPLIED, &payload);
            let result = validate(CONTROL_APPLIED, &payload);
            if count == MAX_EXCERPT_SCALARS {
                result.unwrap();
            } else {
                assert_eq!(
                    result.unwrap_err().kind,
                    ValidationErrorKind::OverBound {
                        path: "payload.reason".to_owned(),
                        count,
                        max: MAX_EXCERPT_SCALARS,
                    }
                );
            }
        }
    }
}

#[test]
fn new_optional_strings_reject_null_and_wrong_types_at_parse() {
    for (event_type, base, fields) in [
        (
            CONTROL_REQUESTED,
            serde_json::to_value(answer()).unwrap(),
            vec!["decisionId", "optionId"],
        ),
        (
            CONTROL_APPLIED,
            json!({ "controlId": "c", "ok": true }),
            vec!["reason"],
        ),
    ] {
        for field in fields {
            for invalid in [Value::Null, json!(7), json!({ "Unknown": "answer" })] {
                let mut payload = base.clone();
                payload[field] = invalid;
                let wire = serialise_event(&event(event_type, &payload, 1)).unwrap();
                assert!(parse_event(&wire).is_err());
                assert!(matches!(validate(event_type, &payload).unwrap_err().kind,
                    ValidationErrorKind::Malformed { path, .. } if path == format!("payload.{field}")));
            }
        }
    }
}
