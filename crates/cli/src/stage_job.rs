//! `gputeer stage-job` — 큐에 오른 Job 을 **실제로 한 노드에 예약**한다.
//!
//! `DoD-46` 이 `orchestrate` 를 만들며 적어 둔 문장이 이 조각의 이유다:
//!
//! > inventory revision/CAS reservation 이 없어 서로 다른 Job 의 같은 GPU
//! > 중복 선택을 막지 못하므로 test fixture 외 production 경로에는
//! > 연결하지 않았다.
//!
//! ★ **그 차단 이유는 `DoD-47` 이 이미 풀었다.** revision CAS 와 node
//!   PRIMARY KEY 예약이 하나의 `BEGIN IMMEDIATE` 안에 들어갔다. 그래서
//!   이 명령이 그 module 의 **첫 production 호출자**가 된다.
//!
//! ```text
//! plan-job   저장된 Manifest -> JobRequirements -> 적격 판정 -> QUEUED
//! stage-job  QUEUED -> 후보 판정 -> best-fit -> CAS 예약 + Attempt/Lease
//!            + fence epoch -> STAGING                            <- 여기
//! ```
//!
//! # ★ 지어낼 수 없는 것은 전부 인자로 받는다
//!
//! `orchestrate` 는 ID 를 만들지 않고 시계를 읽지 않으며 Lease 수명
//! 정책을 발명하지 않는다. 그 원칙을 CLI 도 그대로 따른다 — 기본값을
//! 여기서 정하면 운영자가 고르지 않은 정책으로 남의 GPU 가 잡힌다.
//!
//! ```text
//! --coordinator-id · --coordinator-term   누가 발급하는가
//! --attempt-id · --lease-id               이 시도의 신원(ULID 26자)
//! --lease-*-unix-ms                       Lease 시각(절대값 — 아래 참조)
//! --best-fit-axes                         자원 축 우선순위
//!                                         (기준 계획서에 고정 순서가 없다)
//! --operation-key                         재시도 멱등 키
//! ```
//!
//! # 이 명령이 하지 않는 것
//!
//! ```text
//! 안 한다   Grant 서명·전송         예약과 발급은 다른 일이다
//! 안 한다   Agent 에게 알리기       wire dispatch 는 아직 없다
//! 안 한다   예약 해제               DoD-62 의 증명 관문이 오늘 다 막는다
//! 안 한다   GPU 별 용량 회계        node-exclusive 예약이다 — 같은 노드의
//!                                   다른 GPU 도 함께 잠긴다
//! ```

use std::collections::BTreeMap;

use gputeer_coordinator::inventory_store::CoordinatorInventoryStore;
use gputeer_coordinator::job_store::CoordinatorJobStore;
use gputeer_coordinator::manifest_requirements::job_requirements_from_manifest;
use gputeer_coordinator::orchestrate::{
    orchestrate_placement_to_staging, PlacementToStagingInput, PlacementToStagingOutcome,
    StagingIssuanceInput,
};
use gputeer_coordinator::staging_store::CoordinatorStagingStore;
use gputeer_crypto::{Ed25519Verifier, PersistentKeyring, PlaintextPolicy};
use gputeer_protocol::signing::{verify, NoReplayCheck};
use gputeer_scheduler::{BestFitPolicy, FitAxis, Policy};

/// `gputeer stage-job` 진입점.
pub fn run(args: &[String]) -> Result<String, String> {
    let flags = parse_flags(args)?;

    let job_id = require(&flags, "--job-id")?;
    let control_db = require(&flags, "--control-db")?;
    let keyring_path = require(&flags, "--submitter-keyring")?;
    let submitter_member = require(&flags, "--submitter-member")?;
    let max_snapshot_age_ms = u64_flag(&flags, "--max-snapshot-age-ms")?;
    let best_fit_policy = parse_axes(require(&flags, "--best-fit-axes")?)?;

    // ★★ **Lease 시각을 여기서 시계로 만들지 않는다.**
    //
    //   처음에는 `--lease-duration-ms` 같은 상대값을 받아 `now()` 에
    //   더했다. 그랬더니 **재시도가 멱등이 아니었다** — 두 번째 실행의
    //   시각이 달라 저장소가 `staging operation key payload conflict` 를
    //   냈다. 통합 테스트가 그걸 잡았다.
    //
    //   그리고 그게 옳은 거부였다. operation key 는 "같은 입력이면 같은
    //   결과" 를 약속하는데, 시계를 읽으면 **입력이 달라질 수 있다** —
    //   같은 입력의 재현을 호출자가 보장하지 못한다(재검수 30 — 전에는 "매번
    //   달라진다 · 멱등하다고 말해 놓고 안 지키는 셈" 이라 적었다).
    //
    //   그래서 재시도에서 Lease 시각을 재현하기 위해 세 시각을 **절대값으로**
    //   받는다 — 운영자가 정하고, 재시도는 같은 값을 준다. (`orchestrate` 가 "시계를
    //   읽지 않는다" 는 것은 함수 안의 조회 금지다. 호출자가 읽어 인자로 넘기는 것까지
    //   막지 않는다 — `scheduler_tick` 은 신선도 판정용 현재 시각을 그렇게 넘긴다.
    //   재검수 32 — 전에는 CLI 가 읽으면 "그 계약을 우회한다" 고 적었다)
    let issued_at_unix_ms = u64_flag(&flags, "--lease-issued-at-unix-ms")?;
    let renew_after_unix_ms = u64_flag(&flags, "--lease-renew-after-unix-ms")?;
    let expires_at_unix_ms = u64_flag(&flags, "--lease-expires-at-unix-ms")?;
    if !(issued_at_unix_ms < renew_after_unix_ms && renew_after_unix_ms < expires_at_unix_ms) {
        return Err(format!(
            "Lease 시각이 발급 < 갱신 < 만료 순서가 아니다(발급 {issued_at_unix_ms}, 갱신 {renew_after_unix_ms}, 만료 {expires_at_unix_ms}) — 갱신 시점이 만료 뒤면 갱신할 기회가 없다"
        ));
    }
    let issuance = StagingIssuanceInput {
        operation_key: parse_key16(require(&flags, "--operation-key")?)?,
        attempt_id: require(&flags, "--attempt-id")?.to_string(),
        lease_id: require(&flags, "--lease-id")?.to_string(),
        issuing_coordinator_id: require(&flags, "--coordinator-id")?.to_string(),
        coordinator_term: u64_flag(&flags, "--coordinator-term")?,
        issued_at_unix_ms,
        renew_after_unix_ms,
        expires_at_unix_ms,
        max_total_duration_seconds: u64_flag(&flags, "--lease-max-total-duration-seconds")?,
    };

    // ★★ **"재시도 때 값이 달라도 저장되는 것이 안 바뀐다" 고 적어 뒀던
    //   것을 지운다 — 틀렸다** (2026-09-07 독립 검수 지적).
    //
    //   이 값은 아래에서 `evaluated_at_unix_ms` 로 들어가 **어느 노드가
    //   신선한가**를 가른다. 그래서 durable 결과에 영향을 준다.
    //
    //   반례(검수 제시): 관측 시각이 다른 노드 A·B 가 있고 A 가 먼저
    //   신선도 한계를 넘으면, 첫 호출은 A 를 고르고 재시도는 B 를 고르거나
    //   `NoEligible` 이 된다. **같은 operation key 와 같은 Lease 시각을
    //   줘도** 예약 payload 가 달라져 conflict 나 다른 실패가 된다.
    //
    //   ★ 즉 이 명령이 보장하는 것은 **"같은 입력이면 같은 저장"** 이
    //     아니라 **"중복 예약은 안 생긴다"** 다. 전자는 시계를 읽는 한
    //     성립하지 않는다. 후자는 `DoD-47` 의 CAS·operation key 가 지킨다.
    //
    //   ★ 아래 Manifest 재검증도 같은 시각을 쓴다. 첫 호출이 예약을
    //     commit 한 뒤 응답이 유실되고, 재시도가 Manifest 만료 뒤에
    //     실행되면 **멱등 결과를 돌려주기 전에** `STAGE_REFUSED` 가
    //     된다. 이것도 호출자 관점의 멱등성을 깬다.
    let now_unix_ms = now_unix_ms();

    // ── 저장된 Manifest 를 **지금 다시** 검증한다 ────────────────────
    //
    // `plan-job` 과 같은 이유다(`DoD-50`: raw binding 은 재검증 전
    // scheduler/Grant 에 쓸 수 없다). 계획할 때 믿었다고 예약할 때도
    // 믿는 것이 아니다.
    //
    // ★★ **여기 "그 사이 서명자가 폐기됐을 수 있다" 고 적혀 있었다 —
    //   과장이다** (2026-09-07 독립 검수 지적).
    //
    //   이 재검증이 실제로 다시 보는 것은 **지금의 keyring 파일과 지금
    //   시각**뿐이다. 폐기 상태를 읽는 곳이 없다 — revocation registry 도,
    //   `revoked_at` 도, membership 상태도 안 본다.
    //
    //   반례(검수 제시): 권한 시스템에서 서명자를 폐기하되
    //   `submitters.keyring` 에 공개키를 그대로 두면 **계속 통과한다.**
    //
    //   ★ 즉 "폐기를 잡는다" 가 아니라 **"운영자가 keyring 에서 그 키를
    //     빼거나 바꿨으면 잡는다"** 다. 그 운영 절차는 코드가 강제하지
    //     않는다. 진짜 폐기 검사는 membership 계층이 생겨야 한다.
    let jobs = CoordinatorJobStore::open(control_db)
        .map_err(|e| format!("job store 를 열지 못했다({control_db}): {e}"))?;
    if !jobs.is_durable() {
        return Err(format!(
            "--control-db 가 영속이 아니다({control_db:?}) — 예약이 프로세스와 함께 사라지는데 로그에는 성공으로 찍힌다"
        ));
    }
    let binding = jobs
        .get_manifest_binding(job_id)
        .map_err(|e| format!("Manifest binding 조회 실패: {e}"))?
        .ok_or_else(|| format!("STAGE_REFUSED: {job_id} 에 저장된 Manifest 가 없다"))?;
    drop(jobs);

    let policy = if flags
        .get("--i-understand-plaintext-keyring-is-unsafe")
        .map(String::as_str)
        == Some("true")
    {
        eprintln!(
            "경고: 평문(K0) 제출자 keyring 을 허용했다 — 이 파일을 쓸 수 있는 사람은 임의의 공개키를 신뢰 목록에 넣을 수 있다"
        );
        PlaintextPolicy::Allow
    } else {
        PlaintextPolicy::Reject
    };
    let keyring = PersistentKeyring::load(keyring_path, policy)
        .map_err(|e| format!("제출자 keyring 을 열지 못했다({keyring_path}): {e:?}"))?;
    let verified = verify(
        &binding.manifest,
        1,
        &Ed25519Verifier::new(&keyring),
        now_unix_ms,
        &mut NoReplayCheck,
    )
    .map_err(|e| format!("STAGE_REFUSED: 저장된 Manifest 를 지금 다시 검증하지 못했다: {e:?}"))?;
    let job_requirements = job_requirements_from_manifest(&verified, submitter_member)
        .map_err(|e| format!("STAGE_REFUSED: {e}"))?;

    // ── 예약 ────────────────────────────────────────────────────────
    let mut inventory = CoordinatorInventoryStore::open(control_db)
        .map_err(|e| format!("inventory store 를 열지 못했다({control_db}): {e}"))?;
    let mut staging = CoordinatorStagingStore::open(control_db)
        .map_err(|e| format!("staging store 를 열지 못했다({control_db}): {e}"))?;

    let outcome = orchestrate_placement_to_staging(
        &mut inventory,
        &mut staging,
        &PlacementToStagingInput {
            job_id: job_id.to_string(),
            job_requirements,
            hard_filter_policy: Policy {
                maximum_snapshot_age_ms: max_snapshot_age_ms,
            },
            best_fit_policy,
            evaluated_at_unix_ms: now_unix_ms,
            issuance,
        },
    )
    .map_err(|e| format!("STAGE_REFUSED: {e}"))?;

    match outcome {
        PlacementToStagingOutcome::NoEligible { eligibility } => {
            // ★ 상태를 바꾸지 않는다. 왜 떨어졌는지 노드별로 말한다.
            let mut lines = vec![format!(
                "STAGE_REFUSED: 적격 노드가 없다 — {} 개 후보를 봤다",
                eligibility.rejected.len()
            )];
            for rejected in &eligibility.rejected {
                lines.push(format!(
                    "  {} — {}",
                    rejected.node_id,
                    rejected
                        .reasons
                        .iter()
                        .map(|r| format!("{r:?}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            Err(lines.join("\n"))
        }
        PlacementToStagingOutcome::Staged {
            eligibility,
            selected_node_id,
            selected_gpu_ids,
            stage,
            ..
        } => Ok(format!(
            "STAGED job_id={job_id} node={selected_node_id} gpus=[{}] attempt={} lease={} fence={} eligible={}",
            selected_gpu_ids.join(","),
            stage.attempt.attempt_id,
            stage.lease.lease_id,
            stage.lease.fence_epoch,
            eligibility.eligible.len()
        )),
    }
}

/// 자원 축 우선순위를 이름 목록으로 받는다.
///
/// ★ **기본값이 없다** — 기준 계획서에 v0.1 자원 축의 고정 우선순위가
///   없다고 `model.rs` 가 명시했다. 여기서 하나를 골라 기본으로 두면
///   규범에 없는 정책을 발명하는 것이다.
///
/// 다섯 축을 중복 없이 전부 나열해야 한다.
fn parse_axes(raw: &str) -> Result<BestFitPolicy, String> {
    let names: Vec<&str> = raw.split(',').map(str::trim).collect();
    if names.len() != 5 {
        return Err(format!(
            "--best-fit-axes 는 다섯 축을 전부 나열해야 한다(받은 개수 {}) — vram,gpu_count,cpu,ram,workspace",
            names.len()
        ));
    }
    let mut axes = Vec::with_capacity(5);
    for name in &names {
        let axis = match name.to_ascii_lowercase().as_str() {
            "vram" => FitAxis::Vram,
            "gpu_count" => FitAxis::GpuCount,
            "cpu" => FitAxis::Cpu,
            "ram" => FitAxis::Ram,
            "workspace" => FitAxis::Workspace,
            other => {
                return Err(format!(
                    "--best-fit-axes 에 모르는 축 {other:?} — vram, gpu_count, cpu, ram, workspace"
                ))
            }
        };
        if axes.contains(&axis) {
            return Err(format!("--best-fit-axes 에 {name:?} 가 두 번 나온다"));
        }
        axes.push(axis);
    }
    Ok(BestFitPolicy {
        axis_order: [axes[0], axes[1], axes[2], axes[3], axes[4]],
    })
}

fn parse_key16(hex: &str) -> Result<[u8; 16], String> {
    if hex.len() != 32 {
        return Err(format!(
            "--operation-key 는 32자리 hex(16바이트)여야 한다(길이 {})",
            hex.len()
        ));
    }
    // ★ 결함 ⑬(2026-09-10) — 바이트로 자르지 않는다. `gputeer_crypto::hex`
    //   가 한 바이트씩 읽으므로 문자 경계를 가를 수 없다.
    gputeer_crypto::hex::decode_fixed::<16>(hex)
        .map_err(|e| format!("--operation-key hex 파싱 실패: {e}"))
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as u64
}

fn u64_flag(flags: &BTreeMap<String, String>, key: &str) -> Result<u64, String> {
    require(flags, key)?
        .parse::<u64>()
        .map_err(|e| format!("{key} 파싱 실패: {e}"))
}

fn parse_flags(args: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < args.len() {
        let key = &args[i];
        if !key.starts_with("--") {
            return Err(format!("알 수 없는 인자: {key}"));
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("{key} 에 값이 없다"))?;
        out.insert(key.clone(), value.clone());
        i += 2;
    }
    Ok(out)
}

fn require<'a>(flags: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    flags
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| format!("{key} 가 필요하다"))
}
