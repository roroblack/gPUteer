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
    /// 해제 명령만 — 로그를 건지고 sync 한다(§4 ①). 기동은 부르지 않는다.
    fn salvage_logs(&self, endpoint: &RuntimeEndpoint, target: &str) -> Result<(), String>;
    /// 해제 명령만 — `rm -f -v`(§4 ②). 응답만으로 지웠다고 보지 않는다(뒤이은 조회가 판정).
    fn remove(&self, endpoint: &RuntimeEndpoint, target: &str) -> Result<(), String>;
}

/// 운영 — 원장 행에 적힌 런타임 실행 파일로 묻는다. `salvage_dir` 는 해제 명령만 준다(`<루트>.leftover-container-logs/`).
pub struct CliQueries<'a> {
    pub program: &'a std::path::Path,
    pub salvage_dir: Option<&'a std::path::Path>,
}

impl RuntimeQueries for CliQueries<'_> {
    fn identity(&self, endpoint: &RuntimeEndpoint) -> Result<String, String> {
        runtime_target::read_identity(self.program, endpoint)
    }
    fn lookup(&self, endpoint: &RuntimeEndpoint, lookup: &Lookup<'_>) -> Result<Presence, String> {
        runtime_target::pinned_lookup(self.program, endpoint, lookup)
    }
    fn salvage_logs(&self, endpoint: &RuntimeEndpoint, target: &str) -> Result<(), String> {
        let dir = self
            .salvage_dir
            .ok_or_else(|| "로그 보존 폴더가 없다(기동은 로그를 건지지 않는다)".to_string())?;
        let (stdout, stderr) = runtime_target::pinned_salvage_logs(self.program, endpoint, target, dir)?;
        println!(
            "RUN_RELEASE_LOGS_SALVAGED target={target} stdout={} stderr={}",
            stdout.display(),
            stderr.display()
        );
        Ok(())
    }
    fn remove(&self, endpoint: &RuntimeEndpoint, target: &str) -> Result<(), String> {
        runtime_target::pinned_remove(self.program, endpoint, target)
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

/// ★ 2026-10-03 11:25 (조각 5d2 · 계약 §4 "ID 없는 행") — 소유자 진술의 근거 종류(위험을 줄인 정도를 감사에 남긴다 — 위험을 없애지는 않는다).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerAttestBasis {
    /// 그 Agent 가 죽은 **뒤** 런타임(데몬 · podman 서비스)을 다시 시작했다 — 그 전에 받은 요청이 남지 않게 했다.
    RuntimeRestarted,
    /// 직접 컨테이너 목록만 확인했다(가장 약하다).
    ListedOnly,
}

impl OwnerAttestBasis {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "runtime-restarted" => Ok(Self::RuntimeRestarted),
            "listed-only" => Ok(Self::ListedOnly),
            other => Err(format!(
                "--owner-attest-basis 는 runtime-restarted 또는 listed-only 다(받은 값 {other:?})"
            )),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeRestarted => "runtime-restarted",
            Self::ListedOnly => "listed-only",
        }
    }
}

/// 소유자 진술 — "그 시도의 컨테이너가 이 런타임에 없음을 확인했고, 늦게 생기거나 시작될 위험을 알고 떠맡는다. 이 런타임이 그 Agent 가 실행에 쓴
/// 런타임이다"(계약 §4). 문장은 비어 있으면 안 된다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerAttestation {
    pub basis: OwnerAttestBasis,
    pub statement: String,
}

/// 해제 명령의 증거.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseEvidence {
    /// 확인한 ID · 고정한 대상으로 로그 보존 → 지움 → 새 조회 셋 모두 없음 · 신원 앞뒤 원장과 같음 — `CONTAINER_ABSENT_CONFIRMED`.
    ContainerAbsentConfirmed,
    /// ID(또는 고정 대상)가 없는 행 — 이름 · owner 라벨로 같은 절차를 밟았고 소유자 진술로 늦은 생성 위험을 떠맡았다 — `CONTAINER_ABSENT_OWNER_ATTESTED`.
    ///   `endpoint` · `identity` 는 실제로 물은 대상(감사에 남긴다).
    OwnerAttested { endpoint: String, identity: String },
}

/// ★ 조각 5d2(계약 §4 — 해제 명령) — OPEN 행을 풀 증거를 만든다. 로그를 건진 뒤에만 지운다. 하나라도 확인하지 못하면 `Err`(아무것도 서명하지 않는다).
///
/// `fallback_endpoint` — 원장에 연결 대상이 없는 행에서 물을 대상(해제 명령이 그 행의 런타임 실행 파일 · 종류로 해석한 값 — 모든 명령에 명시 인자로 준다).
pub fn release_evidence(
    row: &AttemptRow,
    owner: &str,
    attestation: Option<&OwnerAttestation>,
    fallback_endpoint: Option<RuntimeEndpoint>,
    queries: &dyn RuntimeQueries,
) -> Result<ReleaseEvidence, String> {
    let name = row
        .container_name
        .as_deref()
        .ok_or_else(|| "원장에 컨테이너 이름이 없다".to_string())?;
    if owner.trim().is_empty() {
        return Err("owner 라벨이 비었다 — 빈 라벨로 \"없음\" 을 얻지 않는다".into());
    }
    let pinned = match (
        row.container_id.as_deref(),
        row.connection_target.as_deref(),
        row.runtime_target_identity.as_deref(),
    ) {
        (Some(id), Some(target), Some(identity)) => Some((id, target, identity)),
        _ => None,
    };
    if let Some((id, target, recorded)) = pinned {
        let endpoint = RuntimeEndpoint::from_ledger(target)?;
        expect_identity(queries, &endpoint, Some(recorded), "앞")?;
        queries
            .salvage_logs(&endpoint, id)
            .map_err(|why| format!("로그를 건지지 못해 지우지 않았다: {why}"))?;
        if let Err(why) = queries.remove(&endpoint, id) {
            println!("RUN_RELEASE_RM_REPORTED_FAILURE target={id} detail={why} — 뒤이은 조회로 판정한다");
        }
        for lookup in [
            Lookup::Id(id),
            Lookup::OwnerLabelWithName { owner, name },
            Lookup::Name(name),
        ] {
            expect_absent(queries, &endpoint, &lookup)?;
        }
        expect_identity(queries, &endpoint, Some(recorded), "뒤")?;
        return Ok(ReleaseEvidence::ContainerAbsentConfirmed);
    }
    let attestation = attestation.ok_or_else(|| {
        "RUN_RELEASE_NEEDS_OWNER_ATTESTATION: 이 행은 확인한 컨테이너 ID · 고정한 런타임 대상이 없다(늦은 생성을 기계로 배제할 수 없다) — \
         --owner-attest-basis 와 --owner-attest-statement 로 소유자가 위험을 떠맡는다고 적어야 푼다"
            .to_string()
    })?;
    if attestation.statement.trim().is_empty() {
        return Err("소유자 진술 문장이 비었다".into());
    }
    let endpoint = match row.connection_target.as_deref() {
        Some(target) => RuntimeEndpoint::from_ledger(target)?,
        None => fallback_endpoint
            .ok_or_else(|| "물을 런타임 대상을 정하지 못했다(원장에 대상이 없고 해석도 못 했다)".to_string())?,
    };
    let before = expect_identity(queries, &endpoint, row.runtime_target_identity.as_deref(), "앞")?;
    queries
        .salvage_logs(&endpoint, name)
        .map_err(|why| format!("로그를 건지지 못해 지우지 않았다: {why}"))?;
    if let Err(why) = queries.remove(&endpoint, name) {
        println!("RUN_RELEASE_RM_REPORTED_FAILURE target={name} detail={why} — 뒤이은 조회로 판정한다");
    }
    for lookup in [Lookup::OwnerLabelWithName { owner, name }, Lookup::Name(name)] {
        expect_absent(queries, &endpoint, &lookup)?;
    }
    expect_identity(queries, &endpoint, Some(&before), "뒤")?;
    Ok(ReleaseEvidence::OwnerAttested {
        endpoint: endpoint.to_ledger(),
        identity: before,
    })
}

fn expect_identity(
    queries: &dyn RuntimeQueries,
    endpoint: &RuntimeEndpoint,
    expected: Option<&str>,
    when: &str,
) -> Result<String, String> {
    let identity = queries
        .identity(endpoint)
        .map_err(|why| format!("{when} 신원을 읽지 못했다: {why}"))?;
    if let Some(expected) = expected {
        if identity != expected {
            return Err(format!(
                "{when} 신원이 다르다(기대 {expected:?} · 지금 {identity:?}) — 다른 런타임에 묻고 있다"
            ));
        }
    }
    Ok(identity)
}

fn expect_absent(
    queries: &dyn RuntimeQueries,
    endpoint: &RuntimeEndpoint,
    lookup: &Lookup<'_>,
) -> Result<(), String> {
    match queries.lookup(endpoint, lookup) {
        Ok(Presence::Absent) => Ok(()),
        Ok(Presence::Present) => Err(format!("조회 {lookup:?} 에 아직 나온다 — 없음을 확인하지 못했다")),
        Err(why) => Err(format!("조회 {lookup:?} 가 답하지 않았다: {why}")),
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
        /// 지운 대상(지운 뒤 그 ID · 이름은 조회에 나오지 않는다 — 이름이 지워지면 owner 라벨 목록에서도 빠진다).
        removed: RefCell<Vec<String>>,
    }

    impl Fake {
        fn absent_everywhere() -> Self {
            Fake {
                identities: RefCell::new(vec![Ok("docker:D1".into()), Ok("docker:D1".into())]),
                present: vec![],
                failing: None,
                asked: RefCell::new(vec![]),
                removed: RefCell::new(vec![]),
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
            Ok(if self.present.contains(&format!("{tag}:{key}")) && !self.removed.borrow().contains(&key) {
                Presence::Present
            } else {
                Presence::Absent
            })
        }
        fn salvage_logs(&self, endpoint: &RuntimeEndpoint, target: &str) -> Result<(), String> {
            self.asked.borrow_mut().push(format!("salvage {target} @ {}", endpoint.to_ledger()));
            if self.failing == Some("salvage") {
                return Err("logs: daemon error".into());
            }
            Ok(())
        }
        fn remove(&self, endpoint: &RuntimeEndpoint, target: &str) -> Result<(), String> {
            self.asked.borrow_mut().push(format!("rm {target} @ {}", endpoint.to_ledger()));
            if self.failing != Some("rm-noop") {
                self.removed.borrow_mut().push(target.to_string());
            }
            Ok(())
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

    /// 해제 명령 — 확인한 ID · 고정한 대상: 로그 보존 → 그 ID 로 지움 → 새 조회 셋 → 신원 앞뒤. 남아 있던 컨테이너도 이 절차 뒤 없으면 증거다.
    #[test]
    fn release_salvages_then_removes_by_id_and_confirms_absence() {
        let mut fake = Fake::absent_everywhere();
        fake.present = vec!["id:cid-1".into(), "name:gputeer-attempt-1".into()];
        // 남아 있는 컨테이너는 ID · 이름 둘 다로 나온다 — ID 로 지우면 둘 다 사라진다고 흉내 낸다
        let row = pinned_row();
        let evidence = release_evidence(&row, OWNER, None, None, &fake);
        // 이름 조회는 이름으로 지운 적이 없어 "있음" 이다 — 흉내에서 이름도 지워진 것으로 친다
        assert!(evidence.is_err(), "이름으로 남아 있는데 증거로 봤다");
        let mut fake = Fake::absent_everywhere();
        fake.present = vec!["id:cid-1".into()];
        assert_eq!(
            release_evidence(&row, OWNER, None, None, &fake).unwrap(),
            ReleaseEvidence::ContainerAbsentConfirmed
        );
        let asked = fake.asked.borrow().clone();
        let order: Vec<&str> = asked.iter().map(|a| a.split(' ').next().unwrap()).collect();
        assert_eq!(order, ["identity", "salvage", "rm", "id", "owner", "name", "identity"], "{asked:?}");
        assert!(asked[1].starts_with("salvage cid-1") && asked[2].starts_with("rm cid-1"), "확인한 ID 로 하지 않았다: {asked:?}");
    }

    /// 로그를 건지지 못하면 지우지 않는다 · rm 이 받아들이고도 남아 있으면 증거가 아니다 · 신원이 바뀌면 증거가 아니다.
    #[test]
    fn release_never_removes_without_salvaged_logs_and_trusts_only_fresh_lookups() {
        let row = pinned_row();
        let mut fake = Fake::absent_everywhere();
        fake.failing = Some("salvage");
        assert!(release_evidence(&row, OWNER, None, None, &fake).is_err());
        assert!(!fake.asked.borrow().iter().any(|a| a.starts_with("rm")), "로그를 못 건졌는데 지웠다");

        let mut fake = Fake::absent_everywhere();
        fake.present = vec!["id:cid-1".into()];
        fake.failing = Some("rm-noop");
        assert!(release_evidence(&row, OWNER, None, None, &fake).is_err(), "rm 응답만 믿었다");

        let mut fake = Fake::absent_everywhere();
        fake.identities = RefCell::new(vec![Ok("docker:D1".into()), Ok("docker:D2".into())]);
        assert!(release_evidence(&row, OWNER, None, None, &fake).is_err());
    }

    /// ID(또는 고정 대상)가 없는 행 — 소유자 진술 없이는 거부(아무것도 묻지 않는다) · 진술이 있으면 이름으로 같은 절차를 밟고 실제로 물은 대상 · 신원을 돌려준다.
    #[test]
    fn an_id_less_row_needs_an_owner_attestation_and_goes_by_name() {
        let mut row = pinned_row();
        row.container_id = None;
        row.connection_target = None;
        row.runtime_target_identity = None;
        let fake = Fake::absent_everywhere();
        let refused = release_evidence(&row, OWNER, None, Some(RuntimeEndpoint::DockerHost("unix:///x.sock".into())), &fake)
            .unwrap_err();
        assert!(refused.contains("RUN_RELEASE_NEEDS_OWNER_ATTESTATION"), "{refused}");
        assert!(fake.asked.borrow().is_empty(), "진술 없이 런타임에 물었다");

        let attestation = OwnerAttestation {
            basis: OwnerAttestBasis::RuntimeRestarted,
            statement: "데몬을 재시작했고 목록에 없다".into(),
        };
        let empty = OwnerAttestation { basis: OwnerAttestBasis::ListedOnly, statement: "  ".into() };
        assert!(release_evidence(&row, OWNER, Some(&empty), Some(RuntimeEndpoint::DockerHost("unix:///x.sock".into())), &fake).is_err());
        assert!(release_evidence(&row, OWNER, Some(&attestation), None, &fake).is_err(), "물을 대상 없이 증거를 만들었다");
        let fake = Fake::absent_everywhere();
        let evidence = release_evidence(
            &row,
            OWNER,
            Some(&attestation),
            Some(RuntimeEndpoint::DockerHost("unix:///x.sock".into())),
            &fake,
        )
        .unwrap();
        assert_eq!(
            evidence,
            ReleaseEvidence::OwnerAttested {
                endpoint: "docker-host:unix:///x.sock".into(),
                identity: "docker:D1".into()
            }
        );
        let asked = fake.asked.borrow().clone();
        let order: Vec<&str> = asked.iter().map(|a| a.split(' ').next().unwrap()).collect();
        assert_eq!(order, ["identity", "salvage", "rm", "owner", "name", "identity"], "{asked:?}");
        assert!(asked[1].starts_with("salvage gputeer-attempt-1"), "이름으로 하지 않았다: {asked:?}");
        assert_eq!(OwnerAttestBasis::parse("listed-only").unwrap(), OwnerAttestBasis::ListedOnly);
        assert!(OwnerAttestBasis::parse("trust-me").is_err());
    }
}
