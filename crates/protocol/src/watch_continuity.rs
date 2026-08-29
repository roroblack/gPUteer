//! Subscriber-side continuity checks for control-plane watch streams.
//!
//! The kernel consumes only an optional previously accepted cursor and the received events.
//! It does not read I/O, clocks, randomness, or global state.

use std::cmp::Ordering;

use crate::pb::{watch_event, Cursor, WatchEvent};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchContinuityReport {
    pub outcome: WatchContinuityOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WatchContinuityOutcome {
    /// The subscriber may retain this cursor and continue from it.
    Continuous { accepted_cursor: Option<Cursor> },
    /// Continuing could silently omit or misorder control events.
    Resynchronize { cause: WatchDiscontinuity },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WatchDiscontinuity {
    Gap {
        previous: Cursor,
        received: Cursor,
    },
    DuplicateOrRegression {
        previous: Cursor,
        received: Cursor,
        kind: CursorRegression,
    },
    /// `control.proto` requires a full resynchronization whenever this is received.
    WatchReset,
    /// The existing schema does not provide enough information to prove continuity.
    Indeterminate {
        previous: Option<Cursor>,
        received: Option<Cursor>,
        reason: IndeterminateReason,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorRegression {
    DuplicateIndex,
    IndexRegression,
    TermRegression,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndeterminateReason {
    MissingEventKind,
    MissingCursor,
    /// No normative rule defines how an index relates to an increased term.
    TermAdvanced,
}

/// Determines whether received watch events can safely extend an accepted cursor.
///
/// Event order is the received stream order and is deliberately not canonicalized. A reset is
/// checked across the entire input first because `control.proto` makes its resynchronization
/// requirement unconditional, regardless of where it appears in a received batch.
pub fn evaluate_watch_continuity(
    previous: Option<&Cursor>,
    events: &[WatchEvent],
) -> WatchContinuityReport {
    if events
        .iter()
        .any(|event| matches!(event.kind.as_ref(), Some(watch_event::Kind::Reset(_))))
    {
        return resynchronize(WatchDiscontinuity::WatchReset);
    }

    let mut accepted = previous.copied();
    for event in events {
        let received = match (event.kind.as_ref(), event.cursor.as_ref()) {
            (None, cursor) => {
                return resynchronize(WatchDiscontinuity::Indeterminate {
                    previous: accepted,
                    received: cursor.copied(),
                    reason: IndeterminateReason::MissingEventKind,
                });
            }
            (Some(watch_event::Kind::Event(_)), None) => {
                return resynchronize(WatchDiscontinuity::Indeterminate {
                    previous: accepted,
                    received: None,
                    reason: IndeterminateReason::MissingCursor,
                });
            }
            (Some(watch_event::Kind::Event(_)), Some(cursor)) => *cursor,
            (Some(watch_event::Kind::Reset(_)), _) => {
                return resynchronize(WatchDiscontinuity::WatchReset);
            }
        };

        if let Some(previous) = accepted {
            if let Some(cause) = compare_cursors(previous, received) {
                return resynchronize(cause);
            }
        }
        accepted = Some(received);
    }

    WatchContinuityReport {
        outcome: WatchContinuityOutcome::Continuous {
            accepted_cursor: accepted,
        },
    }
}

fn compare_cursors(previous: Cursor, received: Cursor) -> Option<WatchDiscontinuity> {
    match received.term.cmp(&previous.term) {
        Ordering::Less => {
            return Some(WatchDiscontinuity::DuplicateOrRegression {
                previous,
                received,
                kind: CursorRegression::TermRegression,
            });
        }
        Ordering::Greater => {
            return Some(WatchDiscontinuity::Indeterminate {
                previous: Some(previous),
                received: Some(received),
                reason: IndeterminateReason::TermAdvanced,
            });
        }
        Ordering::Equal => {}
    }

    match received.index.cmp(&previous.index) {
        Ordering::Less => Some(WatchDiscontinuity::DuplicateOrRegression {
            previous,
            received,
            kind: CursorRegression::IndexRegression,
        }),
        Ordering::Equal => Some(WatchDiscontinuity::DuplicateOrRegression {
            previous,
            received,
            kind: CursorRegression::DuplicateIndex,
        }),
        Ordering::Greater if previous.index.checked_add(1) == Some(received.index) => None,
        Ordering::Greater => Some(WatchDiscontinuity::Gap { previous, received }),
    }
}

fn resynchronize(cause: WatchDiscontinuity) -> WatchContinuityReport {
    WatchContinuityReport {
        outcome: WatchContinuityOutcome::Resynchronize { cause },
    }
}
