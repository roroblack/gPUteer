//! ★★ 2026-09-10 독립 검수 지적 — 거부 사유에 **안정적인 코드**를 붙였다.
//!
//! 그전에는 인자 검사 오류가 사용자 입력을 그대로 메시지에 넣었다:
//!
//! ```text
//! --best-fit-axes 에 모르는 축 "already reserved" — vram, gpu_count, ...
//! ```
//!
//! 그래서 `--best-fit-axes "already reserved,..."` 를 주면 **축 파서에서
//! 죽으면서도** 테스트의 `output.contains("already reserved")` 를 통과했다.
//! 즉 테스트가 **엉뚱한 관문을 재고 있어도 초록이었다.**
//!
//! 이제 두 무리를 코드로 가른다:
//! ```text
//! TICK_ARGS_REFUSED: <CODE>   인자·설정 검사 (DB 를 열기 전)
//! TICK_REFUSED: <CODE>        실행 중 관문
//! ```
//! 테스트는 `.contains()` 가 아니라 **오류 줄의 시작**으로 확인한다
//! (`tests/scheduler_tick.rs` 의 `refused_with()`).
//! `gputeer scheduler-tick` — 큐에서 **한 건**을 꺼내 예약까지 진행한다.
//!
//! ```text
//! plan-job        운영자가 Job 하나를 큐에 올린다              (QUEUED)
//! scheduler-tick  큐를 보고 스스로 골라 예약한다               (STAGING)
//! ```
//!
//! # ★ 왜 "데몬" 이 아니라 "한 번(tick)" 인가
//!
//! 루프는 쉽고 **틀리기도 쉽다** — 실패를 어떻게 다룰지, 얼마나 자주
//! 돌지, 언제 멈출지가 전부 정책이고 이 저장소에 그 규범이 없다.
//! 한 번 도는 것부터 정직하게 만들면, 루프는 그것을 반복하는 얇은
//! 껍데기가 된다. 반대로 하면 껍데기의 정책이 알맹이의 결함을 가린다.
//!
//! # ★★ 식별자를 **저장된 사실에서 유도한다**
//!
//! `stage-job` 은 운영자가 `--attempt-id`·`--lease-id`·`--operation-key`
//! 와 Lease 시각 셋을 직접 준다. 사람이 부르면 그게 맞다 — 지어낼 수
//! 없는 것을 지어내지 않는다.
//!
//! 그런데 **스스로 도는 것은 그 값을 어디선가 만들어야 한다.** 시계나
//! 난수로 만들면 재시도마다 달라지고, 그러면 저장소의 operation key 가
//! 약속하는 멱등이 **거짓말**이 된다(`stage-job` 에서 통합 테스트가
//! 정확히 그걸 잡았다).
//!
//! 그래서 전부 `(job_id, plan_id)` 에서 유도한다:
//!
//! ```text
//! attempt_id      BLAKE3("attempt", job_id, plan_id) -> 26자
//! lease_id        BLAKE3("lease",   job_id, plan_id) -> 26자
//! operation_key   BLAKE3("operation", job_id, plan_id)[..16]
//! Lease 발급 시각  job.queued_at_unix_ms  (저장된 사실)
//! Lease 갱신·만료  queued_at + 운영자가 준 오프셋
//! ```
//!
//! 같은 Job·계획에서는 식별자와 operation key 가 같게 유도된다. 요청에는 현재
//! 시각(`evaluated_at_unix_ms`)과 운영자 설정도 들어가므로 **요청 전체가 같다고는
//! 하지 않는다** — 저장소가 최초 결과를 돌려주는 조건은 저장소의 동일성 규칙을
//! 따른다(재검수 24 — 전에는 "바이트까지 같은 요청" 이라 적었다).
//!
//! ★ **`plan_id` 가 오늘 무엇을 막는지 확인하지 못했다.** 처음에 쓴
//!   설명이 과장이었고, 그것을 고친 두 번째 설명도 반대쪽으로
//!   과장이었다(2026-09-10 재검수 21 — "짐을 지고 있지 않다" 고 적었다).
//!
//!   원래 이렇게 적었다: "inventory 가 바뀌어 계획이 달라지면 다른
//!   Attempt 가 나오므로 낡은 계획의 예약을 재사용하지 않는다."
//!   그 상황을 **테스트로 만들려다 못 만들었다** — 저장소가 더 앞에서
//!   막는다:
//!
//!   ```text
//!   PLAN_REFUSED: queued Job plan conflict: stored=e4b3f8a4..., requested=18b096ec...
//!   ```
//!
//!   관측한 것은 둘이다 — 저장소가 QUEUED Job 의 계획 변경을 위처럼
//!   거부한다는 것, 그리고 `plan_id` 를 유도에서 통째로 빼도 테스트가
//!   하나도 안 깨진다는 것(**숨기지 않고 적는다**). 그래서 **시도한
//!   경로에서는** 그 영향을 확인하지 못했다. 재계획·재큐잉 같은 다른
//!   경로에서 `plan_id` 가 역할을 하는지는 **판정하지 않았다.**
//!
//!   그래도 남겨 두는 이유: 저장소가 언젠가 재계획을 허용하면 계획별로
//!   식별자를 가르는 데 쓰일 **가능성**이 있어서다. 실제 방어 기여는 그
//!   경로에서 검증해야 한다(재검수 22 — 전에는 "그때는 짐을 지게 된다" 고
//!   확정했다).
//!   다만 **지금 무언가를 막는다고도, 안 막는다고도 쓰지 않는다.**
//!
//! ★ 26자는 **ULID 모양일 뿐 ULID 가 아니다.** 시간 순서가 없다.
//!   `fenced_operation.rs` 가 길이 26 을 요구하므로 모양을 맞췄고,
//!   시간 정렬이 필요해지면 그때 진짜 ULID 를 발급해야 한다.
//!
//! # 이 명령이 하지 않는 것
//!
//! ```text
//! 안 한다   반복            한 번 돈다. 루프는 별도 조각이다
//! 안 한다   여러 건 처리    큐의 **맨 앞 하나**만 본다
//! 안 한다   Grant 발급·전송  issue-grant / coordinator-stub 의 일이다
//! 안 한다   실패한 Job 정리  QUEUED->FAILED 판정은 별도 경로다
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

/// `gputeer scheduler-tick` 진입점.
pub fn run(args: &[String]) -> Result<String, String> {
    let flags = parse_flags(args)?;

    let control_db = require(&flags, "--control-db")?;
    let keyring_path = require(&flags, "--submitter-keyring")?;
    let submitter_member = require(&flags, "--submitter-member")?;
    let max_snapshot_age_ms = u64_flag(&flags, "--max-snapshot-age-ms")?;
    let best_fit_policy = parse_axes(require(&flags, "--best-fit-axes")?)?;
    let coordinator_id = require(&flags, "--coordinator-id")?;
    let coordinator_term = u64_flag(&flags, "--coordinator-term")?;
    let lease_ttl_ms = u64_flag(&flags, "--lease-ttl-ms")?;
    let lease_renew_after_ms = u64_flag(&flags, "--lease-renew-after-ms")?;
    let max_total_duration_seconds = u64_flag(&flags, "--lease-max-total-duration-seconds")?;
    if lease_renew_after_ms == 0 || lease_renew_after_ms >= lease_ttl_ms {
        return Err(format!(
            "TICK_ARGS_REFUSED: RENEW_AFTER_NOT_BEFORE_TTL — --lease-renew-after-ms({lease_renew_after_ms}) 는 0 보다 크고 --lease-ttl-ms({lease_ttl_ms}) 보다 작아야 한다. 갱신 시점이 만료 뒤면 갱신할 기회가 없다"
        ));
    }

    // ★ 신선도 판정에 쓰는 "지금". durable 기록으로는 안 쓴다.
    let now_unix_ms = now_unix_ms();

    let jobs = CoordinatorJobStore::open(control_db)
        .map_err(|e| format!("job store 를 열지 못했다({control_db}): {e}"))?;
    if !jobs.is_durable() {
        return Err(format!(
            "TICK_ARGS_REFUSED: CONTROL_DB_NOT_DURABLE — --control-db 가 영속이 아니다({control_db:?}). 예약이 프로세스와 함께 사라진다"
        ));
    }

    // ── 큐의 맨 앞 하나 ─────────────────────────────────────────────
    //
    // `list_queued()` 는 `(queued_at, job_id)` 순의 결정적 FIFO 다.
    let queued = jobs
        .list_queued()
        .map_err(|e| format!("큐 조회 실패: {e}"))?;
    let Some(job) = queued.into_iter().next() else {
        // ★ 빈 큐는 **오류가 아니다.** 루프가 이걸 실패로 세면 정상
        //   유휴 상태가 장애로 보인다.
        return Ok("TICK_IDLE 큐가 비었다".to_string());
    };
    let job_id = job.job_id.clone();
    let plan_id = job
        .plan_id
        .clone()
        .ok_or_else(|| format!("TICK_REFUSED: {job_id} 가 QUEUED 인데 plan_id 가 없다"))?;
    let queued_at = job.queued_at_unix_ms.ok_or_else(|| {
        format!("TICK_REFUSED: {job_id} 가 QUEUED 인데 queued_at 이 없다")
    })?;

    // ── Lease 시각을 저장된 사실에서 유도한다 ───────────────────────
    //
    // ★ `queued_at` 을 발급 시각으로 쓴다. 저장소가 "Lease 발급 시각은
    //   큐 진입보다 앞설 수 없다" 고 요구하는데, **같은 값이면 그
    //   경계를 정확히 만족**하면서도 시계를 안 읽는다.
    let expires_at = queued_at.saturating_add(lease_ttl_ms);
    if expires_at <= now_unix_ms {
        return Err(format!(
            "TICK_REFUSED: QUEUE_TOO_OLD — {job_id} 는 큐에 너무 오래 있었다(큐 진입 {queued_at}, 이 설정의 Lease 만료 {expires_at}, 지금 {now_unix_ms}). 지금 예약하면 이미 만료된 Lease 를 준다"
        ));
    }

    let binding = jobs
        .get_manifest_binding(&job_id)
        .map_err(|e| format!("Manifest binding 조회 실패: {e}"))?
        .ok_or_else(|| format!("TICK_REFUSED: {job_id} 에 저장된 Manifest 가 없다"))?;
    drop(jobs);

    // ── 저장된 Manifest 를 지금 다시 검증한다 ───────────────────────
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
    .map_err(|e| format!("TICK_REFUSED: 저장된 Manifest 를 지금 다시 검증하지 못했다: {e:?}"))?;
    let job_requirements = job_requirements_from_manifest(&verified, submitter_member)
        .map_err(|e| format!("TICK_REFUSED: {e}"))?;

    // ── 예약 ────────────────────────────────────────────────────────
    let mut inventory = CoordinatorInventoryStore::open(control_db)
        .map_err(|e| format!("inventory store 를 열지 못했다({control_db}): {e}"))?;
    let mut staging = CoordinatorStagingStore::open(control_db)
        .map_err(|e| format!("staging store 를 열지 못했다({control_db}): {e}"))?;

    let attempt_id = derive_id("attempt", &job_id, &plan_id);
    let lease_id = derive_id("lease", &job_id, &plan_id);

    let outcome = orchestrate_placement_to_staging(
        &mut inventory,
        &mut staging,
        &PlacementToStagingInput {
            job_id: job_id.clone(),
            job_requirements,
            hard_filter_policy: Policy {
                maximum_snapshot_age_ms: max_snapshot_age_ms,
            },
            best_fit_policy,
            evaluated_at_unix_ms: now_unix_ms,
            issuance: StagingIssuanceInput {
                operation_key: derive_operation_key(&job_id, &plan_id),
                attempt_id: attempt_id.clone(),
                lease_id: lease_id.clone(),
                issuing_coordinator_id: coordinator_id.to_string(),
                coordinator_term,
                issued_at_unix_ms: queued_at,
                renew_after_unix_ms: queued_at.saturating_add(lease_renew_after_ms),
                expires_at_unix_ms: expires_at,
                max_total_duration_seconds,
            },
        },
    )
    .map_err(|e| format!("TICK_REFUSED: {e}"))?;

    match outcome {
        PlacementToStagingOutcome::NoEligible { eligibility } => {
            let mut lines = vec![format!(
                "TICK_REFUSED: {job_id} 에 맞는 노드가 없다 — {} 개 후보를 봤다",
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
            selected_node_id,
            selected_gpu_ids,
            stage,
            ..
        } => Ok(format!(
            "TICK_STAGED job_id={job_id} plan={plan_id} node={selected_node_id} gpus=[{}] attempt={} lease={} fence={} created={}",
            selected_gpu_ids.join(","),
            stage.attempt.attempt_id,
            stage.lease.lease_id,
            stage.lease.fence_epoch,
            stage.created
        )),
    }
}

/// Crockford base32 — ULID 가 쓰는 알파벳이다(`I`·`L`·`O`·`U` 없음).
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// `(kind, job_id, plan_id)` 에서 **26자 식별자**를 결정적으로 만든다.
///
/// ★ **ULID 모양일 뿐 ULID 가 아니다** — 시간 순서가 없다.
///   `fenced_operation.rs` 가 길이 26 을 요구해서 모양을 맞췄다.
///   시간 정렬이 실제로 필요해지면 진짜 ULID 발급기가 있어야 하고,
///   그때는 이 함수를 지워야 한다.
///
/// ★ 바이트를 32 로 나눈 나머지로 고른다. 256 은 32 의 배수라 균등한 바이트
///   입력에 이 나머지 연산이 편향을 더하지 않는다(재검수 22 — 전에는 한 문장
///   안에서 "균등하지 않다" 와 "사실 균등하다" 를 같이 적었다). 이 값은
///   비밀이 아니고 추측 불가능성을 요구하지도 않는다 — 같은 입력에
///   같은 값이 나오는 것만 필요하다.
fn derive_id(kind: &str, job_id: &str, plan_id: &str) -> String {
    let digest = derive_digest(kind, job_id, plan_id);
    digest[..26]
        .iter()
        .map(|b| CROCKFORD[(*b % 32) as usize] as char)
        .collect()
}

fn derive_operation_key(job_id: &str, plan_id: &str) -> [u8; 16] {
    let digest = derive_digest("operation", job_id, plan_id);
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest[..16]);
    out
}

/// 길이 프리픽스를 붙여 인접 값이 서로 스며들지 않게 한다.
fn derive_digest(kind: &str, job_id: &str, plan_id: &str) -> [u8; 32] {
    let mut input = Vec::new();
    for part in [
        b"gputeer/v1/scheduler-tick".as_slice(),
        kind.as_bytes(),
        job_id.as_bytes(),
        plan_id.as_bytes(),
    ] {
        input.extend_from_slice(&(part.len() as u64).to_le_bytes());
        input.extend_from_slice(part);
    }
    gputeer_protocol::canonical::blake3_256(&input)
}

/// `stage-job` 과 같은 규칙 — 다섯 축을 중복 없이 전부 나열해야 한다.
fn parse_axes(raw: &str) -> Result<BestFitPolicy, String> {
    let names: Vec<&str> = raw.split(',').map(str::trim).collect();
    if names.len() != 5 {
        return Err(format!(
            "TICK_ARGS_REFUSED: AXES_COUNT — --best-fit-axes 는 다섯 축을 전부 나열해야 한다(받은 개수 {}). vram,gpu_count,cpu,ram,workspace",
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
                    "TICK_ARGS_REFUSED: AXES_UNKNOWN — --best-fit-axes 에 모르는 축 {other:?}. vram, gpu_count, cpu, ram, workspace"
                ))
            }
        };
        if axes.contains(&axis) {
            return Err(format!("TICK_ARGS_REFUSED: AXES_DUPLICATE — --best-fit-axes 에 {name:?} 가 두 번 나온다"));
        }
        axes.push(axis);
    }
    Ok(BestFitPolicy {
        axis_order: [axes[0], axes[1], axes[2], axes[3], axes[4]],
    })
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
            return Err(format!("TICK_ARGS_REFUSED: UNKNOWN_FLAG — 알 수 없는 인자: {key}"));
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
