use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use ethogram::parse_event;

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
fn every_run_id_belongs_to_exactly_one_capture() -> Result<(), Box<dyn Error>> {
    struct RunIdGroup {
        run_id: &'static str,
        fixtures: &'static [(&'static str, u64)],
        reason: &'static str,
    }

    // A `runId` belongs to exactly one capture. Fixtures drawn from the same
    // capture share it — `conformance/README.md` says so, and a run's events
    // would otherwise have to be given different ids, which would falsify the
    // capture and break `foldRun` for anyone who did the obvious thing. What
    // must never happen is a *new* capture reusing a `runId` already here.
    //
    // This list pins every `runId` carrying more than one fixture, with the
    // exact (fixture, seq) set it carries, and the check below asserts disk
    // equals it in both directions. That yields, without a second list:
    //
    //   - a new fixture joining any listed `runId` fails, colliding on `seq`
    //     or not — which is the hole this replaces (issue onsager-ai/ethogram#62);
    //   - a new fixture reusing a single-fixture `runId` such as `gate` fails
    //     too, because that `runId` becomes an unlisted group;
    //   - a withdrawal that shrinks or empties a group fails until the entry
    //     is updated, so an entry cannot go stale unnoticed;
    //   - a new multi-fixture capture adds one entry, as a deliberate edit
    //     someone reviews rather than a silent arrival.
    //
    // It subsumes the `(runId, seq)` collision test this replaces: any new
    // collision requires sharing a `runId`, which fails here first.
    //
    // Rust only: this inventories the corpus files, not either SDK's
    // behaviour. Principle 1 needs one definition here, and a TypeScript copy
    // would be a second list free to drift from this one.
    const GROUPS: &[RunIdGroup] = &[
        RunIdGroup {
            run_id: "builder-20260909T091631822Z-1098540-0",
            fixtures: &[
                ("control-applied-answer.json", 10),
                ("control-requested-answer.json", 7),
                ("decision-answered-permission.json", 9),
                ("decision-requested-permission.json", 6),
            ],
            reason: "one capture of the first real permission exchange, four events selected from its stream: an ungranted call raised as a decision, the supervisor's answer, the answer recorded, and the control echoed",
        },
        RunIdGroup {
            run_id: "judgment-20300102T030405000Z-fixture-0",
            fixtures: &[
                ("decision-answered-excuse-requested-run.json", 2),
                ("decision-answered-excuse.json", 2),
            ],
            reason: "the backward-compatibility pair documented in conformance/README.md: one shape before and after decision.answered gained requestedRunId. Two separate captures that share a seq as well as a runId, because a deterministic clock synthesised the same id twice",
        },
        RunIdGroup {
            run_id: "run-claude-control-interrupt",
            fixtures: &[
                ("control-applied-interrupt.json", 5),
                ("control-applied-not-live.json", 6),
                ("control-requested-interrupt.json", 4),
                ("control-requested-steer.json", 3),
            ],
            reason: "one capture of a control exchange, four events selected from its stream",
        },
        RunIdGroup {
            run_id: "run-claude-error-shapes",
            fixtures: &[
                ("agent-completed-max-turns.json", 5),
                ("agent-tool-result-error.json", 4),
                ("agent-warning.json", 6),
            ],
            reason: "one capture, three events selected from its stream",
        },
        RunIdGroup {
            run_id: "run-claude-subagent",
            fixtures: &[
                ("agent-completed-repeated-terminal.json", 14),
                ("agent-completed.json", 13),
                ("agent-started.json", 1),
                ("agent-text.json", 4),
                ("agent-tool-result-subagent.json", 9),
                ("agent-tool-result.json", 6),
                ("agent-tool-use-subagent.json", 8),
                ("agent-tool-use.json", 5),
            ],
            reason: "one subagent capture, eight events selected from its stream",
        },
        RunIdGroup {
            run_id: "sweep",
            fixtures: &[
                ("decision-requested-human-decides-options.json", 2),
                ("decision-requested-human-decides.json", 3),
                ("decision-requested-tripwire.json", 2),
                ("decision-requested-unclassified.json", 3),
                ("decision-requested-unexplained-write.json", 4),
            ],
            reason: "separate captures whose synthesised run id was the literal \"sweep\", taken before this rule existed. Two pairs of them share a seq as well; this is the group the rule exists to stop recurring",
        },
    ];

    let mut on_disk = BTreeMap::<String, BTreeSet<(String, u64)>>::new();
    for (name, raw_json) in disk_fixtures()? {
        let event = parse_event(&raw_json)?;
        on_disk
            .entry(event.run_id)
            .or_default()
            .insert((name, event.seq));
    }
    on_disk.retain(|_, fixtures| fixtures.len() > 1);

    let mut problems = Vec::new();
    for group in GROUPS {
        let expected = group
            .fixtures
            .iter()
            .map(|(name, seq)| ((*name).to_owned(), *seq))
            .collect::<BTreeSet<_>>();
        match on_disk.remove(group.run_id) {
            None => problems.push(format!(
                "stale runId group: {:?} no longer carries more than one fixture; remove or shrink the entry, which listed {:?} (reason: {})",
                group.run_id, expected, group.reason,
            )),
            Some(actual) if actual != expected => problems.push(format!(
                "runId group changed: {:?} expected {:?}, found {:?}; a new capture must use a runId not already in the corpus, and a withdrawn fixture must leave this entry (reason: {})",
                group.run_id, expected, actual, group.reason,
            )),
            Some(_) => {}
        }
    }
    for (run_id, fixtures) in on_disk {
        problems.push(format!(
            "unlisted runId group: {run_id:?} carries {fixtures:?}; a runId belongs to exactly one capture, so either these fixtures are one capture and the group needs an entry with a reason, or a new capture has reused a runId already in the corpus",
        ));
    }
    assert!(
        problems.is_empty(),
        "corpus runId inventory does not match the historical groups:\n{}",
        problems.join("\n"),
    );

    Ok(())
}

#[test]
fn the_permanent_agreement_inputs_are_still_present() -> Result<(), Box<dyn Error>> {
    // `handwritten-agreement-inputs/README.md` says an input there must be
    // superseded by a real captured fixture once a producer emits its shape.
    // That is true of the five answer-verb shapes and false of these three:
    // no producer emits them, because each exists to hold a property the
    // corpus cannot hold. Retiring all eight on the supersession rule would
    // silently remove the only band pin in the byte diff, the only cross-SDK
    // check of Rust's typed retention, and the only input on the harness's
    // untyped-only branch — with every remaining test still green.
    const PERMANENT: &[(&str, &str)] = &[
        (
            "run-finished-band-cost.json",
            "the only input inside the [1e-6, 1e-5) notation band, which no capture has produced and no fixture can carry (onsager-ai/ethogram#53)",
        ),
        (
            "run-started-unknown-fields.json",
            "the only cross-SDK check of Rust's typed payload retention, and of the four number shapes through the flatten layer (onsager-ai/ethogram#57)",
        ),
        (
            "unrecognised-type.json",
            "the only input on the untyped-only branch; no producer emits a type invented to be unrecognised (onsager-ai/ethogram#60)",
        ),
    ];

    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/handwritten-agreement-inputs");
    let missing = PERMANENT
        .iter()
        .filter(|(name, _)| !directory.join(name).is_file())
        .map(|(name, reason)| format!("{name} — {reason}"))
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "permanent agreement inputs are missing; these are not superseded by any capture:\n{}",
        missing.join("\n"),
    );

    Ok(())
}
