use gputeer_protocol::pb::{watch_event, ControlEvent, Cursor, WatchEvent, WatchReset};
use gputeer_protocol::{
    evaluate_watch_continuity, CursorRegression, IndeterminateReason, WatchContinuityOutcome,
    WatchContinuityReport, WatchDiscontinuity,
};

fn cursor(index: u64, term: u64) -> Cursor {
    Cursor { index, term }
}

fn control_event(index: u64, term: u64) -> WatchEvent {
    WatchEvent {
        cursor: Some(cursor(index, term)),
        kind: Some(watch_event::Kind::Event(ControlEvent::default())),
    }
}

fn reset_event() -> WatchEvent {
    WatchEvent {
        cursor: None,
        kind: Some(watch_event::Kind::Reset(WatchReset::default())),
    }
}

fn resynchronize(cause: WatchDiscontinuity) -> WatchContinuityReport {
    WatchContinuityReport {
        outcome: WatchContinuityOutcome::Resynchronize { cause },
    }
}

fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    fn visit<T: Clone>(remaining: Vec<T>, prefix: Vec<T>, output: &mut Vec<Vec<T>>) {
        if remaining.is_empty() {
            output.push(prefix);
            return;
        }
        for index in 0..remaining.len() {
            let mut next_remaining = remaining.clone();
            let item = next_remaining.remove(index);
            let mut next_prefix = prefix.clone();
            next_prefix.push(item);
            visit(next_remaining, next_prefix, output);
        }
    }

    let mut output = Vec::new();
    visit(items.to_vec(), Vec::new(), &mut output);
    output
}

#[test]
fn consecutive_events_are_safe_to_continue() {
    let previous = cursor(40, 7);
    let events = [control_event(41, 7), control_event(42, 7), control_event(43, 7)];

    assert_eq!(
        evaluate_watch_continuity(Some(&previous), &events),
        WatchContinuityReport {
            outcome: WatchContinuityOutcome::Continuous {
                accepted_cursor: Some(cursor(43, 7)),
            },
        }
    );
}

#[test]
fn absent_previous_cursor_uses_first_event_as_subscription_anchor() {
    assert_eq!(
        evaluate_watch_continuity(None, &[control_event(900, 12), control_event(901, 12)]),
        WatchContinuityReport {
            outcome: WatchContinuityOutcome::Continuous {
                accepted_cursor: Some(cursor(901, 12)),
            },
        }
    );
    assert_eq!(
        evaluate_watch_continuity(None, &[]),
        WatchContinuityReport {
            outcome: WatchContinuityOutcome::Continuous { accepted_cursor: None },
        }
    );
}

#[test]
fn gap_requires_resynchronization() {
    let previous = cursor(40, 7);

    assert_eq!(
        evaluate_watch_continuity(Some(&previous), &[control_event(42, 7)]),
        resynchronize(WatchDiscontinuity::Gap {
            previous,
            received: cursor(42, 7),
        })
    );
}

#[test]
fn duplicate_and_index_or_term_regression_require_resynchronization() {
    let previous = cursor(40, 7);
    let cases = [
        (
            control_event(40, 7),
            CursorRegression::DuplicateIndex,
            cursor(40, 7),
        ),
        (
            control_event(39, 7),
            CursorRegression::IndexRegression,
            cursor(39, 7),
        ),
        (
            control_event(41, 6),
            CursorRegression::TermRegression,
            cursor(41, 6),
        ),
    ];

    for (event, kind, received) in cases {
        assert_eq!(
            evaluate_watch_continuity(Some(&previous), &[event]),
            resynchronize(WatchDiscontinuity::DuplicateOrRegression {
                previous,
                received,
                kind,
            })
        );
    }
}

#[test]
fn advanced_term_is_indeterminate_without_a_normative_index_relation() {
    let previous = cursor(40, 7);
    let received = cursor(41, 8);

    assert_eq!(
        evaluate_watch_continuity(Some(&previous), &[control_event(41, 8)]),
        resynchronize(WatchDiscontinuity::Indeterminate {
            previous: Some(previous),
            received: Some(received),
            reason: IndeterminateReason::TermAdvanced,
        })
    );
}

#[test]
fn reset_dominates_every_four_event_permutation_with_the_identical_report() {
    let previous = cursor(40, 7);
    let events = [
        control_event(41, 7),
        control_event(42, 7),
        reset_event(),
        control_event(43, 7),
    ];
    let expected = resynchronize(WatchDiscontinuity::WatchReset);

    for permutation in permutations(&events) {
        assert_eq!(evaluate_watch_continuity(Some(&previous), &permutation), expected);
    }
}

#[test]
fn missing_kind_or_cursor_is_indeterminate_and_fails_closed() {
    let previous = cursor(40, 7);
    let missing_kind = WatchEvent {
        cursor: Some(cursor(41, 7)),
        kind: None,
    };
    assert_eq!(
        evaluate_watch_continuity(Some(&previous), &[missing_kind]),
        resynchronize(WatchDiscontinuity::Indeterminate {
            previous: Some(previous),
            received: Some(cursor(41, 7)),
            reason: IndeterminateReason::MissingEventKind,
        })
    );

    let missing_cursor = WatchEvent {
        cursor: None,
        kind: Some(watch_event::Kind::Event(ControlEvent::default())),
    };
    assert_eq!(
        evaluate_watch_continuity(Some(&previous), &[missing_cursor]),
        resynchronize(WatchDiscontinuity::Indeterminate {
            previous: Some(previous),
            received: None,
            reason: IndeterminateReason::MissingCursor,
        })
    );
}

#[test]
fn non_reset_event_order_is_stream_semantics_and_is_not_canonicalized() {
    let previous = cursor(9, 3);
    let events = [
        control_event(10, 3),
        control_event(11, 3),
        control_event(12, 3),
        control_event(13, 3),
    ];
    let expected = WatchContinuityReport {
        outcome: WatchContinuityOutcome::Continuous {
            accepted_cursor: Some(cursor(13, 3)),
        },
    };

    for permutation in permutations(&events) {
        let actual = evaluate_watch_continuity(Some(&previous), &permutation);
        if permutation == events {
            assert_eq!(actual, expected);
        } else {
            assert!(matches!(
                actual.outcome,
                WatchContinuityOutcome::Resynchronize { .. }
            ));
        }
    }
}
