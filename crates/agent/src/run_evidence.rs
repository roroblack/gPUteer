//! 해제 전 증거(실행 알림 계약 v18k §4 · v18l) · 실행 알림 만들기 — 계획 조각 5d.
//!
//! ```text
//! Agent 기동의 자동 증거(지우지 않는 변형 · v18f)
//!   조건   원장 행에 확인한 컨테이너 ID · 연결 대상 · 런타임 대상 신원 · 이름이 있다(없으면 증거 없음 — "ID 없는 행" · 고정 못 한 런타임)
//!   절차   ① 고정한 대상으로 신원을 읽어 원장 값과 대조 ② 새 조회 셋(ID · owner 라벨 안의 그 이름 · 이름)이 **모두** not-found
//!          ③ 다시 신원을 읽어 대조. 무응답 · 시한 · 오류 · 하나라도 "있음" · 신원 다름 → 증거 없음
//!   지우지 않는다 — 로그 보존 · rm 은 해제 명령만 한다(5d2)
//! 알림   노드 키로 서명한 `AttemptRunNotice` — 바이트(서명 포함 protobuf)와 notice_hash(= BLAKE3(sig_input) · ACK 가 echo)를 원장에 넘긴다
//! ```
//! ★ 부재는 "지금 멈췄다" 의 증거지 "안 돌았다" 의 증거가 아니다(결정 D — 받아들인 한계).

use gputeer_crypto::{sign, SigningKey};
use gputeer_protocol::{canonical::blake3_256, pb, signing::signing_input};
use prost::Message;

use crate::container::runtime_target::{self, Lookup, Presence, RuntimeEndpoint};
use crate::run_ledger::{AttemptRow, NewNotice, NoticeKind};

/// 런타임에 묻는 길 — 운영은 런타임 CLI([`CliQueries`]), 시험은 흉내를 넣는다.
pub trait RuntimeQueries {
    fn identity(&self, endpoint: &RuntimeEndpoint) -> Result<String, String>;
    fn lookup(&self, endpoint: &RuntimeEndpoint, lookup: &Lookup<'_>) -> Result<Presence, String>;
}

/// 운영 — 원장 행에 적힌 런타임 실행 파일로 묻는다.
pub struct CliQueries<'a> {
    pub program: &'a std::path::Path,
}

impl RuntimeQueries for CliQueries<'_> {
    fn identity(&self, endpoint: &RuntimeEndpoint) -> Result<String, String> {
        runtime_target::read_identity(self.program, endpoint)
    }
    fn lookup(&self, endpoint: &RuntimeEndpoint, lookup: &Lookup<'_>) -> Result<Presence, String> {
        runtime_target::pinned_lookup(self.program, endpoint, lookup)
    }
}

/// 자동 증거의 판정.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoEvidence {
    /// 확인한 ID 의 컨테이너가 고정한 대상에서 **최종적으로 없다**(조회 셋 모두 not-found · 신원 앞뒤 같음).
    Absent,
    /// 증거를 세우지 못했다 — 사유. `id_recorded` 는 원장에 확인한 컨테이너 ID 가 있었는가(없으면 늦은 생성을 배제할 수 없다).
    NotEstablished { why: String, id_recorded: bool },
}

/// §4 — Agent 기동의 자동 증거(지우지 않는다). `owner` 는 이 Agent 의 owner 라벨(루트를 잠그고 계산한 값).
pub fn startup_absence_evidence(
    row: &AttemptRow,
    owner: &str,
    queries: &dyn RuntimeQueries,
) -> AutoEvidence {
    let not = |why: &str| AutoEvidence::NotEstablished {
        why: why.to_string(),
        id_recorded: row.container_id.is_some(),
    };
    let Some(id) = row.container_id.as_deref() else {
        return not("원장에 확인한 컨테이너 ID 가 없다(create 결과를 적기 전 · 옛 행) — 늦은 생성을 배제할 수 없다");
    };
    let (Some(target), Some(recorded_identity)) = (
        row.connection_target.as_deref(),
        row.runtime_target_identity.as_deref(),
    ) else {
        return not("원장에 런타임 대상 · 신원이 없다(고정하지 못한 런타임) — 다른 런타임의 \"없음\" 일 수 있다");
    };
    let Some(name) = row.container_name.as_deref() else {
        return not("원장에 컨테이너 이름이 없다");
    };
    if owner.trim().is_empty() {
        return not("이 Agent 의 owner 라벨이 비었다 — 빈 라벨로 \"없음\" 을 얻지 않는다");
    }
    let endpoint = match RuntimeEndpoint::from_ledger(target) {
        Ok(endpoint) => endpoint,
        Err(why) => return not(&format!("원장의 연결 대상을 읽지 못했다: {why}")),
    };
    match queries.identity(&endpoint) {
        Ok(identity) if identity == recorded_identity => {}
        Ok(identity) => {
            return not(&format!(
                "앞 신원이 원장과 다르다(원장 {recorded_identity:?} · 지금 {identity:?}) — 다른 런타임에 묻고 있다"
            ))
        }
        Err(why) => return not(&format!("앞 신원을 읽지 못했다: {why}")),
    }
    for lookup in [
        Lookup::Id(id),
        Lookup::OwnerLabelWithName { owner, name },
        Lookup::Name(name),
    ] {
        match queries.lookup(&endpoint, &lookup) {
            Ok(Presence::Absent) => {}
            Ok(Presence::Present) => {
                return not(&format!("조회 {lookup:?} 에 나온다 — 이미 없음이 아니다"))
            }
            Err(why) => return not(&format!("조회 {lookup:?} 가 답하지 않았다: {why}")),
        }
    }
    match queries.identity(&endpoint) {
        Ok(identity) if identity == recorded_identity => AutoEvidence::Absent,
        Ok(identity) => not(&format!(
            "뒤 신원이 원장과 다르다(원장 {recorded_identity:?} · 지금 {identity:?}) — 조회 도중 대상이 바뀌었다"
        )),
        Err(why) => not(&format!("뒤 신원을 읽지 못했다: {why}")),
    }
}

/// 알림의 모양.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeShape {
    RunUnknown {
        origin: pb::RunUnknownOrigin,
        reason: pb::RunUnknownReason,
    },
    StopConfirmed(pb::RunStopEvidence),
}

/// 원장 행의 신원으로 알림을 만들어 노드 키로 서명한다. 조합 규칙을 통과해야 한다(보내기 전에 거른다).
pub fn build_notice(
    signing_key: &SigningKey,
    row: &AttemptRow,
    shape: NoticeShape,
    sequence: u64,
    observed_at_unix_ms: u64,
    issued_at_unix_ms: u64,
) -> Result<NewNotice, String> {
    let (Some(job_id), Some(node_id), Some(fence_epoch)) =
        (row.job_id.as_deref(), row.node_id.as_deref(), row.fence_epoch)
    else {
        return Err(format!(
            "RUN_NOTICE: 원장 행 {} 에 job · node · fence 가 없다 — 알림을 만들 수 없다(legacy 행)",
            row.attempt_id
        ));
    };
    let (kind, origin, reason, stop_evidence, ledger_kind) = match shape {
        NoticeShape::RunUnknown { origin, reason } => (
            pb::RunNoticeKind::RunUnknown,
            origin as i32,
            reason as i32,
            0,
            NoticeKind::RunUnknown,
        ),
        NoticeShape::StopConfirmed(evidence) => (
            pb::RunNoticeKind::StopConfirmed,
            0,
            0,
            evidence as i32,
            NoticeKind::StopConfirmed,
        ),
    };
    let mut notice = pb::AttemptRunNotice {
        schema_version: 1,
        job_id: job_id.to_string(),
        attempt_id: row.attempt_id.clone(),
        node_id: node_id.to_string(),
        fence_epoch,
        kind: kind as i32,
        origin,
        reason,
        stop_evidence,
        sequence,
        observed_at_unix_ms,
        issued_at_unix_ms,
        ..Default::default()
    };
    gputeer_protocol::attempt_run_notice_rules::validate_attempt_run_notice(&notice)
        .map_err(|error| format!("RUN_NOTICE: 조합 규칙 위반 — {error:?}"))?;
    notice.node_signature = sign(signing_key, &notice).to_vec();
    let notice_hash = blake3_256(&signing_input(&notice));
    Ok(NewNotice {
        sequence,
        kind: ledger_kind,
        notice_bytes: notice.encode_to_vec(),
        notice_hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_ledger::Executor;
    use std::cell::RefCell;

    const OWNER: &str = "node-1.root";

    /// 흉내 — 신원은 차례로 돌려준다(앞 · 뒤를 다르게 둘 수 있다), 조회는 "있는 것" 목록 · 실패할 조회로 답한다. 받은 물음을 적는다.
    struct Fake {
        identities: RefCell<Vec<Result<String, String>>>,
        present: Vec<String>,
        failing: Option<&'static str>,
        asked: RefCell<Vec<String>>,
    }

    impl Fake {
        fn absent_everywhere() -> Self {
            Fake {
                identities: RefCell::new(vec![Ok("docker:D1".into()), Ok("docker:D1".into())]),
                present: vec![],
                failing: None,
                asked: RefCell::new(vec![]),
            }
        }
    }

    impl RuntimeQueries for Fake {
        fn identity(&self, endpoint: &RuntimeEndpoint) -> Result<String, String> {
            self.asked.borrow_mut().push(format!("identity {}", endpoint.to_ledger()));
            self.identities.borrow_mut().remove(0)
        }
        fn lookup(&self, endpoint: &RuntimeEndpoint, lookup: &Lookup<'_>) -> Result<Presence, String> {
            let (tag, key) = match lookup {
                Lookup::Id(id) => ("id", id.to_string()),
                Lookup::Name(name) => ("name", name.to_string()),
                Lookup::OwnerLabelWithName { owner, name } => {
                    assert_eq!(*owner, OWNER);
                    ("owner", name.to_string())
                }
            };
            self.asked
                .borrow_mut()
                .push(format!("{tag} {key} @ {}", endpoint.to_ledger()));
            if self.failing == Some(tag) {
                return Err("daemon: connection refused".into());
            }
            Ok(if self.present.contains(&format!("{tag}:{key}")) {
                Presence::Present
            } else {
                Presence::Absent
            })
        }
    }

    fn pinned_row() -> AttemptRow {
        let mut row = AttemptRow::new_active("attempt-1", "job-1", "node-1", 4, Executor::Container);
        row.container_name = Some("gputeer-attempt-1".into());
        row.container_id = Some("cid-1".into());
        row.connection_target = Some("docker-host:unix:///run/docker.sock".into());
        row.runtime_target_identity = Some("docker:D1".into());
        row
    }

    #[test]
    fn absence_needs_all_three_lookups_absent_and_the_same_identity_before_and_after() {
        let fake = Fake::absent_everywhere();
        assert_eq!(startup_absence_evidence(&pinned_row(), OWNER, &fake), AutoEvidence::Absent);
        let asked = fake.asked.borrow().clone();
        assert_eq!(asked.len(), 5, "{asked:?}");
        assert!(asked[0].starts_with("identity") && asked[4].starts_with("identity"), "{asked:?}");
        for (i, tag) in [(1, "id cid-1"), (2, "owner gputeer-attempt-1"), (3, "name gputeer-attempt-1")] {
            assert!(asked[i].starts_with(tag), "{asked:?}");
            assert!(asked[i].ends_with("docker-host:unix:///run/docker.sock"), "고정한 대상에 묻지 않았다: {asked:?}");
        }
    }

    #[test]
    fn any_presence_error_or_identity_change_is_no_evidence() {
        for (tag, key) in [("id", "cid-1"), ("owner", "gputeer-attempt-1"), ("name", "gputeer-attempt-1")] {
            let mut fake = Fake::absent_everywhere();
            fake.present = vec![format!("{tag}:{key}")];
            assert!(
                matches!(startup_absence_evidence(&pinned_row(), OWNER, &fake), AutoEvidence::NotEstablished { .. }),
                "{tag} 에 나왔는데 증거로 봤다"
            );
            let mut fake = Fake::absent_everywhere();
            fake.failing = Some(match tag { "id" => "id", "owner" => "owner", _ => "name" });
            assert!(matches!(
                startup_absence_evidence(&pinned_row(), OWNER, &fake),
                AutoEvidence::NotEstablished { .. }
            ));
        }
        for identities in [
            vec![Ok("docker:D2".to_string()), Ok("docker:D2".to_string())],
            vec![Ok("docker:D1".to_string()), Ok("docker:D9".to_string())],
            vec![Err("timeout".to_string()), Ok("docker:D1".to_string())],
            vec![Ok("docker:D1".to_string()), Err("timeout".to_string())],
        ] {
            let mut fake = Fake::absent_everywhere();
            fake.identities = RefCell::new(identities.clone());
            assert!(
                matches!(startup_absence_evidence(&pinned_row(), OWNER, &fake), AutoEvidence::NotEstablished { .. }),
                "{identities:?}"
            );
        }
    }

    #[test]
    fn rows_without_an_id_target_or_owner_never_produce_evidence_and_ask_nothing() {
        let mut no_id = pinned_row();
        no_id.container_id = None;
        let mut unpinned = pinned_row();
        unpinned.connection_target = None;
        unpinned.runtime_target_identity = None;
        for (row, owner, id_recorded) in [(no_id, OWNER, false), (unpinned, OWNER, true), (pinned_row(), " ", true)] {
            let fake = Fake::absent_everywhere();
            match startup_absence_evidence(&row, owner, &fake) {
                AutoEvidence::NotEstablished { id_recorded: recorded, .. } => assert_eq!(recorded, id_recorded),
                other => panic!("증거로 봤다: {other:?}"),
            }
            assert!(fake.asked.borrow().is_empty(), "증거 조건이 없는데 런타임에 물었다");
        }
    }

    #[test]
    fn notices_are_signed_valid_and_hash_their_signing_input() {
        let key = SigningKey::from_bytes(&[5; 32]);
        let row = pinned_row();
        let stop = build_notice(
            &key,
            &row,
            NoticeShape::StopConfirmed(pb::RunStopEvidence::ContainerAbsentConfirmed),
            3,
            100,
            101,
        )
        .unwrap();
        let decoded = pb::AttemptRunNotice::decode(stop.notice_bytes.as_slice()).unwrap();
        assert_eq!(decoded.attempt_id, "attempt-1");
        assert_eq!(decoded.fence_epoch, 4);
        assert_eq!(decoded.sequence, 3);
        assert_eq!(decoded.kind, pb::RunNoticeKind::StopConfirmed as i32);
        assert_eq!(stop.kind, NoticeKind::StopConfirmed);
        assert_eq!(stop.notice_hash, blake3_256(&signing_input(&decoded)));
        let mut keys = gputeer_crypto::InMemoryKeyring::new();
        keys.insert("node-1", key.verifying_key());
        gputeer_protocol::signing::verify(
            &decoded,
            1,
            &gputeer_crypto::Ed25519Verifier::new(keys),
            999,
            &mut gputeer_protocol::signing::NoReplayCheck,
        )
        .expect("노드 키 서명이 검증돼야 한다");
        let unknown = build_notice(
            &key,
            &row,
            NoticeShape::RunUnknown {
                origin: pb::RunUnknownOrigin::Running,
                reason: pb::RunUnknownReason::ExitUnobserved,
            },
            1,
            100,
            101,
        )
        .unwrap();
        assert_eq!(unknown.kind, NoticeKind::RunUnknown);
        // 조합 규칙 위반은 서명 전에 거른다
        assert!(build_notice(
            &key,
            &row,
            NoticeShape::StopConfirmed(pb::RunStopEvidence::Unspecified),
            1,
            1,
            1
        )
        .is_err());
        assert!(build_notice(&key, &row, NoticeShape::StopConfirmed(pb::RunStopEvidence::ContainerAbsentConfirmed), 0, 1, 1).is_err());
    }
}
