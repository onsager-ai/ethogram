use ethogram::{
    CAPTURE_REFUSED, CONTROL_REQUESTED, CaptureRefusalCause, CaptureRefusedPayload, ControlKind,
    ControlRequestedPayload, RUN_FINISHED, RUN_STARTED, RunFinishedPayload, RunKind, RunOutcome,
    RunStartedPayload, ValidationErrorKind, validate,
};
use serde::Serialize;
use serde_json::json;

fn started(kind: RunKind) -> RunStartedPayload {
    RunStartedPayload {
        kind,
        ..serde_json::from_value(json!({ "kind": "relay", "actor": "a", "harness": "h" })).unwrap()
    }
}

fn finished(outcome: RunOutcome) -> RunFinishedPayload {
    RunFinishedPayload {
        outcome,
        ..serde_json::from_value(json!({ "outcome": "completed", "durationMs": 1 })).unwrap()
    }
}

fn requested(kind: ControlKind) -> ControlRequestedPayload {
    let mut payload: ControlRequestedPayload = serde_json::from_value(json!({
        "kind": "interrupt", "controlId": "c", "by": "a"
    }))
    .unwrap();
    // Supply valid fields for the wire spelling, so only the typed Unknown
    // distinction can fail the test, never a kind-conditioned policy rule.
    if kind.as_str() == "answer" {
        payload.decision_id = Some("d".to_owned());
        payload.option_id = Some("allow".to_owned());
    } else if kind.as_str() == "steer" {
        payload.text = Some("next turn".to_owned());
    }
    payload.kind = kind;
    payload
}

fn refused(cause: CaptureRefusalCause) -> CaptureRefusedPayload {
    CaptureRefusedPayload {
        cause,
        ..serde_json::from_value(json!({ "cause": "gap", "sourceRunId": "r" })).unwrap()
    }
}

#[test]
fn run_kind_unknown_cannot_spell_known_member() {
    for known in [
        RunKind::Relay,
        RunKind::Loop,
        RunKind::Handoff,
        RunKind::Subagent,
        RunKind::Session,
        RunKind::Judgment,
    ] {
        let raw = known.as_str();
        let payload = started(RunKind::Unknown(raw.to_owned()));
        assert_eq!(
            validate(RUN_STARTED, &payload)
                .expect_err("RunKind::Unknown spelling a known member must be Malformed")
                .kind,
            ValidationErrorKind::Malformed {
                path: "payload.kind".to_owned(),
                message: format!(
                    "RunStartedPayload.kind cannot use Unknown for known value: {raw}"
                ),
            }
        );
        // Value conversion erases the variant: these two payloads are now
        // identical, and the wire value receives the known member's rules.
        let wire = serde_json::to_value(&payload).unwrap();
        assert_eq!(wire, serde_json::to_value(started(known.clone())).unwrap());
        validate(RUN_STARTED, &wire).unwrap();
        validate(RUN_STARTED, &started(known.clone())).unwrap();
        let parsed: RunStartedPayload = serde_json::from_value(wire).unwrap();
        assert_eq!(parsed.kind, known);
    }
}

#[test]
fn run_outcome_unknown_cannot_spell_known_member() {
    for known in [
        RunOutcome::Capped,
        RunOutcome::Completed,
        RunOutcome::Failed,
        RunOutcome::NoOp,
        RunOutcome::TimedOut,
        RunOutcome::Interrupted,
        RunOutcome::PermissionDenied,
        RunOutcome::Canceled,
        RunOutcome::Blocked,
        RunOutcome::Unstarted,
    ] {
        let raw = known.as_str();
        let payload = finished(RunOutcome::Unknown(raw.to_owned()));
        assert_eq!(
            validate(RUN_FINISHED, &payload)
                .expect_err("RunOutcome::Unknown spelling a known member must be Malformed")
                .kind,
            ValidationErrorKind::Malformed {
                path: "payload.outcome".to_owned(),
                message: format!(
                    "RunFinishedPayload.outcome cannot use Unknown for known value: {raw}"
                ),
            }
        );
        let wire = serde_json::to_value(&payload).unwrap();
        assert_eq!(wire, serde_json::to_value(finished(known.clone())).unwrap());
        validate(RUN_FINISHED, &wire).unwrap();
        validate(RUN_FINISHED, &finished(known.clone())).unwrap();
        let parsed: RunFinishedPayload = serde_json::from_value(wire).unwrap();
        assert_eq!(parsed.outcome, known);
    }
}

#[test]
fn control_kind_unknown_cannot_spell_known_member() {
    for known in [
        ControlKind::Answer,
        ControlKind::Interrupt,
        ControlKind::Steer,
    ] {
        let raw = known.as_str();
        let payload = requested(ControlKind::Unknown(raw.to_owned()));
        assert_eq!(
            validate(CONTROL_REQUESTED, &payload)
                .expect_err("ControlKind::Unknown spelling a known member must be Malformed")
                .kind,
            ValidationErrorKind::Malformed {
                path: "payload.kind".to_owned(),
                message: format!(
                    "ControlRequestedPayload.kind cannot use Unknown for known value: {raw}"
                ),
            }
        );
        let wire = serde_json::to_value(&payload).unwrap();
        assert_eq!(
            wire,
            serde_json::to_value(requested(known.clone())).unwrap()
        );
        validate(CONTROL_REQUESTED, &wire).unwrap();
        validate(CONTROL_REQUESTED, &requested(known.clone())).unwrap();
        let parsed: ControlRequestedPayload = serde_json::from_value(wire).unwrap();
        assert_eq!(parsed.kind, known);
    }
}

#[test]
fn capture_refusal_cause_unknown_cannot_spell_known_member() {
    for known in [
        CaptureRefusalCause::Gap,
        CaptureRefusalCause::OverBound,
        CaptureRefusalCause::Duplicate,
        CaptureRefusalCause::Finished,
        CaptureRefusalCause::Malformed,
    ] {
        let raw = known.as_str();
        let payload = refused(CaptureRefusalCause::Unknown(raw.to_owned()));
        assert_eq!(
            validate(CAPTURE_REFUSED, &payload)
                .expect_err(
                    "CaptureRefusalCause::Unknown spelling a known member must be Malformed"
                )
                .kind,
            ValidationErrorKind::Malformed {
                path: "payload.cause".to_owned(),
                message: format!(
                    "CaptureRefusedPayload.cause cannot use Unknown for known value: {raw}"
                ),
            }
        );
        let wire = serde_json::to_value(&payload).unwrap();
        assert_eq!(wire, serde_json::to_value(refused(known.clone())).unwrap());
        validate(CAPTURE_REFUSED, &wire).unwrap();
        validate(CAPTURE_REFUSED, &refused(known.clone())).unwrap();
        let parsed: CaptureRefusedPayload = serde_json::from_value(wire).unwrap();
        assert_eq!(parsed.cause, known);
    }
}

const UNFAMILIAR: &str = "future/答😀 e\u{301}\n\"";

#[test]
fn run_kind_unfamiliar_string_remains_unknown_member() {
    let payload = started(RunKind::Unknown(UNFAMILIAR.to_owned()));
    assert_eq!(
        validate(RUN_STARTED, &payload).unwrap_err().kind,
        ValidationErrorKind::UnknownMember {
            path: "payload.kind".to_owned(),
            value: UNFAMILIAR.to_owned(),
        }
    );
    let wire = serde_json::to_value(&payload).unwrap();
    assert_eq!(
        validate(RUN_STARTED, &wire),
        validate(RUN_STARTED, &payload)
    );
    let parsed: RunStartedPayload = serde_json::from_value(wire).unwrap();
    assert_eq!(parsed, payload);
}

#[test]
fn run_outcome_unfamiliar_string_remains_unknown_member() {
    let payload = finished(RunOutcome::Unknown(UNFAMILIAR.to_owned()));
    assert_eq!(
        validate(RUN_FINISHED, &payload).unwrap_err().kind,
        ValidationErrorKind::UnknownMember {
            path: "payload.outcome".to_owned(),
            value: UNFAMILIAR.to_owned(),
        }
    );
    let wire = serde_json::to_value(&payload).unwrap();
    assert_eq!(
        validate(RUN_FINISHED, &wire),
        validate(RUN_FINISHED, &payload)
    );
    let parsed: RunFinishedPayload = serde_json::from_value(wire).unwrap();
    assert_eq!(parsed, payload);
}

#[test]
fn control_kind_unfamiliar_string_remains_unknown_member() {
    let payload = requested(ControlKind::Unknown(UNFAMILIAR.to_owned()));
    assert_eq!(
        validate(CONTROL_REQUESTED, &payload).unwrap_err().kind,
        ValidationErrorKind::UnknownMember {
            path: "payload.kind".to_owned(),
            value: UNFAMILIAR.to_owned(),
        }
    );
    let wire = serde_json::to_value(&payload).unwrap();
    assert_eq!(
        validate(CONTROL_REQUESTED, &wire),
        validate(CONTROL_REQUESTED, &payload)
    );
    let parsed: ControlRequestedPayload = serde_json::from_value(wire).unwrap();
    assert_eq!(parsed, payload);
}

#[test]
fn capture_refusal_cause_unfamiliar_string_remains_unknown_member() {
    let payload = refused(CaptureRefusalCause::Unknown(UNFAMILIAR.to_owned()));
    assert_eq!(
        validate(CAPTURE_REFUSED, &payload).unwrap_err().kind,
        ValidationErrorKind::UnknownMember {
            path: "payload.cause".to_owned(),
            value: UNFAMILIAR.to_owned(),
        }
    );
    let wire = serde_json::to_value(&payload).unwrap();
    assert_eq!(
        validate(CAPTURE_REFUSED, &wire),
        validate(CAPTURE_REFUSED, &payload)
    );
    let parsed: CaptureRefusedPayload = serde_json::from_value(wire).unwrap();
    assert_eq!(parsed, payload);
}

#[test]
fn probe_uses_the_registered_event_field_and_union_only() {
    #[derive(Serialize)]
    struct Extensions {
        kind: RunKind,
        actor: &'static str,
        harness: &'static str,
        outcome: RunOutcome,
        nested: RunStartedPayload,
    }
    let payload = Extensions {
        kind: RunKind::Relay,
        actor: "a",
        harness: "h",
        outcome: RunOutcome::Unknown("completed".to_owned()),
        nested: started(RunKind::Unknown("relay".to_owned())),
    };
    validate(RUN_STARTED, &payload).unwrap();
    validate(
        "future.happened",
        &started(RunKind::Unknown("relay".to_owned())),
    )
    .unwrap();
    // Another union in a borrowed producer's field is governed by its wire
    // representation, not mistaken for the registered union's typed Unknown.
    let other_union = std::collections::BTreeMap::from([
        ("kind", ControlKind::Unknown("relay".to_owned())),
        ("actor", ControlKind::Unknown("a".to_owned())),
        ("harness", ControlKind::Unknown("h".to_owned())),
    ]);
    validate(RUN_STARTED, &other_union).unwrap();
}
