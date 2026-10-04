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
    /// 기동(조각 5e1) — 그 ID 의 실행 상태.
    fn run_state(&self, endpoint: &RuntimeEndpoint, id: &str) -> Result<runtime_target::RunState, String>;
    /// 기동(조각 5e1) — 멈추고 조회로 확인한다. 지우지 않는다.
    fn stop(&self, endpoint: &RuntimeEndpoint, id: &str) -> Result<(), String>;
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
    fn run_state(&self, endpoint: &RuntimeEndpoint, id: &str) -> Result<runtime_target::RunState, String> {
        runtime_target::pinned_run_state(self.program, endpoint, id)
    }
    fn stop(&self, endpoint: &RuntimeEndpoint, id: &str) -> Result<(), String> {
        runtime_target::pinned_stop(self.program, endpoint, id)
    }
}

/// ★ 2026-10-03 11:37 (조각 5e1 · 계약 §5 기동 관문 v18i/j) — 재기동 때 확인한 ID 의 컨테이너가 **돌고 있는가** 를 먼저 본다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunningAtRestart {
    /// 돌고 있지 않다(멈춤 · 없음 · 얼림) 또는 볼 수 없다(ID · 대상 없음 · 신원 다름 · 조회 실패) — 다음은 §4 증거 판정.
    NotRunning,
    /// 돌고 있었다 → 멈췄다(지우지 않음). 재부착(B′ — 갱신으로 확인한 뒤 다시 붙기)은 조각 5e2 — 그 전까지는 감시 없이 두지 않고 멈춘다(보수).
    ///   컨테이너는 남으므로 "이미 없음" 이 아니다 → OPEN.
    StoppedWhileRunning,
    /// 돌고 있었는데 멈추지 못했다 → OPEN + RUN_UNKNOWN(사유 "정지 실패") — 계약 v18j.
    StopFailed(String),
}

/// 재기동 때 확인한 ID 의 컨테이너가 돌고 있으면 멈춘다. 신원이 원장과 다르거나 볼 수 없으면 아무것도 하지 않는다(다른 런타임의 것을 멈추지 않는다).
pub fn stop_if_running_at_restart(row: &AttemptRow, queries: &dyn RuntimeQueries) -> RunningAtRestart {
    let (Some(id), Some(target), Some(recorded)) = (
        row.container_id.as_deref(),
        row.connection_target.as_deref(),
        row.runtime_target_identity.as_deref(),
    ) else {
        return RunningAtRestart::NotRunning;
    };
    let Ok(endpoint) = RuntimeEndpoint::from_ledger(target) else {
        return RunningAtRestart::NotRunning;
    };
    if queries.identity(&endpoint).as_deref() != Ok(recorded) {
        return RunningAtRestart::NotRunning;
    }
    match queries.run_state(&endpoint, id) {
        Ok(runtime_target::RunState::Running) => match queries.stop(&endpoint, id) {
            Ok(()) => RunningAtRestart::StoppedWhileRunning,
            Err(why) => RunningAtRestart::StopFailed(why),
        },
        _ => RunningAtRestart::NotRunning,
    }
}

/// ★ 2026-10-04 13:52 (조각 5e2c · 계약 v18n 조건 ①~⑤ · ⑦ · ⑧ · v18o ② ③ · v18j ②) — 재기동 때 ACTIVE 컨테이너 행이 **재부착 후보**인가.
///   이 판정은 아무 명령도 바꾸지 않는다(신원 · 상태 조회만) — 발견 단계다(v18o ⑤). 승인(마커 다시 쓰기 · 갱신 확인)은 재부착 회차(5e2d)가 한다.
#[derive(Debug, Clone, PartialEq)]
pub enum ReattachVerdict {
    /// 모든 조건이 참 — 승인 단계로 넘긴다.
    Candidate(Box<ReattachInputs>),
    /// 재부착하지 않는다. `stop_allowed` 는 v18o ② — 대상 신원을 확인한 컨테이너에만 stop 을 보낸다(거짓이면 아무 명령 없이 OPEN).
    NotCandidate { why: String, stop_allowed: bool },
}

/// 재부착 회차가 쓰는 입력(원장 바이트를 풀어 대조한 값).
#[derive(Debug, Clone, PartialEq)]
pub struct ReattachInputs {
    pub endpoint: RuntimeEndpoint,
    pub container_id: String,
    pub grant: pb::ExecutionGrant,
    pub last_lease: pb::Lease,
    pub self_stop_at_unix_ms: u64,
    pub started_at_unix_ms: u64,
}

/// 재부착 후보 판정. `incident_open` 은 그 컨테이너의 열린 사건 표식이 있는가(조건 ⑧ — 기동은 원장 판정이 사건 표식 검사보다 먼저라 여기서 직접 받는다).
pub fn evaluate_reattach(
    row: &AttemptRow,
    queries: &dyn RuntimeQueries,
    incident_open: bool,
    now_unix_ms: u64,
) -> ReattachVerdict {
    use prost::Message;
    let no = |why: &str, stop_allowed: bool| ReattachVerdict::NotCandidate { why: why.to_string(), stop_allowed };
    // ⑦ 정지 결정 · ⑧ 사건 표식 — 대상과 무관하게 먼저 본다(정지는 확인된 대상에만 — 아래에서 다시 가른다)
    if row.stop_decision.is_some() {
        return no("정지 결정이 이미 기록됐다(조건 ⑦)", true);
    }
    if incident_open {
        return no("그 컨테이너의 열린 사건 표식이 있다(조건 ⑧)", true);
    }
    // ② 근거와 입력
    let (Some(grant_bytes), Some(started_at), Some(lease_bytes), Some(self_stop_at)) = (
        row.reattach_grant.as_deref(),
        row.started_at_unix_ms,
        row.last_lease.as_deref(),
        row.self_stop_at_unix_ms,
    ) else {
        return no("재부착 입력 · 갱신 근거가 원장에 없다(조건 ②)", true);
    };
    let (Some(id), Some(target), Some(recorded)) = (
        row.container_id.as_deref(),
        row.connection_target.as_deref(),
        row.runtime_target_identity.as_deref(),
    ) else {
        return no("원장에 확인한 ID · 대상 · 신원이 없다", false);
    };
    let Ok(endpoint) = RuntimeEndpoint::from_ledger(target) else {
        return no("원장의 런타임 대상을 읽지 못했다", false);
    };
    // ③ 저장 Grant · 마지막 Lease 의 신원 대조(v18o ③ — grant ID 는 대조하지 않는다)
    let (Ok(grant), Ok(last_lease)) = (pb::ExecutionGrant::decode(grant_bytes), pb::Lease::decode(lease_bytes)) else {
        return no("저장된 Grant · Lease 바이트를 풀지 못했다(조건 ③)", true);
    };
    let Some(grant_lease) = grant.lease.as_ref() else {
        return no("저장된 Grant 에 Lease 가 없다(조건 ③)", true);
    };
    let row_job = row.job_id.as_deref().unwrap_or("");
    let row_node = row.node_id.as_deref().unwrap_or("");
    let row_fence = row.fence_epoch.unwrap_or(u64::MAX);
    let manifest_job = grant.manifest.as_ref().map(|m| m.job_id.as_str());
    let identity_matches = grant.attempt_id == row.attempt_id
        && manifest_job.is_none_or(|job| job == row_job)
        && grant_lease.job_id == row_job
        && grant_lease.attempt_id == row.attempt_id
        && grant_lease.holder_node_id == row_node
        && grant_lease.fence_epoch == row_fence
        && last_lease.lease_id == grant_lease.lease_id
        && last_lease.job_id == row_job
        && last_lease.attempt_id == row.attempt_id
        && last_lease.fence_epoch == row_fence;
    if !identity_matches {
        return no("저장된 Grant · 마지막 Lease · 원장 행의 신원이 서로 다르다(조건 ③ — 변조 · 섞임)", true);
    }
    // ④ 대상 신원 — 다르거나 묻지 못하면 **아무 명령도 보내지 않는다**(v18o ②)
    match queries.identity(&endpoint) {
        Ok(found) if found == recorded => {}
        Ok(_) => return no("런타임 대상의 신원이 원장과 다르다(조건 ④)", false),
        Err(why) => return no(&format!("런타임 대상의 신원을 확인하지 못했다(조건 ④ — {why})"), false),
    }
    // ⑤ running 만 — 얼림 · 멈춤 · 생성됨 · 없음은 후보가 아니다
    match queries.run_state(&endpoint, id) {
        Ok(runtime_target::RunState::Running) => {}
        Ok(other) => return no(&format!("돌고 있지 않다({other:?} — 조건 ⑤)"), true),
        Err(why) => return no(&format!("상태를 확인하지 못했다(조건 ⑤ — {why})"), false),
    }
    // v18j ② — 원장의 시한이 지났거나 Lease 가 로컬로 만료면 묻지 않고 stop
    if now_unix_ms >= self_stop_at || last_lease.expires_at_unix_ms <= now_unix_ms {
        return no("원장의 끊김 시한이 지났거나 Lease 가 로컬로 만료됐다(v18j ②)", true);
    }
    ReattachVerdict::Candidate(Box::new(ReattachInputs {
        endpoint,
        container_id: id.to_string(),
        grant,
        last_lease,
        self_stop_at_unix_ms: self_stop_at,
        started_at_unix_ms: started_at,
    }))
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
        fn run_state(&self, endpoint: &RuntimeEndpoint, id: &str) -> Result<runtime_target::RunState, String> {
            self.asked.borrow_mut().push(format!("state {id} @ {}", endpoint.to_ledger()));
            match self.failing {
                Some("running") => Ok(runtime_target::RunState::Running),
                Some("paused") => Ok(runtime_target::RunState::Paused),
                Some("running-stuck") => Ok(runtime_target::RunState::Running),
                _ => Ok(runtime_target::RunState::NotRunning),
            }
        }
        fn stop(&self, endpoint: &RuntimeEndpoint, id: &str) -> Result<(), String> {
            self.asked.borrow_mut().push(format!("stop {id} @ {}", endpoint.to_ledger()));
            if self.failing == Some("running-stuck") {
                return Err("kill 뒤에도 Running".into());
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

    /// 재기동 때 돌고 있으면 멈춘다(지우지 않음) · 얼린 것 · 멈춘 것은 건드리지 않는다 · 멈추지 못하면 그 사유 · 신원이 다르면 아무것도 하지 않는다.
    #[test]
    fn a_container_still_running_at_restart_is_stopped_never_removed() {
        let row = pinned_row();
        let mut fake = Fake::absent_everywhere();
        fake.failing = Some("running");
        assert_eq!(stop_if_running_at_restart(&row, &fake), RunningAtRestart::StoppedWhileRunning);
        let asked = fake.asked.borrow().clone();
        assert!(asked.iter().any(|a| a.starts_with("stop cid-1 @ docker-host:")), "{asked:?}");
        assert!(!asked.iter().any(|a| a.starts_with("rm")), "지웠다: {asked:?}");

        let mut fake = Fake::absent_everywhere();
        fake.failing = Some("running-stuck");
        assert!(matches!(stop_if_running_at_restart(&row, &fake), RunningAtRestart::StopFailed(_)));

        for state in ["paused", "none"] {
            let mut fake = Fake::absent_everywhere();
            fake.failing = Some(state);
            assert_eq!(stop_if_running_at_restart(&row, &fake), RunningAtRestart::NotRunning, "{state}");
            assert!(!fake.asked.borrow().iter().any(|a| a.starts_with("stop")), "{state} 를 멈췄다");
        }

        let mut fake = Fake::absent_everywhere();
        fake.failing = Some("running");
        fake.identities = RefCell::new(vec![Ok("docker:OTHER".into())]);
        assert_eq!(stop_if_running_at_restart(&row, &fake), RunningAtRestart::NotRunning);
        assert!(!fake.asked.borrow().iter().any(|a| a.starts_with("stop") || a.starts_with("state")), "다른 런타임의 것을 건드렸다");

        let mut no_id = pinned_row();
        no_id.container_id = None;
        let fake = Fake::absent_everywhere();
        assert_eq!(stop_if_running_at_restart(&no_id, &fake), RunningAtRestart::NotRunning);
        assert!(fake.asked.borrow().is_empty());
    }

    // ─── ★ 조각 5e2c — 재부착 후보 판정(계약 v18n 조건 · v18o ② ③ · v18j ②) ─────────

    fn reattach_row(now: u64) -> AttemptRow {
        use prost::Message;
        let mut row = pinned_row();
        let lease = pb::Lease {
            lease_id: "lease-1".into(),
            job_id: "job-1".into(),
            attempt_id: "attempt-1".into(),
            fence_epoch: 4,
            holder_node_id: "node-1".into(),
            expires_at_unix_ms: now + 60_000,
            ..Default::default()
        };
        let grant = pb::ExecutionGrant {
            grant_id: "grant-1".into(),
            attempt_id: "attempt-1".into(),
            manifest: Some(pb::JobManifest { job_id: "job-1".into(), ..Default::default() }),
            lease: Some(lease.clone()),
            ..Default::default()
        };
        row.reattach_grant = Some(grant.encode_to_vec());
        row.started_at_unix_ms = Some(now - 1_000);
        row.last_lease = Some(lease.encode_to_vec());
        row.self_stop_at_unix_ms = Some(now + 30_000);
        row
    }

    fn running() -> Fake {
        Fake { failing: Some("running"), ..Fake::absent_everywhere() }
    }

    /// 모든 조건이 참이면 후보 — 신원 · 상태만 묻고 아무것도 바꾸지 않는다.
    #[test]
    fn a_running_container_with_full_matching_inputs_is_a_reattach_candidate() {
        let now = 1_000_000;
        let fake = running();
        let ReattachVerdict::Candidate(inputs) = evaluate_reattach(&reattach_row(now), &fake, false, now) else {
            panic!("후보여야 한다");
        };
        assert_eq!(inputs.container_id, "cid-1");
        assert_eq!(inputs.grant.grant_id, "grant-1");
        assert_eq!(inputs.self_stop_at_unix_ms, now + 30_000);
        let asked = fake.asked.borrow().clone();
        assert_eq!(asked.len(), 2, "{asked:?}");
        assert!(asked.iter().all(|a| a.starts_with("identity") || a.starts_with("state")), "판정이 명령을 보냈다: {asked:?}");
    }

    /// 후보가 아닌 경우 — 각 조건이 따로 막는다. 대상을 확인하지 못한 경우만 stop 도 허락하지 않는다(v18o ②).
    #[test]
    fn each_condition_alone_blocks_reattach_and_only_a_confirmed_target_may_be_stopped() {
        let now = 1_000_000;
        let verdict = |row: AttemptRow, fake: &Fake, incident: bool| match evaluate_reattach(&row, fake, incident, now) {
            ReattachVerdict::NotCandidate { why, stop_allowed } => (why, stop_allowed),
            ReattachVerdict::Candidate(_) => panic!("후보가 아니어야 한다"),
        };
        let mut decided = reattach_row(now);
        decided.stop_decision = Some("OWNER".into());
        decided.stop_decision_at_unix_ms = Some(now);
        assert!(verdict(decided, &running(), false).0.contains("⑦"));
        assert!(verdict(reattach_row(now), &running(), true).0.contains("⑧"));
        let mut no_inputs = reattach_row(now);
        no_inputs.reattach_grant = None;
        no_inputs.started_at_unix_ms = None;
        assert!(verdict(no_inputs, &running(), false).0.contains("②"));
        let mut other_fence = reattach_row(now);
        other_fence.fence_epoch = Some(5);
        assert!(verdict(other_fence, &running(), false).0.contains("③"));
        // 대상 신원이 다르다 — 아무 명령도 보내지 않는다
        let other_daemon = Fake { identities: RefCell::new(vec![Ok("docker:OTHER".into())]), ..running() };
        let (why, stop_allowed) = verdict(reattach_row(now), &other_daemon, false);
        assert!(why.contains("④") && !stop_allowed, "{why}");
        assert!(!other_daemon.asked.borrow().iter().any(|a| a.starts_with("state")), "신원이 다른 대상의 상태를 물었다");
        // 신원을 묻지 못했다 — 역시 명령 없음
        let unreachable = Fake { identities: RefCell::new(vec![Err("daemon down".into())]), ..running() };
        assert!(!verdict(reattach_row(now), &unreachable, false).1);
        // 얼림 · 멈춤은 후보가 아니지만 대상은 확인됐다
        let paused = Fake { failing: Some("paused"), ..Fake::absent_everywhere() };
        let (why, stop_allowed) = verdict(reattach_row(now), &paused, false);
        assert!(why.contains("⑤") && stop_allowed, "{why}");
        // 시한이 지났다 · Lease 가 로컬로 만료 — 묻지 않고 stop(v18j ②)
        let mut late = reattach_row(now);
        late.self_stop_at_unix_ms = Some(now);
        assert!(verdict(late, &running(), false).0.contains("v18j"));
    }
}

