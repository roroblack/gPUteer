//! `state-machines.md` 의 전이표를 **실제로 파싱해** 구현과 대조한다.
//!
//! # 왜 이 파일이 필요한가
//!
//! `docs/protocol/state-machines.md` 는 이렇게 적혀 있었다.
//!
//! > 이 문서의 표는 **테스트가 직접 파싱한다.** `tests/unit/state_machine_test.rs` 가 …
//! > **표를 고치지 않고 전이를 추가하면 CI 가 실패한다. 이것이 이 문서의 목적이다.**
//!
//! `CLAUDE.md` §2 도 "테스트가 이 표를 파싱해 검사한다" 고 적었다.
//!
//! ★ **그 테스트가 없었다.** `tests/unit/` 디렉터리 자체가 존재하지 않았다.
//!   독립 검수(2026-08-16)가 찾았다.
//!
//! `durability.rs` 는 전이를 `matches!` 로 **하드코딩**하고 있었고,
//! 문서 표가 바뀌어도 코드는 그대로였다. 문서와 코드가 **서로 다른 상태기계를
//! 말해도 테스트는 초록색**이었다.
//!
//! 이 세션 내내 고쳐 온 것과 정확히 같은 패턴이다 —
//! **강제 장치가 없는 규범은 규범이 아니라 희망이다.**
//!
//! # 무엇을 검사하는가
//!
//! `state-machines.md` §6 의 테스트 계약 중 이 크레이트 범위(Checkpoint)의 것:
//!
//! ```text
//! 1. 표의 모든 전이가 구현에서 허용된다
//! 2. 구현이 허용하는 전이가 표에 전부 있다  (표에 없는 전이 = 실패)
//! 6. terminal 상태에서 나가는 전이가 없다
//! ```
//!
//! 3(ControlStore durability)·4·5 는 다른 스트림 범위라 여기서 다루지 않는다 —
//! 그 사실을 아래 `unchecked_contract_items` 가 명시한다.

use std::collections::BTreeSet;
use std::path::PathBuf;

use gputeer_checkpoint::DurabilityState;

// ══════════════════════════════════════════════════════════════════
// 파서 — §0 파싱 계약
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Row {
    from: String,
    to: String,
    trigger: String,
    guard: String,
    effect: String,
    durability: String,
}

/// ```` ```statetable ```` 펜스만 읽는다 (§0).
fn parse_state_table(machine: &str) -> Vec<Row> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/protocol/state-machines.md");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} 읽기 실패: {e}", path.display()));

    let mut rows = Vec::new();
    let mut in_fence = false;
    let mut current_machine: Option<String> = None;

    for raw in src.lines() {
        let line = raw.trim();

        if line.starts_with("```statetable") {
            in_fence = true;
            current_machine = None;
            continue;
        }
        if in_fence && line.starts_with("```") {
            in_fence = false;
            continue;
        }
        if !in_fence || line.is_empty() {
            continue;
        }

        if let Some(name) = line.strip_prefix("machine:") {
            current_machine = Some(name.trim().to_string());
            continue;
        }
        // 헤더 행
        if line.starts_with("from |") {
            continue;
        }
        if current_machine.as_deref() != Some(machine) {
            continue;
        }

        let cols: Vec<&str> = line.split('|').map(str::trim).collect();
        assert_eq!(
            cols.len(),
            6,
            "§0 파싱 계약 위반 — 6열이어야 한다: {line:?}"
        );
        rows.push(Row {
            from: cols[0].into(),
            to: cols[1].into(),
            trigger: cols[2].into(),
            guard: cols[3].into(),
            effect: cols[4].into(),
            durability: cols[5].into(),
        });
    }
    rows
}

/// 표의 상태 이름 → 구현의 enum.
///
/// `(none)` 과 `(deleted)` 는 상태가 아니라 **경계**다 —
/// 생성 이전 / 삭제 이후를 뜻하므로 전이 검사 대상이 아니다.
fn to_state(name: &str) -> Option<DurabilityState> {
    use DurabilityState::*;
    Some(match name {
        "WRITING" => Writing,
        "LOCAL_WRITTEN" => LocalWritten,
        "HASH_VERIFIED" => HashVerified,
        "REPLICATING" => Replicating,
        "REPLICATED" => Replicated,
        "COMMITTED" => Committed,
        "COMMITTED_DEGRADED" => CommittedDegraded,
        "PARTIAL" => Partial,
        "(none)" | "(deleted)" => return None,
        other => panic!(
            "표에 알 수 없는 상태 이름: {other:?}\n\
             DurabilityState 에 추가했거나 표에 오타가 있다"
        ),
    })
}

const ALL_STATES: &[DurabilityState] = &[
    DurabilityState::Writing,
    DurabilityState::LocalWritten,
    DurabilityState::HashVerified,
    DurabilityState::Replicating,
    DurabilityState::Replicated,
    DurabilityState::Committed,
    DurabilityState::CommittedDegraded,
    DurabilityState::Partial,
];

// ══════════════════════════════════════════════════════════════════
// 파서 비공허성 — 파서가 빈 결과를 내면 아래가 전부 무의미해진다
// ══════════════════════════════════════════════════════════════════

#[test]
fn parser_is_not_vacuous() {
    let rows = parse_state_table("Checkpoint");
    assert!(
        rows.len() >= 15,
        "Checkpoint 전이표에서 {}행만 뽑았다 — 파서 결함",
        rows.len()
    );

    // 표의 특징적인 행이 실제로 잡히는가
    assert!(
        rows.iter().any(|r| r.from == "COMMITTED"
            && r.to == "COMMITTED_DEGRADED"
            && r.trigger == "REPLICA_LOST"),
        "COMMITTED -> COMMITTED_DEGRADED 행을 못 찾았다"
    );
    assert!(
        rows.iter().any(|r| r.from == "(none)" && r.to == "WRITING"),
        "(none) -> WRITING 행을 못 찾았다"
    );
    assert!(
        rows.iter().any(|r| r.to == "(deleted)"),
        "(deleted) 행을 못 찾았다"
    );

    // 다른 machine 이 섞이지 않았는가
    let node = parse_state_table("Node");
    assert!(!node.is_empty(), "Node 표를 못 뽑았다");
    assert!(
        !rows.iter().any(|r| r.from == "DISCOVERED"),
        "Checkpoint 표에 Node 행이 섞였다"
    );

    // durability 열이 §0 의 세 값 중 하나인가
    for r in &rows {
        assert!(
            matches!(r.durability.as_str(), "COMMITTED" | "DURABLE" | "LOCAL"),
            "durability 열 값이 §0 과 다르다: {r:?}"
        );
    }
}

// ══════════════════════════════════════════════════════════════════
// ★ 계약 1 — 표의 모든 전이가 구현에서 허용되는가
// ══════════════════════════════════════════════════════════════════

#[test]
fn every_documented_transition_is_allowed_by_implementation() {
    let mut missing = Vec::new();
    for r in parse_state_table("Checkpoint") {
        let (Some(from), Some(to)) = (to_state(&r.from), to_state(&r.to)) else {
            continue; // (none) / (deleted) 경계
        };
        if !from.can_transition_to(to) {
            missing.push(format!(
                "{} -> {} ({})",
                r.from, r.to, r.trigger
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "★ 규범 표에 있는데 구현이 거부하는 전이가 있다:\n  {}\n\
         `state-machines.md` §4 를 고쳤다면 `durability.rs` 도 고쳐야 한다.",
        missing.join("\n  ")
    );
}

// ══════════════════════════════════════════════════════════════════
// ★★ 계약 2 — 구현이 허용하는 전이가 표에 전부 있는가
//
// 이쪽이 더 중요하다. **표에 없는 전이를 구현이 허용하면**
// 아무도 검토하지 않은 상태 변화가 일어난다.
// ══════════════════════════════════════════════════════════════════

#[test]
fn every_implemented_transition_is_documented() {
    let documented: BTreeSet<(String, String)> = parse_state_table("Checkpoint")
        .into_iter()
        .filter_map(|r| {
            let (Some(_), Some(_)) = (to_state(&r.from), to_state(&r.to)) else {
                return None;
            };
            Some((r.from, r.to))
        })
        .collect();

    let name = |s: DurabilityState| format!("{s:?}").to_uppercase();
    // enum Debug 이름 -> 표 이름 매핑을 위해 역변환 표를 쓴다
    let table_name = |s: DurabilityState| -> &'static str {
        use DurabilityState::*;
        match s {
            Writing => "WRITING",
            LocalWritten => "LOCAL_WRITTEN",
            HashVerified => "HASH_VERIFIED",
            Replicating => "REPLICATING",
            Replicated => "REPLICATED",
            Committed => "COMMITTED",
            CommittedDegraded => "COMMITTED_DEGRADED",
            Partial => "PARTIAL",
        }
    };
    let _ = name;

    let mut undocumented = Vec::new();
    for &from in ALL_STATES {
        for &to in ALL_STATES {
            if from.can_transition_to(to)
                && !documented.contains(&(
                    table_name(from).to_string(),
                    table_name(to).to_string(),
                ))
            {
                undocumented.push(format!("{} -> {}", table_name(from), table_name(to)));
            }
        }
    }
    assert!(
        undocumented.is_empty(),
        "★★ 구현이 허용하는데 규범 표에 없는 전이가 있다:\n  {}\n\
         **아무도 검토하지 않은 상태 변화다.** \
         `state-machines.md` §4 에 추가하거나 구현에서 제거하라.",
        undocumented.join("\n  ")
    );
}

// ══════════════════════════════════════════════════════════════════
// 계약 6 — terminal 상태에서 나가는 전이가 없는가
// ══════════════════════════════════════════════════════════════════

/// `PARTIAL` 은 `(deleted)` 로만 간다 — 다른 상태로 승격되면 안 된다.
///
/// ★ 이것이 `P0-03` 의 핵심 불변식이다:
/// **"partial file 이 절대 COMMITTED 로 승격되지 않는다."**
#[test]
fn partial_never_gets_promoted() {
    for &to in ALL_STATES {
        assert!(
            !DurabilityState::Partial.can_transition_to(to),
            "★ PARTIAL -> {to:?} 가 허용된다 — P0-03 의 핵심 불변식이 깨졌다"
        );
    }

    // 표에서도 PARTIAL 의 유일한 출구가 (deleted) 인지 확인
    let outs: Vec<_> = parse_state_table("Checkpoint")
        .into_iter()
        .filter(|r| r.from == "PARTIAL")
        .map(|r| r.to)
        .collect();
    assert_eq!(
        outs,
        vec!["(deleted)"],
        "표에서 PARTIAL 의 출구가 (deleted) 뿐이 아니다: {outs:?}"
    );
}

// ══════════════════════════════════════════════════════════════════
// ★ 이 테스트가 검사하지 "않는" 것을 명시한다
// ══════════════════════════════════════════════════════════════════

/// `state-machines.md` §6 의 테스트 계약 6개 중 **3개는 여기서 검사하지 않는다.**
///
/// 조용히 빠뜨리면 "계약이 전부 검사된다" 고 잘못 믿게 된다.
#[test]
fn unchecked_contract_items_are_declared() {
    const UNCHECKED: &[(&str, &str)] = &[
        (
            "§6-3 durability == COMMITTED 전이를 SingleNodeStore 로 시도하면 Unsupported",
            "ControlStore 계층이 미구현이다 (Control 스트림). crates/control-store 없음",
        ),
        (
            "§6-4 각 상태에서 도달 불가능한 상태로의 전이 거부",
            "계약 2(구현->표)가 부분적으로 덮는다. \
             trigger 별 guard 검증은 트리거 계층이 없어 불가",
        ),
        (
            "§6-5 `*` 행을 모든 from 상태에 대해 개별 검증",
            "Checkpoint 표에는 `*` 행이 없다. Node/Job/Attempt/Lease 표에는 있으나 \
             그 상태기계는 미구현이다",
        ),
        (
            "§6-7 공개 풀 COMMITTED 가 §0.1 BROKER_ATTESTED 요건을 만족",
            "Broker·공개 풀 자체가 미구현이다. ParticipationModel 선택자만 있고 배선이 없다 (crates/protocol/src/participation.rs)",
        ),
    ];

    for (item, why) in UNCHECKED {
        println!("미검사 계약: {item}\n  사유: {why}");
    }
    assert_eq!(UNCHECKED.len(), 4, "미검사 항목 수가 바뀌었다 — 목록을 갱신하라");

    // 다른 4개 상태기계는 구현 자체가 없다 — 그 사실을 고정한다
    for machine in ["Node", "Job", "Attempt", "Lease"] {
        let rows = parse_state_table(machine);
        assert!(
            !rows.is_empty(),
            "{machine} 표가 비었다 — 문서가 바뀌었거나 파서가 깨졌다"
        );
        println!("{machine}: 표 {}행, 구현 없음 (미착수)", rows.len());
    }
}
