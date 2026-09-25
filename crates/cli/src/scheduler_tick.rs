//! ★★ 2026-09-10 독립 검수 지적 — **일부** 거부 사유에 안정적인 코드를 붙였다.
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
//! 코드가 붙은 분기는 두 무리로 가른다:
//! ```text
//! TICK_ARGS_REFUSED: <CODE>   인자·설정 검사 (CONTROL_DB_NOT_DURABLE 만 저장소를 연 뒤)
//! TICK_REFUSED: <CODE>        실행 중 관문 (★ QUEUE_TOO_OLD 는 결함 211 로 없어졌다)
//! ```
//! ★ **코드 없는 오류도 낸다**(재검수 30 — 전에는 위 두 줄이 오류 전체를 가르는
//!   것처럼 적었다). 필수 인자 누락(`--control-db 가 필요하다`) · 숫자 파싱 실패 ·
//!   값 누락에는 접두어가 없고, `TICK_REFUSED:` 뒤에 코드 대신 `job_id`·설명이
//!   오는 줄도 있다.
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
//! 그래서 단발 tick 을 먼저 검증하고, 반복·실패 처리·중단 정책은 따로
//! 설계하려 한다(재검수 26 — 전에는 "루프는 얇은 껍데기가 된다 · 반대로 하면
//! 결함을 가린다" 고 구현 순서의 효과를 확정했다).
//!
//! # ★★ 식별자를 **저장된 사실에서 유도한다**
//!
//! `stage-job` 은 운영자가 `--attempt-id`·`--lease-id`·`--operation-key`
//! 와 Lease 시각 셋을 직접 준다. 사람이 부르면 그게 맞다 — 지어낼 수
//! 없는 것을 지어내지 않는다.
//!
//! 그런데 **스스로 도는 것은 그 값을 어디선가 만들어야 한다.** 재시도마다
//! 시계나 난수로 새로 만들면 같은 Job·계획의 재시도가 같은 operation key 를
//! 갖는다고 보장할 수 없다(재검수 30 — 같은 밀리초 안의 두 조회는 같은 값일 수
//! 있다. 매번 달라지는 것이 아니라 재현이 보장되지 않는 것이다). 처음 만든 값을
//! 저장해 재사용하는 설계도 가능하지만, 이 경로는 이미 저장된 두 값에서
//! 유도하는 쪽을 택했다(재검수 28 — 전에는 저장소의 멱등이 "거짓말" 이
//! 된다고 적었다. 호출자의 키 생성 문제이지 저장소 보장의 실패가 아니다).
//!
//! 그래서 식별자 셋은 `(job_id, plan_id)` 에서, Lease 시각은 저장된 `queued_at` 과
//! 운영자 오프셋에서 유도한다(재검수 26 — 전에는 "전부 (job_id, plan_id) 에서" 라 적었다):
//!
//! ```text
//! attempt_id      BLAKE3("attempt", job_id, plan_id) -> 26자
//! lease_id        BLAKE3("lease",   job_id, plan_id) -> 26자
//! operation_key   BLAKE3("operation", job_id, plan_id)[..16]
//! Lease 발급 시각  max(job.queued_at_unix_ms, 지금)   ★ 결함 211 — 아래 참조
//! Lease 갱신·만료  발급 시각 + 운영자가 준 오프셋
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
//!   `fenced_operation.rs` 가 길이 26 을 요구하므로 모양을 맞췄다. 시간 정렬이
//!   필요해지면 정렬 기준과 식별자 생성 방식을 그때 따로 설계한다(재검수 28 —
//!   전에는 "진짜 ULID 를 발급해야 한다" 고 구현을 정해 적었다).
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
    // ★ 2026-09-23 (신뢰망 남은 일 C) — 생존 축. 이만큼 소식이 없는 노드에는 새 일을 주지 않는다.
    //   **주지 않으면 이 축을 보지 않는다** — "정책 없음" 을 0ms 로 읽으면 모든 노드가 탈락한다(2026-09-23 P2).
    //   소식은 풀 Coordinator 가 적는 검증된 Hello 시각이다(`--liveness-db` 가 control DB 와 같을 때 보인다).
    let silent_after_ms = match flags.get("--silent-after-ms") {
        Some(raw) => Some(raw.parse::<u64>().map_err(|e| {
            format!(
                "TICK_ARGS_REFUSED: SILENT_AFTER_NOT_A_NUMBER — --silent-after-ms 파싱 실패: {e}"
            )
        })?),
        None => None,
    };
    if lease_renew_after_ms == 0 || lease_renew_after_ms >= lease_ttl_ms {
        return Err(format!(
            "TICK_ARGS_REFUSED: RENEW_AFTER_NOT_BEFORE_TTL — --lease-renew-after-ms({lease_renew_after_ms}) 는 0 보다 크고 --lease-ttl-ms({lease_ttl_ms}) 보다 작아야 한다. 갱신 시점이 만료 뒤면 갱신할 기회가 없다"
        ));
    }

    // ★ 신선도 판정에 쓰는 "지금". durable 기록으로는 안 쓴다.
    let now_unix_ms = now_unix_ms();

    let mut jobs = CoordinatorJobStore::open(control_db)
        .map_err(|e| format!("job store 를 열지 못했다({control_db}): {e}"))?;
    if !jobs.is_durable() {
        return Err(format!(
            "TICK_ARGS_REFUSED: CONTROL_DB_NOT_DURABLE — --control-db 가 영속이 아니다({control_db:?}). 예약이 프로세스와 함께 사라진다"
        ));
    }

    // ── 장애 이어받기 먼저 ─────────────────────────────────────────
    //
    // ★ 2026-09-23 (신뢰망 남은 일 G) — `--failover-grace-ms` 를 주면 **큐를 보기 전에** 끊긴 시도의 Job 을 규범 경로로
    //   되돌린다(`gputeer_coordinator::failover`). 되돌아온 Job 은 곧바로 이 tick 의 큐 후보가 된다.
    //   주지 않으면 하지 않는다 — 유예 시간을 지어내지 않는다.
    //   결과 줄은 바로 찍는다(표준 출력) — tick 결과가 거부여도 되돌린 사실은 남아야 한다.
    if let Some(raw) = flags.get("--failover-grace-ms") {
        let grace_ms = raw.parse::<u64>().map_err(|e| {
            format!("TICK_ARGS_REFUSED: FAILOVER_GRACE_NOT_A_NUMBER — --failover-grace-ms 파싱 실패: {e}")
        })?;
        let producer_keys = match flags.get("--pool-agents") {
            Some(raw) => parse_pool_agents(raw)?,
            None => Vec::new(),
        };
        let policy = gputeer_coordinator::failover::FailoverPolicy {
            grace_ms,
            shared_checkpoint_root: flags
                .get("--shared-checkpoint-root")
                .map(std::path::PathBuf::from),
            producer_keys,
        };
        let mut notes = Vec::new();
        let outcomes = gputeer_coordinator::failover::failover_lost_attempts(
            std::path::Path::new(control_db),
            &policy,
            now_unix_ms,
            &mut notes,
        )
        .map_err(|e| format!("TICK_REFUSED: FAILOVER_STORE — 장애 판정 중 저장소 오류: {e}"))?;
        for note in notes {
            println!("{note}");
        }
        for outcome in outcomes {
            println!("{}", outcome.line());
        }
    }

    // ── 큐의 맨 앞 하나 ─────────────────────────────────────────────
    //
    // `list_queued()` 는 `(queued_at, job_id)` 순의 결정적 FIFO 다.
    // ★ 2026-09-23 (신뢰망 남은 일 H) — 선점으로 멈춘 PAUSED Job 도 배치한다(다른 노드에서 RESUMED).
    let queued = jobs
        .list_schedulable()
        .map_err(|e| format!("큐 조회 실패: {e}"))?;
    // ── 제출자 keyring(Manifest 재검증용) ───────────────────────────
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

    // ★ 2026-09-24 (결함 235 · 236 · 재검수 79) — 풀 여부는 **풀 Coordinator 가 control DB 에 적은 표식**으로 안다. 전에는
    //   `--pool-agents` 유무로 짐작해, 실제 풀에서 그 인자를 빠뜨리면 거부가 빠지고(217 재현), 풀 밖에서 이어받기 키로 주면
    //   만족 가능한 작업까지 영구 FAILED 가 됐다. 그리고 비-LOCAL 작업은 **건너뛰고** 다음 작업을 본다 — QUEUED 는 규범
    //   `QUEUED -> FAILED`(PERMANENTLY_INFEASIBLE)로 내리고, PAUSED 는 규범상 여기서 내릴 전이가 없어(PAUSE_TIMEOUT · USER_CANCELLED 뿐)
    //   건너뛰기만 한다 — 전에는 PAUSED 가 FIFO 맨 앞을 영구히 막았다.
    // ★ 결함 249 (재검수 82) — 표식은 Coordinator 가 처음 뜰 때 적는다. 그 전에 스케줄러가 돌면 표식이 없어 비-LOCAL 이 배치됐다.
    //   그래서 풀 운영의 스케줄러는 `--pool-mode true` 를 **명시**한다(짐작이 아니다) — 주면 표식을 적고 풀로 본다.
    match flags.get("--pool-mode").map(String::as_str) {
        None | Some("true") | Some("false") => {}
        Some(other) => {
            return Err(format!(
                "TICK_ARGS_REFUSED: POOL_MODE_NOT_A_BOOL — --pool-mode 는 true · false 다(받은 값 {other:?})"
            ))
        }
    }
    if flags.get("--pool-mode").map(String::as_str) == Some("true") {
        gputeer_coordinator::job_store::declare_pool_mode(
            std::path::Path::new(control_db),
            now_unix_ms,
        )
        .map_err(|e| format!("TICK_REFUSED: 풀 모드 표식을 적지 못했다: {e}"))?;
    }
    let pool_declared =
        gputeer_coordinator::job_store::pool_mode_declared(std::path::Path::new(control_db))
            .map_err(|e| format!("TICK_REFUSED: 풀 모드 표식을 읽지 못했다: {e}"))?;
    let queue_was_empty = queued.is_empty();
    let mut chosen = None;
    for candidate in queued {
        // ★ 2026-09-25 (결함 423 · 재검수 107) — 저장된 Manifest 를 **고르기 전에** 검증한다. 전에는 맨 앞을 고른 뒤 검증해, 만료된 Manifest 하나가
        //   tick 을 매번 거부로 끝내고 뒤의 모든 Job 을 인질로 잡았다(결함 211 · 236 과 같은 모양).
        //   만료는 되돌릴 수 없어 큐에서 내린다(제출자가 다시 서명해 새로 낸다). 그 밖의 실패(모르는 서명자 등)는 keyring 을 고치면 풀릴 수 있어
        //   내리지 않고 건너뛴다. 저장된 Manifest 가 없는 Job 도 건너뛴다.
        // ★ 결함 432 (재검수 110) — 여기서 읽은 Manifest 를 **들고 간다.** 전에는 풀 내구성 검사 · 예약 직전에 DB 를 두 번 더 읽었고, 그 재조회에는
        //   이 분류가 없었다(첫 검사 뒤 행이 사라지면 루프가 멈췄다).
        let manifest = match stored_manifest_check(&jobs, &candidate.job_id, &keyring, now_unix_ms)?
        {
            ManifestCheck::Valid(manifest) => manifest,
            ManifestCheck::Expired => {
                if candidate.state == gputeer_coordinator::job_store::JobState::Queued {
                    jobs.fail_queued(
                        &candidate.job_id,
                        gputeer_coordinator::job_store::QueueFailure::PermanentlyInfeasible {
                            reason: "MANIFEST_EXPIRED — 제출 Manifest 가 만료됐다. 제출자가 다시 서명해 새로 낸다".to_string(),
                        },
                        now_unix_ms,
                    )
                    .map_err(|e| {
                        format!("TICK_REFUSED: {} 를 큐에서 내리지 못했다: {e}", candidate.job_id)
                    })?;
                    println!("TICK_JOB_FAILED_MANIFEST_EXPIRED {}", candidate.job_id);
                } else {
                    println!(
                        "TICK_SKIPPED {} state={:?} — 제출 Manifest 가 만료됐다. 규범상 여기서 내릴 전이가 없어 건너뛴다(운영자가 취소한다)",
                        candidate.job_id, candidate.state
                    );
                }
                continue;
            }
            ManifestCheck::Unusable(reason) => {
                println!(
                    "TICK_SKIPPED {} — {reason}. 배치하지 않고 다음 작업을 본다",
                    candidate.job_id
                );
                continue;
            }
        };
        if pool_declared {
            if let Some(reason) =
                pool_unsupported_durability(&candidate.job_id, &manifest, &keyring, now_unix_ms)?
            {
                if candidate.state == gputeer_coordinator::job_store::JobState::Queued {
                    jobs.fail_queued(
                        &candidate.job_id,
                        gputeer_coordinator::job_store::QueueFailure::PermanentlyInfeasible {
                            reason: reason.clone(),
                        },
                        now_unix_ms,
                    )
                    .map_err(|e| {
                        format!(
                            "TICK_REFUSED: {} 를 큐에서 내리지 못했다: {e}",
                            candidate.job_id
                        )
                    })?;
                    println!(
                        "TICK_JOB_FAILED_PERMANENTLY_INFEASIBLE {} — {reason}",
                        candidate.job_id
                    );
                } else {
                    println!(
                        "TICK_SKIPPED {} state={:?} — {reason}. 규범상 여기서 내릴 전이가 없어 건너뛴다(운영자가 취소한다)",
                        candidate.job_id, candidate.state
                    );
                }
                continue;
            }
        }
        chosen = Some((candidate, manifest));
        break;
    }
    let Some((job, manifest)) = chosen else {
        // ★ 빈 큐는 **오류가 아니다.** 루프가 이걸 실패로 세면 정상
        //   유휴 상태가 장애로 보인다.
        return Ok(if queue_was_empty {
            "TICK_IDLE 큐가 비었다".to_string()
        } else {
            "TICK_IDLE 배치할 작업이 없다 — 남은 작업은 전부 이 풀이 채울 수 없는 내구성을 요구한다"
                .to_string()
        });
    };
    let job_id = job.job_id.clone();
    let plan_id = job
        .plan_id
        .clone()
        .ok_or_else(|| format!("TICK_REFUSED: {job_id} 가 QUEUED 인데 plan_id 가 없다"))?;
    let queued_at = job
        .queued_at_unix_ms
        .ok_or_else(|| format!("TICK_REFUSED: {job_id} 가 QUEUED 인데 queued_at 이 없다"))?;

    // ── Lease 시각 ──────────────────────────────────────────────────
    //
    // ★★ 결함 211 (2026-09-23) — 전에는 `queued_at` 을 발급 시각으로 쓰고, 큐에서 TTL 보다 오래 기다린 작업을
    //   `QUEUE_TOO_OLD` 로 거부했다. 그 작업은 **영영** 배치되지 않았고 FIFO 맨 앞이라 뒤 작업까지 인질이 됐다 —
    //   작업 수가 GPU 수보다 많은 신뢰망에서는 보통 상황이다. 시계를 안 읽는 이득(같은 tick 의 재시도가 같은
    //   요청을 만든다)은 실제로 없었다 — 예약된 Job 은 큐에서 빠져 두 번째 tick 이 다시 보지 않는다.
    //   그래서 발급 시각을 **지금**으로 둔다. 저장소의 "큐 진입보다 앞설 수 없다" 는 `max` 로 그대로 지킨다.
    //   ★ 동시에 도는 tick 둘은 발급 시각이 달라 같은 operation key 에 **다른 요청**이 된다 — 저장소가 둘째를
    //     `OperationConflict` 로 거부한다(중복 예약이 아니라 거부다).
    let issued_at = queued_at.max(now_unix_ms);
    let expires_at = issued_at.saturating_add(lease_ttl_ms);

    drop(jobs);

    // ── 저장된 Manifest 를 지금 다시 검증한다(keyring 은 위에서 열었다) ─────
    //   ★ 결함 432 — 고를 때 읽은 그 Manifest 다(DB 를 다시 읽지 않는다).
    let verified = verify(
        &manifest,
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

    // ★ 2026-09-23 (신뢰망 남은 일 G) — 이어받기로 되돌아온 Job 은 **새 시도**다. 같은 (job, plan) 에서 유도하면 옛 시도와
    //   식별자가 겹쳐 저장소가 옛 결과를 돌려준다(operation replay). 되돌아온 횟수로 가른다 — 0 이면 전과 같다.
    let generation = if job.requeue_count == 0 {
        plan_id.clone()
    } else {
        format!("{plan_id}#requeue-{}", job.requeue_count)
    };
    let attempt_id = derive_id("attempt", &job_id, &generation);
    let lease_id = derive_id("lease", &job_id, &generation);

    let outcome = orchestrate_placement_to_staging(
        &mut inventory,
        &mut staging,
        &PlacementToStagingInput {
            job_id: job_id.clone(),
            job_requirements,
            hard_filter_policy: Policy {
                maximum_snapshot_age_ms: max_snapshot_age_ms,
                silent_after_ms,
            },
            best_fit_policy,
            evaluated_at_unix_ms: now_unix_ms,
            issuance: StagingIssuanceInput {
                operation_key: derive_operation_key(&job_id, &generation),
                attempt_id: attempt_id.clone(),
                lease_id: lease_id.clone(),
                issuing_coordinator_id: coordinator_id.to_string(),
                coordinator_term,
                issued_at_unix_ms: issued_at,
                renew_after_unix_ms: issued_at.saturating_add(lease_renew_after_ms),
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
///   시간 정렬이 필요해지면 정렬 기준과 생성 방식을 따로 설계한다
///   (재검수 28 — 전에는 ULID 발급기와 이 함수 삭제를 정해 적었다).
///
/// ★ 바이트를 32 로 나눈 나머지로 고른다. 256 은 32 의 배수라 균등한 바이트
///   입력에 이 나머지 연산이 편향을 더하지 않는다(재검수 22 — 전에는 한 문장
///   안에서 "균등하지 않다" 와 "사실 균등하다" 를 같이 적었다). 이 값은
///   비밀이 아니고 추측 불가능성을 요구하지도 않는다 — 이 유도는 같은 입력에서
///   같은 식별자를 **재현하기 위해** 쓴다(재검수 26 — 전에는 "그것만 필요하다" 고
///   요구사항을 좁혔다. 계획별로 식별자를 가르는 용도는 모듈 주석에 있다).
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
            return Err(format!(
                "TICK_ARGS_REFUSED: AXES_DUPLICATE — --best-fit-axes 에 {name:?} 가 두 번 나온다"
            ));
        }
        axes.push(axis);
    }
    Ok(BestFitPolicy {
        axis_order: [axes[0], axes[1], axes[2], axes[3], axes[4]],
    })
}

/// `--pool-agents "id=hex;id2=hex"` — 체크포인트 생산자 서명을 검증할 풀 노드 키(풀 Coordinator 와 같은 형식).
/// 풀이 채울 수 없는 내구성인가 — 저장된 Manifest 를 **지금 다시 검증한 뒤** 읽는다(§0.2). 검증이 안 되면 여기서 거부하지 않는다
/// (아래 본 경로가 같은 검증으로 거부한다 — 사유를 한 곳에서 낸다).
/// 결함 423 — 후보를 고르기 전의 Manifest 검사 결과.
enum ManifestCheck {
    /// 지금 검증된다 — 읽은 Manifest 를 그대로 들고 간다(결함 432).
    Valid(gputeer_protocol::pb::JobManifest),
    /// 만료 — 되돌릴 수 없다.
    Expired,
    /// 그 밖의 이유로 지금은 쓸 수 없다(저장된 Manifest 없음 · 모르는 서명자 · 서명 불일치 등).
    Unusable(String),
}

fn stored_manifest_check(
    jobs: &CoordinatorJobStore,
    job_id: &str,
    keyring: &PersistentKeyring,
    now_unix_ms: u64,
) -> Result<ManifestCheck, String> {
    // ★ 결함 427 (재검수 108) — 옛 Job(본문 행 없음 · `LegacyManifestMissing`)과 본문 손상(`CorruptData`)은 **이 Job 을 쓸 수 없다** 는 뜻이라
    //   건너뛴다. 처음엔 `?` 로 올려 tick 이 끝났고, 오류가 `TICK_REFUSED` 로 시작하지 않아 scheduler-loop 가 멈췄다. 그 밖의 저장소 장애(잠금 · I/O)는
    //   거부한다(fail-closed).
    let binding = match jobs.get_manifest_binding(job_id) {
        Ok(Some(binding)) => binding,
        Ok(None) => {
            return Ok(ManifestCheck::Unusable(
                "저장된 Manifest 가 없다".to_string(),
            ))
        }
        // ★ 결함 429 · 430 (재검수 109) — Manifest **전용** 오류만 건너뛴다(본문 손상은 `ManifestCorrupt` 등으로 온다 — `CorruptData` 가 아니다).
        //   그 밖의 저장소 장애(`Io` · `LockTimeout` · Job 행 손상 `CorruptData`)는 접두어 없이 올려 scheduler-loop 를 멈춘다(fail-closed) —
        //   `TICK_REFUSED` 로 포장하면 루프가 헛돈다.
        Err(
            e @ (gputeer_coordinator::job_store::JobStoreError::LegacyManifestMissing { .. }
            | gputeer_coordinator::job_store::JobStoreError::ManifestCorrupt { .. }
            | gputeer_coordinator::job_store::JobStoreError::ManifestHashMismatch
            | gputeer_coordinator::job_store::JobStoreError::ManifestIdentityMismatch(_)),
        ) => {
            return Ok(ManifestCheck::Unusable(format!(
                "저장된 Manifest 를 쓸 수 없다: {e}"
            )))
        }
        Err(e) => return Err(format!("STORAGE_FAILED: Manifest binding 조회 실패: {e}")),
    };
    Ok(
        match verify(
            &binding.manifest,
            1,
            &Ed25519Verifier::new(keyring),
            now_unix_ms,
            &mut NoReplayCheck,
        ) {
            Ok(_) => ManifestCheck::Valid(binding.manifest.clone()),
            Err(gputeer_protocol::signing::VerifyError::Outcome(
                gputeer_protocol::signing::VerifyOutcome::Expired,
            )) => ManifestCheck::Expired,
            Err(e) => {
                ManifestCheck::Unusable(format!("저장된 Manifest 를 지금 검증하지 못했다: {e:?}"))
            }
        },
    )
}

fn pool_unsupported_durability(
    job_id: &str,
    manifest: &gputeer_protocol::pb::JobManifest,
    keyring: &PersistentKeyring,
    now_unix_ms: u64,
) -> Result<Option<String>, String> {
    let Ok(verified) = verify(
        manifest,
        1,
        &Ed25519Verifier::new(keyring),
        now_unix_ms,
        &mut NoReplayCheck,
    ) else {
        return Ok(None);
    };
    let durability = gputeer_protocol::pb::Durability::try_from(verified.get().durability)
        .map_err(|_| format!("TICK_REFUSED: {job_id} 의 durability 값을 모른다"))?;
    Ok(
        (durability != gputeer_protocol::pb::Durability::Local).then(|| {
            format!(
                "{}_NOT_SUPPORTED_BY_POOL — 이 풀은 복제하지 않는다(LOCAL 만)",
                durability.as_str_name()
            )
        }),
    )
}

fn parse_pool_agents(raw: &str) -> Result<Vec<(String, gputeer_crypto::VerifyingKey)>, String> {
    let mut keys = Vec::new();
    for entry in raw
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let (id, hex) = entry.split_once('=').ok_or_else(|| {
            format!("TICK_ARGS_REFUSED: POOL_AGENTS_FORMAT — {entry:?} 는 id=공개키hex 가 아니다")
        })?;
        let hex = hex.trim();
        if hex.len() != 64 {
            return Err(format!(
                "TICK_ARGS_REFUSED: POOL_AGENTS_FORMAT — {id} 의 공개키가 32바이트 hex 가 아니다"
            ));
        }
        let mut bytes = [0u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).map_err(|_| {
                format!("TICK_ARGS_REFUSED: POOL_AGENTS_FORMAT — {id} 의 공개키가 hex 가 아니다")
            })?;
        }
        let key = gputeer_crypto::VerifyingKey::from_bytes(&bytes).map_err(|e| {
            format!("TICK_ARGS_REFUSED: POOL_AGENTS_FORMAT — {id} 의 공개키가 유효하지 않다: {e}")
        })?;
        keys.push((id.trim().to_string(), key));
    }
    Ok(keys)
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
            return Err(format!(
                "TICK_ARGS_REFUSED: UNKNOWN_FLAG — 알 수 없는 인자: {key}"
            ));
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
