//! 장애 이어받기 — 끊긴 노드의 작업을 **규범 경로로** 되돌린다(신뢰망 남은 일 G).
//!
//! # 무엇을 끊겼다고 보나
//!
//! ```text
//! Job 이 STAGING · RUNNING 이고, 그 Job 의 **가장 최근 시도**가 끝나지 않았는데
//! 그 시도의 Lease 만료 + 유예(grace) 가 지났다
//! ```
//!
//! 규범의 guard 그대로다 — `RUNNING -> INTERRUPTED (NODE_LOST)`: "lease 만료 + grace 경과".
//! 실행 중인 Agent 는 갱신 세션으로 Lease 를 늘린다(`--renew-during-execution-ms`) — 그게 멈췄다는 것이 신호다.
//!
//! # 어디로 되돌리나 (규범 §2)
//!
//! ```text
//! STAGING(ACK 전)  -> QUEUED                               STAGING_NODE_LOST — 아직 실행하지 않았다
//! RUNNING          -> INTERRUPTED -> REPLANNING -> QUEUED  마지막 체크포인트가 있을 때 — 새 시도가 거기서 이어간다
//! RUNNING          -> INTERRUPTED -> FAILED                없을 때(NO_COMMITTED_CHECKPOINT) — 처음부터 다시 돌리지 않는다
//! ```
//!
//! # 어느 체크포인트를 믿나 — 넷을 **전부** 본다
//!
//! ```text
//! 1  생산자 서명 — 풀 노드 키로 검증(§0.2)
//! 2  이 Job 의 시도가 만든 것 — 그 시도의 fence · 노드와 서명된 값이 같다(다른 시도 · 다른 노드가 흉내 낸 것 거부)
//! 3  공유 저장소의 파일을 다시 해시해 매니페스트 · 머클 루트와 맞다
//! 4  그중 (fence, step) 이 가장 큰 것
//! ```
//!
//! ★ "COMMITTED 체크포인트" 를 **공유 저장소 하나에 확정 + 서명**으로 읽는다 — 신뢰망 계획의 "기준선과 다른 점" 이다.
//!   공개 풀에서 이 완화를 쓰면 안 된다.
//!
//! # 하지 않는 것
//!
//! ```text
//! 옛 노드 멈추기      못 한다. 그 노드가 살아 있으면 계속 돌 수 있다 — 옛 Lease 를 되돌리는 같은 커밋에서 **폐기**해
//!                     그 뒤의 갱신을 거부할 뿐이다(§0.4 — 억제이지 방지가 아니다). PURE 작업을 전제한다
//!                     ★ 2026-10-02 (대체 통지 우편함 v3 §3) — 같은 커밋에 **서명된 "그 시도는 폐기됐다" 통지**를 영속한다. 갱신을 그만둔
//!                       노드도 다시 붙으면 우편함에서 받아 멈춘다(배달 · 처리는 다음 조각). 서명 키 없이는 폐기하지 않는다(인자가 필수다)
//!                     ★ 결함 227(검수 76) — 전에는 "새 시도의 더 큰 fence 가 옛 Lease 갱신을 막는다" 고 적었는데, 옛 Lease 갱신은
//!                       자기 행의 fence 만 봐서 막히지 않았다. 판정 직전에 시작된 갱신이 옛 Lease 를 되살릴 수 있었다
//! 옛 예약 풀기        풀지 않는다. 만료 **표시**만 한다(§A1 4a). 그 노드는 운영자가 멈췄음을 확인하고 풀 때까지 새 일을
//!                     받지 않는다 — 살아 있는 좀비와 새 작업이 같은 GPU 를 다투지 않게
//! 옛 시도 상태        바꾸지 않는다. Coordinator 가 그 시도의 끝을 관측하지 못했다 — 지어내지 않는다
//! ```

use std::path::{Path, PathBuf};

use gputeer_crypto::{Ed25519Verifier, InMemoryKeyring, VerifyingKey};
use gputeer_protocol::attempt_state::AttemptState;
use gputeer_protocol::job_state::JobState;
use gputeer_protocol::{pb, verify};
use prost::Message;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

/// 장애 판정 정책 — 기본값을 두지 않는다(호출부가 정한다).
#[derive(Debug, Clone)]
pub struct FailoverPolicy {
    /// Lease 만료 뒤 기다리는 시간. 짧으면 잠깐 끊긴 노드의 작업을 뺏는다.
    pub grace_ms: u64,
    /// 공유 저장소. 없으면 실행 중이던 작업은 이어갈 체크포인트가 없어 FAILED 로 끝난다.
    pub shared_checkpoint_root: Option<PathBuf>,
    /// 체크포인트 생산자 서명을 검증할 풀 노드 키.
    pub producer_keys: Vec<(String, VerifyingKey)>,
}

/// 한 Job 에 대해 한 일.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailoverOutcome {
    Requeued {
        job_id: String,
        lost_attempt_id: String,
        lost_node_id: String,
        was_running: bool,
        resume_checkpoint_id: Option<String>,
        resume_step: Option<u64>,
    },
    FailedNoCheckpoint {
        job_id: String,
        lost_attempt_id: String,
        lost_node_id: String,
    },
}

impl FailoverOutcome {
    /// 로그 한 줄.
    pub fn line(&self) -> String {
        match self {
            Self::Requeued {
                job_id,
                lost_attempt_id,
                lost_node_id,
                was_running,
                resume_checkpoint_id,
                resume_step,
            } => format!(
                "FAILOVER_REQUEUED job_id={job_id} lost_attempt={lost_attempt_id} lost_node={lost_node_id} \
                 was_running={was_running} resume_checkpoint={} resume_step={}",
                resume_checkpoint_id.as_deref().unwrap_or("-"),
                resume_step.map(|step| step.to_string()).unwrap_or_else(|| "-".to_string())
            ),
            Self::FailedNoCheckpoint {
                job_id,
                lost_attempt_id,
                lost_node_id,
            } => format!(
                "FAILOVER_FAILED job_id={job_id} lost_attempt={lost_attempt_id} lost_node={lost_node_id} \
                 reason=NO_COMMITTED_CHECKPOINT"
            ),
        }
    }
}

/// control DB 를 훑어 끊긴 시도의 Job 을 되돌린다. Job 하나에 트랜잭션 하나.
///
/// `notes` 에는 믿지 않고 버린 체크포인트와 그 이유가 쌓인다 — 조용히 버리지 않는다.
pub fn failover_lost_attempts(
    control_db: &Path,
    policy: &FailoverPolicy,
    notice_signer: &crate::supersede_notice_store::NoticeSigner,
    now_unix_ms: u64,
    notes: &mut Vec<String>,
) -> Result<Vec<FailoverOutcome>, String> {
    // 스키마가 없으면 만든다(아직 아무것도 예약하지 않은 DB 도 훑을 수 있게).
    crate::job_store::CoordinatorJobStore::open(control_db).map_err(|e| e.to_string())?;
    crate::staging_store::CoordinatorStagingStore::open(control_db).map_err(|e| e.to_string())?;
    let mut connection = Connection::open(control_db).map_err(|e| e.to_string())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(1))
        .map_err(|e| e.to_string())?;
    // ★ 2026-10-03 13:11 (조각 7b) — D6 이 UNREPORTED 보류를 쓰므로 보류 표를 준비한다.
    crate::job_holds::initialize_schema(&connection)?;

    let candidates =
        crate::job_store::list_in_run_states(&connection).map_err(|e| e.to_string())?;
    let mut outcomes = Vec::new();
    for listed in candidates {
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        // 잠금을 잡은 뒤 다시 읽는다 — 그 사이 보고가 Job 을 끝냈을 수 있다.
        let Some(job) =
            crate::job_store::fetch_job(&transaction, &listed.job_id).map_err(|e| e.to_string())?
        else {
            continue;
        };
        if !matches!(job.state, JobState::Staging | JobState::Running) {
            continue;
        }
        let latest: Option<String> = transaction
            .query_row(
                "SELECT attempt_id FROM coordinator_attempts WHERE job_id = ?1
                 ORDER BY fence_epoch DESC, attempt_id DESC LIMIT 1",
                rusqlite::params![job.job_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(attempt_id) = latest else {
            continue;
        };
        let attempt = crate::staging_store::fetch_attempt(&transaction, &attempt_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("시도 {attempt_id} 가 사라졌다"))?;
        if matches!(
            attempt.state,
            AttemptState::Completed
                | AttemptState::Failed
                | AttemptState::Cancelled
                | AttemptState::Canonical
                | AttemptState::Superseded
        ) {
            continue;
        }
        // ★ 2026-10-03 12:26 (조각 6b · 계약 §2 "새 시도를 만들지 않는다" 순서 A · §9 NODE_LOST · STAGING_NODE_LOST guard) — 잠금 뒤 다시 읽은 시도가
        //   RUN_UNKNOWN 이거나 Job 에 재배치 차단 보류가 있으면 되돌리지 않는다(사유를 남긴다). 아무것도 쓰지 않았으니 트랜잭션은 되돌아간다.
        if attempt.state == AttemptState::RunUnknown
            || crate::job_holds::job_is_held(&transaction, &job.job_id)?
        {
            notes.push(format!(
                "FAILOVER_HELD job_id={} attempt_id={attempt_id} attempt_state={:?} — 실행 여부 불명 보류가 있어 되돌리지 않는다(STOP_CONFIRMED · 운영자 해제가 푼다)",
                job.job_id, attempt.state
            ));
            continue;
        }
        let lease = crate::lease_store::fetch_lease(&transaction, &attempt.lease_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("시도 {attempt_id} 의 Lease {} 가 없다", attempt.lease_id))?;
        // ★ 2026-10-01 (signing.md §6.8) — 노드에 서명해 알린 유예(저장값)보다 일찍 다시 맡기지 않는다. 그 값을 믿고 "곧 끝남" 으로
        //   계속 돈 노드와 두 벌이 된다. 저장값은 보낸 어떤 서명 값보다 작지 않다(서명 직전에 올려 저장한다).
        let grace_ms = policy.grace_ms.max(lease.reassignment_grace_ms);
        if now_unix_ms <= lease.expires_at_unix_ms.saturating_add(grace_ms) {
            continue;
        }
        let lost_node_id = attempt.node_ids.first().cloned().unwrap_or_default();
        // ★ 검수 mbc1 ① — 통지를 보낼 노드가 없으면 폐기도 하지 않는다(규칙 1 — 폐기와 같은 커밋에 서명 통지). 아무것도 쓰기 전이라
        //   트랜잭션을 그냥 놓으면 되돌아간다. 노드 없는 시도는 저장소 손상이다 — 조용히 넘기지 않고 남긴다.
        if lost_node_id.is_empty() {
            notes.push(format!(
                "FAILOVER_SKIPPED_NO_NODE job_id={} attempt_id={attempt_id} — 시도에 노드가 없어 통지를 보낼 곳이 없다. 폐기하지 않았다(저장소 손상 의심)",
                job.job_id
            ));
            continue;
        }
        // ★ 2026-10-03 13:11 (실행 알림 계약 v18k §9 UNREPORTED_RISK_HELD · D6 · 계획 조각 7b) — 알림 없이 Lease 만 끝났다. 서명이 검증된 선언이 **정확히 PURE** 가
        //   아니면(IDEMPOTENT · SIDE_EFFECTING · 누락 · 모르는 값 · 옛 DB) 자동으로 이어가지 않고 UNREPORTED 보류를 건다 — 이 호출에서 NODE_LOST ·
        //   STAGING_NODE_LOST 는 실행하지 않는다(Lease · 예약 · Job 그대로). 그 시도에 운영자 override(release-held-job 이 푼 시도)가 있으면 기존대로
        //   이어받는다(b9 ①). 같은 시도의 보류 행이 이미 있으면 위 보류 검사가 먼저 건너뛴다(두 번째 호출이 행을 더 만들지 않는다).
        //   ★ 선언이지 행동의 강제가 아니다 — PURE 로 잘못 선언한 작업의 두 벌은 막지 못한다(CLAUDE.md §0.4 · 계약 b11 ④). 보류는 시간으로 풀지 않는다.
        if !crate::job_store::side_effect_is_pure(&transaction, &job.job_id)?
            && !crate::job_holds::attempt_has_override(&transaction, &attempt_id)?
        {
            let class = crate::job_store::side_effect_class_of(&transaction, &job.job_id)?
                .unwrap_or_else(|| "선언 없음(SIDE_EFFECTING 취급)".to_string());
            crate::job_holds::install_unreported_hold(
                &transaction,
                &job.job_id,
                &attempt_id,
                &format!("알림 없이 Lease 만료 + grace 경과 · 부작용 등급 {class}"),
                now_unix_ms,
            )?;
            transaction.commit().map_err(|e| e.to_string())?;
            notes.push(format!(
                "FAILOVER_UNREPORTED_HELD job_id={} attempt_id={attempt_id} node_id={lost_node_id} side_effect_class={class} — 노드가 알리지 못한 채 끊겼다. \
                 부작용이 있다고 선언된 작업이라 자동으로 이어가지 않는다(사람이 그 PC 를 확인하고 release-held-job 으로 푼다)",
                job.job_id
            ));
            continue;
        }
        let resume = if job.state == JobState::Running {
            find_resume_point(&transaction, policy, &job.job_id, now_unix_ms, notes, false)?
        } else {
            None
        };
        let updated = crate::job_store::requeue_after_node_lost(
            &transaction,
            &job,
            resume.as_ref().map(|found| found.body.clone()),
            now_unix_ms,
        )
        .map_err(|e| e.to_string())?;
        // ★ 2026-09-23 (결함 227 · 검수 76) — 옛 Lease 를 **같은 커밋에서** 폐기한다. 전에는 만료만 보고 되돌렸는데, 판정 직전에
        //   시작된 갱신이 판정 뒤에 저장되면(옛 시각을 잡은 채) 옛 Lease 가 미래로 늘어나 옛 노드와 새 노드가 같이 돌 수 있었다.
        //   갱신 저장은 자기 트랜잭션 안에서 폐기를 다시 읽으므로, 이 커밋 뒤의 갱신은 전부 REVOKED 로 거부된다.
        crate::lease_store::revoke_within(&transaction, &attempt.lease_id, now_unix_ms)
            .map_err(|e| e.to_string())?;
        {
            // 표시만 한다 — 지우지 않는다(§A1 4a).
            crate::staging_store::mark_reservation_expired(
                &transaction,
                &lost_node_id,
                now_unix_ms,
            )
            .map_err(|e| e.to_string())?;
            // ★ 2026-10-02 (대체 통지 우편함 v3 §3 · 규칙 1) — 폐기와 **같은 커밋**에 서명된 통지 바이트까지 쓴다. 실패하면 폐기도 되돌린다.
            let notice = crate::supersede_notice_store::sign_notice(
                notice_signer,
                &crate::supersede_notice_store::SupersededAttempt {
                    job_id: job.job_id.clone(),
                    attempt_id: attempt_id.clone(),
                    node_id: lost_node_id.clone(),
                    fence_epoch: attempt.fence_epoch,
                    lease_id: attempt.lease_id.clone(),
                    cause: pb::SupersedeCause::NodeLost,
                    job_disposition: if updated.state == JobState::Queued {
                        pb::SupersedeJobDisposition::Requeued
                    } else {
                        pb::SupersedeJobDisposition::Failed
                    },
                    decided_at_unix_ms: now_unix_ms,
                },
            );
            crate::supersede_notice_store::record_within(&transaction, &notice)?;
        }
        transaction.commit().map_err(|e| e.to_string())?;
        outcomes.push(if updated.state == JobState::Queued {
            FailoverOutcome::Requeued {
                job_id: job.job_id.clone(),
                lost_attempt_id: attempt_id,
                lost_node_id,
                was_running: job.state == JobState::Running,
                resume_checkpoint_id: resume.as_ref().map(|found| found.checkpoint_id.clone()),
                resume_step: resume.as_ref().map(|found| found.step),
            }
        } else {
            FailoverOutcome::FailedNoCheckpoint {
                job_id: job.job_id.clone(),
                lost_attempt_id: attempt_id,
                lost_node_id,
            }
        });
    }
    Ok(outcomes)
}

/// ★ 2026-10-03 11:51 (실행 알림 계획 조각 5f) — 정지 확인 처리(`run_notice_store`)가 쓰는 이어갈 지점 찾기. 장애 이어받기와 **같은** 탐색 · 검증이다.
pub(crate) fn resume_body_for(
    connection: &Connection,
    policy: &FailoverPolicy,
    job_id: &str,
    now_unix_ms: u64,
    notes: &mut Vec<String>,
) -> Result<Option<Vec<u8>>, String> {
    Ok(find_resume_point(connection, policy, job_id, now_unix_ms, notes, false)?.map(|point| point.body))
}

struct ResumePoint {
    checkpoint_id: String,
    step: u64,
    body: Vec<u8>,
}

fn find_resume_point(
    connection: &Connection,
    policy: &FailoverPolicy,
    job_id: &str,
    now_unix_ms: u64,
    notes: &mut Vec<String>,
    list_error_as_none: bool,
) -> Result<Option<ResumePoint>, String> {
    let Some(shared_root) = policy.shared_checkpoint_root.as_ref() else {
        notes.push(format!(
            "FAILOVER_NO_SHARED_ROOT job_id={job_id} — 공유 저장소가 없어 이어갈 체크포인트를 찾지 않는다"
        ));
        return Ok(None);
    };
    let mut keyring = InMemoryKeyring::new();
    for (id, key) in &policy.producer_keys {
        keyring.insert(id.clone(), *key);
    }
    let verifier = Ed25519Verifier::new(&keyring);
    // ★ 2026-10-04 15:47 (실행 알림 계약 D6 보강 · 계획 §5 4번) — 더 높은 fence 의 시도가 생긴 뒤에는 옛 시도의 체크포인트를 재개 후보로 올리지 않는다.
    //   이어받은 뒤에도 옛 노드가 공유 저장소에 계속 쓸 수 있다(쓰기 자체는 막지 못한다 — 저장소에 fence 관문이 없다). 그 파일이 옛 시도의 서명 ·
    //   fence 와 맞으니 전에는 새 시도가 체크포인트를 내기 전 다음 이어받기에서 더 높은 step 으로 뽑혔다 — 새 시도가 시작한 지점과 갈라진 이력이다.
    //   그래서 후보는 ① Job 의 최고 fence 시도가 낸 것 ② 지금 시도가 이어받은 기준 지점(Job 에 저장된 바로 그 바이트) 둘뿐이다.
    let top_fence: Option<Vec<u8>> = connection
        .query_row(
            "SELECT fence_epoch FROM coordinator_attempts WHERE job_id = ?1
             ORDER BY fence_epoch DESC, attempt_id DESC LIMIT 1",
            rusqlite::params![job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let top_fence = top_fence
        .map(|bytes| {
            <[u8; 8]>::try_from(bytes.as_slice())
                .map(u64::from_be_bytes)
                .map_err(|_| format!("job {job_id} 의 시도 fence 를 읽지 못했다"))
        })
        .transpose()?;
    let carried: Option<Vec<u8>> = connection
        .query_row(
            "SELECT resume_checkpoint FROM coordinator_jobs WHERE job_id = ?1",
            rusqlite::params![job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    let mut best: Option<(u64, u64, ResumePoint)> = None;
    // ★ 2026-09-24 (결함 254 · 재검수 83) — "공유 저장소 목록을 못 읽었다" 만 호출자가 원하면 "지점 없음" 으로 받는다(선점 — 보고를 롤백하지
    //   않으려고). control DB 오류는 그대로 올린다 — 전에는 선점 쪽이 **모든** 오류를 삼켜 DB 손상까지 "지점 없음" 으로 커밋했다.
    //   ★ 결함 263 (재검수 85) — 한계: 목록이 **일부만** 읽혀도 전체를 못 읽은 것으로 본다(읽힌 정상 후보도 버린다). 목록 뒤 체크포인트 파일
    //     읽기 오류(NAS I/O)는 그 체크포인트를 "건너뜀" 으로 처리한다 — "그 밖의 오류는 올린다" 가 아니다.
    let listed = match gputeer_checkpoint::shared::list_signed_manifests(shared_root, job_id) {
        Ok(listed) => listed,
        Err(error) if list_error_as_none => {
            notes.push(format!(
                "RESUME_POINT_UNAVAILABLE job_id={job_id} detail={error} — 공유 저장소 목록을 못 읽어 새 이어갈 지점 없이 멈춘다(보고는 저장한다)"
            ));
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    for (checkpoint_id, body) in listed {
        let mut skip = |why: String| {
            notes.push(format!(
                "FAILOVER_CHECKPOINT_SKIPPED job_id={job_id} checkpoint_id={checkpoint_id} reason={why}"
            ))
        };
        let Ok(manifest) = pb::CheckpointManifest::decode(body.as_slice()) else {
            skip("디코드 실패".into());
            continue;
        };
        if manifest.checkpoint_id != checkpoint_id {
            skip("파일 이름과 checkpoint_id 가 다르다".into());
            continue;
        }
        if let Err(error) = verify(
            &manifest,
            1,
            &verifier,
            now_unix_ms,
            &mut gputeer_protocol::signing::NoReplayCheck,
        ) {
            skip(format!("생산자 서명 검증 실패 {error:?}"));
            continue;
        }
        if manifest.job_id != job_id {
            skip("다른 Job 의 체크포인트다".into());
            continue;
        }
        let Some(attempt) = crate::staging_store::fetch_attempt(connection, &manifest.attempt_id)
            .map_err(|e| e.to_string())?
        else {
            skip("모르는 시도가 만들었다".into());
            continue;
        };
        if attempt.job_id != job_id
            || attempt.fence_epoch != manifest.fence_epoch
            || !attempt.node_ids.contains(&manifest.producer_node_id)
        {
            skip("그 시도의 fence · 노드와 서명된 값이 다르다".into());
            continue;
        }
        if top_fence.is_some_and(|top| top > manifest.fence_epoch) && carried.as_deref() != Some(body.as_slice()) {
            skip("더 높은 fence 의 시도가 있다 — 옛 시도의 체크포인트는 이어받은 기준 지점만 쓴다(D6)".into());
            continue;
        }
        if let Err(error) = gputeer_checkpoint::shared::verify_on_disk(shared_root, &manifest) {
            skip(error);
            continue;
        }
        let key = (manifest.fence_epoch, manifest.step);
        if best
            .as_ref()
            .is_none_or(|(fence, step, _)| key > (*fence, *step))
        {
            best = Some((
                key.0,
                key.1,
                ResumePoint {
                    checkpoint_id: checkpoint_id.clone(),
                    step: manifest.step,
                    body,
                },
            ));
        }
    }
    Ok(best.map(|(_, _, point)| point))
}

/// 운영자가 풀어 준 끊긴 노드의 예약.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorRelease {
    pub node_id: String,
    pub attempt_id: String,
    pub job_id: String,
    pub released_gpu_ids: Vec<String>,
}

/// ★ 2026-09-23 (신뢰망 남은 일 G) — **운영자가 "그 노드는 멈췄다" 고 확인한 뒤** 끊긴 노드의 옛 예약을 푼다.
///
/// 장애 이어받기는 옛 예약을 지우지 않는다(§A1 4a — 살아 있는 좀비와 새 작업이 같은 GPU 를 다투지 않게).
/// 그래서 그 노드를 다시 쓰려면 누군가 멈췄음을 확인해야 한다 — Coordinator 는 증명할 수 없다(§0.4).
///
/// 관문: 그 예약의 시도가 **대체됐거나 끝난 Job 의 것**일 때만 푼다. Job 이 아직 그 시도로 도는 중(STAGING · RUNNING ·
/// PAUSED)이면 거부한다 — 운영자가 노드를 잘못 짚어 지금 도는 남의 작업을 푸는 실수를 막는다(§0.1).
///
/// ★ 2026-09-23 (결함 228 · 검수 76 — 전에는 "살아 있는 시도의 예약은 거부한다" 고 적었다) — 이 관문은 **Job 이 넘어갔는지**를
///   볼 뿐, 옛 시도의 **프로세스가 멈췄는지**는 증명하지 않는다. 장애 이어받기 뒤 옛 시도의 저장 상태는 STARTING · RUNNING 으로 남아
///   있을 수 있고, 그 예약도 풀린다. 멈춤의 근거는 **운영자 진술**뿐이다(그래서 진술을 남긴다) — Coordinator 는 증명할 수 없다(§0.4).
///
/// 기록은 `coordinator_operator_releases` 에 남는다 — 누가 · 언제 · 무엇을 풀었나. 되돌리지 않는 기록이다.
/// ★ 2026-10-03 (조각 4b) — 이제는 해제 기록 표(`coordinator_release_facts` + `coordinator_release_evidence` 의 OPERATOR_RELEASE)에 남는다.
///   옛 표는 감사 원본으로 남고 새로 쓰지 않는다.
pub fn release_lost_node_by_operator(
    control_db: &Path,
    node_id: &str,
    operator_statement: &str,
    now_unix_ms: u64,
) -> Result<OperatorRelease, String> {
    release_lost_node_by_operator_with(control_db, node_id, operator_statement, now_unix_ms, None)
}

/// ★ 2026-10-05 02:26 (실행 알림 계약 v18q ⑤) — `failover_grace_ms` 는 취소된 Job 의 **최신 시도** 예약을 풀 때만 쓴다(장애 이어받기의
///   `--failover-grace-ms` 와 같은 값 — 유예를 지어내지 않으므로 그 경우 없으면 거부한다).
pub fn release_lost_node_by_operator_with(
    control_db: &Path,
    node_id: &str,
    operator_statement: &str,
    now_unix_ms: u64,
    failover_grace_ms: Option<u64>,
) -> Result<OperatorRelease, String> {
    if operator_statement.trim().is_empty() {
        return Err("RELEASE_REFUSED: 운영자 진술(누가 · 무엇을 확인했나)이 비었다".to_string());
    }
    crate::staging_store::CoordinatorStagingStore::open(control_db).map_err(|e| e.to_string())?;
    let mut connection = Connection::open(control_db).map_err(|e| e.to_string())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(1))
        .map_err(|e| e.to_string())?;
    // ★ 2026-10-03 (조각 4b) — 해제 기록 표(사실 · 근거)와 옛 기록 이관. 이관이 멈추면(옛 기록의 시도 행 없음 등) 풀지 않는다.
    crate::reservation_release::initialize_release_schema(&connection)
        .map_err(|e| format!("RELEASE_REFUSED: 해제 기록 표를 준비하지 못했다 — {e:?}"))?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let reservation = crate::staging_store::fetch_node_reservation(&transaction, node_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("RELEASE_REFUSED: {node_id} 에 예약이 없다"))?;
    let job = crate::job_store::fetch_job(&transaction, &reservation.job_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("RELEASE_REFUSED: 예약의 Job {} 이 없다", reservation.job_id))?;
    // ★ 2026-10-03 12:26 (조각 6b · 계약 §2 "새 시도를 만들지 않는다" release-lost-node 줄) — 재배치 차단 보류가 있는 Job 의 예약은 풀지 않는다.
    //   NOTICE 는 그 시도의 STOP_CONFIRMED 가, UNREPORTED 는 release-held-job(조각 7)이 푼다.
    if crate::job_holds::job_is_held(&transaction, &job.job_id)? {
        return Err(format!(
            "RELEASE_REFUSED: Job {} 에 실행 여부 불명 보류가 있다 — 이 명령으로 풀지 않는다(정지 확인 알림 · release-held-job 이 푼다)",
            job.job_id
        ));
    }
    let latest: Option<String> = transaction
        .query_row(
            "SELECT attempt_id FROM coordinator_attempts WHERE job_id = ?1
             ORDER BY fence_epoch DESC, attempt_id DESC LIMIT 1",
            rusqlite::params![reservation.job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let superseded = latest.as_deref() != Some(reservation.attempt_id.as_str());
    // ★ 2026-10-05 02:26 (실행 알림 계약 v18q ⑤ (a)) — 풀리지 않은 불명(시도가 RUN_UNKNOWN · 또는 STOP 으로 해소되지 않은 RUN_UNKNOWN 알림)이 있으면 Job 상태와
    //   무관하게 풀지 않는다 — RUN_UNKNOWN 은 STOP 만 푼다. 최종 Job 에는 늦은 RUN_UNKNOWN 이 NOTICE 보류를 만들지 않아 위 보류 검사로는 막히지 않았다
    //   (FAILED · COMPLETED 의 기존 공백도 같이 닫는다 — 더 거부하는 쪽).
    if crate::run_notice_store::has_unresolved_run_unknown(&transaction, &reservation.attempt_id)? {
        return Err(format!(
            "RELEASE_REFUSED: {node_id} 의 예약을 쥔 시도 {} 에 풀리지 않은 실행 여부 불명이 있다 — 그 시도의 정지 확인(STOP_CONFIRMED)만 푼다",
            reservation.attempt_id
        ));
    }
    // ★ 계약 v18q ⑤ (b) — 취소된 Job 의 **최신 시도** 예약: 취소가 Lease 를 폐기했어도 노드는 다음 갱신 · 끊김 시한까지 돈다. 장애 이어받기와 같은 식 ·
    //   같은 경계(지금 > 만료 + max(정책 유예, 서명한 유예))가 지나야 푼다. 운영자 진술은 기록이지 정지 증거가 아니다.
    if job.state == JobState::Cancelled && !superseded {
        let Some(policy_grace_ms) = failover_grace_ms else {
            return Err(format!(
                "RELEASE_REFUSED: {node_id} 의 예약은 취소된 Job {} 의 최신 시도 것이다 — --failover-grace-ms(장애 이어받기와 같은 값)를 함께 줘야 한다",
                job.job_id
            ));
        };
        let attempt = crate::staging_store::fetch_attempt(&transaction, &reservation.attempt_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("RELEASE_REFUSED: 시도 {} 의 행이 없다(저장소 손상)", reservation.attempt_id))?;
        let lease = crate::lease_store::fetch_lease(&transaction, &attempt.lease_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("RELEASE_REFUSED: 시도 {} 의 Lease 가 없다", reservation.attempt_id))?;
        let grace_ms = policy_grace_ms.max(lease.reassignment_grace_ms);
        let free_after = lease.expires_at_unix_ms.saturating_add(grace_ms);
        if lease.revoked_at_unix_ms.is_none() || now_unix_ms <= free_after {
            return Err(format!(
                "RELEASE_REFUSED: 취소된 Job {} 의 최신 시도 {} 가 아직 돌 수 있다 — Lease 폐기 여부 {} · {free_after} 이후에만 푼다(지금 {now_unix_ms})",
                job.job_id,
                reservation.attempt_id,
                lease.revoked_at_unix_ms.is_some()
            ));
        }
    }
    let job_moved_on = matches!(
        job.state,
        JobState::Queued | JobState::Completed | JobState::Failed | JobState::Cancelled
    );
    if !superseded && !job_moved_on {
        return Err(format!(
            "RELEASE_REFUSED: {node_id} 의 예약은 **살아 있는 시도**({}) 의 것이다(Job {:?}) — 풀지 않는다. \
             장애 이어받기가 먼저 그 Job 을 되돌려야 한다",
            reservation.attempt_id, job.state
        ));
    }
    // ★ 2026-10-03 (계약 v18k §3 b16 ③ · 조각 4b) — 판정은 위 그대로, **기록 방식만** 바꿨다: 예약을 지우는 같은 트랜잭션에서
    //   해제 사실 + OPERATOR_RELEASE 근거(진술 · 노드 · Job · 시도 · fence · 시각의 고정 인코딩)를 쓴다. 옛 `coordinator_operator_releases` 에는
    //   더 쓰지 않는다(옛 행은 열 때 새 표로 옮긴다). 그래서 늦게 온 정지 확인 · 종료 보고는 "이미 해제됨" 으로 근거만 더한다.
    let outcome = crate::reservation_release::release_by_operator_within(
        &transaction,
        crate::reservation_release::OperatorReleaseCommand::ReleaseLostNode,
        operator_statement,
        node_id,
        &reservation.attempt_id,
        now_unix_ms,
    )
    .map_err(|e| format!("RELEASE_REFUSED: {e:?}"))?;
    let released = match outcome {
        crate::reservation_release::ReleaseOutcome::Released(record) => record,
        // 예약이 그 시도에 남아 있는데 해제 사실이 이미 있다 — 기록이 어긋났다. 예약을 남긴 채 성공이라 하지 않는다.
        other => {
            return Err(format!(
                "RELEASE_REFUSED: {node_id} 의 예약은 남았는데 해제 기록은 이미 있다 — 사람이 봐야 한다({other:?})"
            ))
        }
    };
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(OperatorRelease {
        node_id: node_id.to_string(),
        attempt_id: released.attempt_id,
        job_id: released.job_id,
        released_gpu_ids: released.released_gpu_ids,
    })
}

/// ★ 2026-10-03 13:28 (실행 알림 계약 v18k §6 (3) · §9 · 계획 조각 7c) — `release-held-job` 이 푼 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldJobRelease {
    pub job_id: String,
    /// 보류를 푼 시도들(시도 순).
    pub released_attempts: Vec<String>,
    /// Job 이 최종 상태였다 — 그 시도들의 Lease 를 폐기하고 남은 예약을 지웠다.
    pub job_final: bool,
    /// 한 줄 기록들(예약 해제 · 예약 없음 등).
    pub notes: Vec<String>,
}

/// ★ 2026-10-03 13:28 (실행 알림 계약 v18k §6 (3) · §9 "release-held-job 한 명령" · 계획 조각 7c) — **사람이 그 PC 를 확인한 뒤** D6 보류(UNREPORTED)를 푼다.
///
/// ```text
/// 거부     진술이 비었다 · Job 이 없다 · NOTICE 보류가 하나라도 있다(그것은 그 시도의 STOP_CONFIRMED 만 푼다) · 풀 UNREPORTED 보류가 없다
/// 한 커밋  UNREPORTED 행 제거(감사 기록) · 그 시도에 override(다음 failover 가 같은 시도로 다시 보류하지 않고 이어받는다 — b9 ①)
///          Job 이 최종(COMPLETED · FAILED · CANCELLED · ARCHIVED)이면 그 시도의 Lease 폐기(revoked_at — failover 와 같은 칸) ·
///          그 시도가 쥔 예약 삭제(해제 사실 + OPERATOR_RELEASE 근거 — release-held-job 명령 이름으로)
///          최종이 아니면 Job · Lease · 예약은 그대로다 — 다음 장애 이어받기가 기존 행(NODE_LOST · STAGING_NODE_LOST)으로 간다
/// ```
/// ★ release-lost-node 의 **판정**은 바꾸지 않는다(계약 — 예약 해제와 Job 해제를 가른다).
pub fn release_held_job_by_operator(
    control_db: &Path,
    job_id: &str,
    operator_statement: &str,
    now_unix_ms: u64,
) -> Result<HeldJobRelease, String> {
    if operator_statement.trim().is_empty() {
        return Err("RELEASE_HELD_REFUSED: 운영자 진술(누가 · 무엇을 확인했나)이 비었다".to_string());
    }
    crate::job_store::CoordinatorJobStore::open(control_db).map_err(|e| e.to_string())?;
    crate::staging_store::CoordinatorStagingStore::open(control_db).map_err(|e| e.to_string())?;
    let mut connection = Connection::open(control_db).map_err(|e| e.to_string())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(1))
        .map_err(|e| e.to_string())?;
    crate::job_holds::initialize_schema(&connection)?;
    crate::reservation_release::initialize_release_schema(&connection)
        .map_err(|e| format!("RELEASE_HELD_REFUSED: 해제 기록 표를 준비하지 못했다 — {e:?}"))?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let job = crate::job_store::fetch_job(&transaction, job_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("RELEASE_HELD_REFUSED: Job {job_id} 이 없다"))?;
    let holds = crate::job_holds::holds_for_job(&transaction, job_id)?;
    if let Some(notice) = holds.iter().find(|h| h.hold_kind == crate::job_holds::NOTICE_RUN_UNKNOWN) {
        return Err(format!(
            "RELEASE_HELD_REFUSED: Job {job_id} 의 시도 {} 에 실행 여부 불명 알림 보류가 있다 — 그 시도의 정지 확인(STOP_CONFIRMED)만 푼다",
            notice.attempt_id
        ));
    }
    let attempts: Vec<String> = holds
        .iter()
        .filter(|h| h.hold_kind == crate::job_holds::UNREPORTED_SIDE_EFFECT_RISK)
        .map(|h| h.attempt_id.clone())
        .collect();
    if attempts.is_empty() {
        return Err(format!("RELEASE_HELD_REFUSED: Job {job_id} 에 풀 보류(UNREPORTED)가 없다"));
    }
    let job_final = matches!(
        job.state,
        JobState::Completed | JobState::Failed | JobState::Cancelled | JobState::Archived
    );
    let mut notes = Vec::new();
    for attempt_id in &attempts {
        crate::job_holds::release_unreported_by_operator(&transaction, job_id, attempt_id, operator_statement, now_unix_ms)?;
        if !job_final {
            continue;
        }
        let attempt = crate::staging_store::fetch_attempt(&transaction, attempt_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("RELEASE_HELD_REFUSED: 보류된 시도 {attempt_id} 의 행이 없다(저장소 손상)"))?;
        // ★ 2026-10-05 02:26 (실행 알림 계약 v18q ⑤ (a)) — UNREPORTED 보류 → 취소 → (관측 못 한 종료 보고) → 늦은 RUN_UNKNOWN: 최종 Job 이라 NOTICE 보류가 없고
        //   시도 상태도 RUN_UNKNOWN 이 아닐 수 있다. 저장된 알림까지 봐서 풀리지 않은 불명이면 STOP 만 푼다.
        if crate::run_notice_store::has_unresolved_run_unknown(&transaction, attempt_id)? {
            return Err(format!(
                "RELEASE_HELD_REFUSED: 시도 {attempt_id} 에 풀리지 않은 실행 여부 불명이 있다 — 그 시도의 정지 확인(STOP_CONFIRMED)만 푼다"
            ));
        }
        crate::lease_store::revoke_within(&transaction, &attempt.lease_id, now_unix_ms)
            .map_err(|e| format!("RELEASE_HELD_REFUSED: 시도 {attempt_id} 의 Lease 를 폐기하지 못했다 — {e}"))?;
        let node_id = attempt.node_ids.first().cloned().unwrap_or_default();
        let reservation = crate::staging_store::fetch_node_reservation(&transaction, &node_id)
            .map_err(|e| e.to_string())?;
        match reservation {
            Some(reservation) if reservation.attempt_id == *attempt_id => {
                crate::reservation_release::release_by_operator_within(
                    &transaction,
                    crate::reservation_release::OperatorReleaseCommand::ReleaseHeldJob,
                    operator_statement,
                    &node_id,
                    attempt_id,
                    now_unix_ms,
                )
                .map_err(|e| format!("RELEASE_HELD_REFUSED: {e:?}"))?;
                notes.push(format!("RESERVATION_RELEASED node_id={node_id} attempt_id={attempt_id}"));
            }
            _ => notes.push(format!(
                "RESERVATION_ALREADY_GONE node_id={node_id} attempt_id={attempt_id} — 그 시도의 예약이 이미 없다(남의 예약은 건드리지 않는다)"
            )),
        }
    }
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(HeldJobRelease {
        job_id: job_id.to_string(),
        released_attempts: attempts,
        job_final,
        notes,
    })
}

/// ★ 2026-09-23 (신뢰망 남은 일 H) — 노드 소유자가 작업을 멈췄다(그 노드가 서명한 INTERRUPTED 보고 · 종료 관측).
///
/// ```text
/// Job  RUNNING -> PAUSED (OWNER_PREEMPT)      이어갈 지점 = 공유 저장소의 검증된 마지막 체크포인트
/// 노드 "되찾김" 표시                          pool_snapshot 이 그 노드를 **다시 Hello 할 때까지** 소식 없음으로 접는다
/// ```
///
/// 스케줄러는 PAUSED Job 도 배치한다 — 다른 노드에서 `PAUSED -> RUNNING`(RESUMED)으로 이어간다. Lease 만료를 기다리지 않는다.
/// 이 Job 의 최근 시도가 아니거나 Job 이 RUNNING 이 아니면 아무것도 하지 않는다(`Ok(None)`).
pub fn pause_for_owner_preempt(
    control_db: &Path,
    job_id: &str,
    attempt_id: &str,
    node_id: &str,
    policy: &FailoverPolicy,
    now_unix_ms: u64,
    notes: &mut Vec<String>,
) -> Result<Option<String>, String> {
    let mut connection = Connection::open(control_db).map_err(|e| e.to_string())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(1))
        .map_err(|e| e.to_string())?;
    ensure_reclaim_table(&connection)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let line = pause_for_owner_preempt_within(
        &transaction,
        job_id,
        attempt_id,
        node_id,
        policy,
        now_unix_ms,
        notes,
    )?;
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(line)
}

/// [`pause_for_owner_preempt`] 의 본체 — **호출자의 트랜잭션 안에서** 돈다(커밋하지 않는다).
///
/// ★ 2026-09-23 (결함 215 · 검수 73) — 종료 보고 저장 · 예약 해제와 **같은 커밋**에 넣으려고 뗐다. 따로 커밋하면
///   그 사이에 죽었을 때 "예약 없음 + Job RUNNING" 이 남고, 재전송도 그것을 되살리지 못했다.
pub(crate) fn pause_for_owner_preempt_within(
    transaction: &Connection,
    job_id: &str,
    attempt_id: &str,
    node_id: &str,
    policy: &FailoverPolicy,
    now_unix_ms: u64,
    notes: &mut Vec<String>,
) -> Result<Option<String>, String> {
    ensure_reclaim_table(transaction)?;
    let Some(job) = crate::job_store::fetch_job(transaction, job_id).map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    if job.state != JobState::Running {
        return Ok(None);
    }
    let latest: Option<String> = transaction
        .query_row(
            "SELECT attempt_id FROM coordinator_attempts WHERE job_id = ?1
             ORDER BY fence_epoch DESC, attempt_id DESC LIMIT 1",
            rusqlite::params![job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if latest.as_deref() != Some(attempt_id) {
        return Ok(None);
    }
    // ★ 2026-09-24 (결함 234 · 재검수 79) — 이 함수는 종료 보고 저장과 **같은 트랜잭션**에서 돈다(215). 이어갈 지점 조회가 공유 저장소
    //   장애로 실패해도 오류로 올리지 않는다 — 올리면 보고 저장까지 롤백되고, 저장소가 계속 죽어 있으면 보고가 영영 안 남는다(증거 유실).
    //   그때는 새 지점 없이 멈춘다(전에 고른 지점은 그대로 둔다) — 사실을 한 줄 남긴다.
    let resume = find_resume_point(transaction, policy, job_id, now_unix_ms, notes, true)?;
    crate::job_store::pause_for_owner_preempt(
        transaction,
        &job,
        resume.as_ref().map(|found| found.body.clone()),
    )
    .map_err(|e| e.to_string())?;
    // ★ 결함 227 — 선점으로 옮기는 Job 도 옛 Lease 를 같은 커밋에서 폐기한다(늦은 갱신이 옛 시도를 되살리지 않게).
    let attempt = crate::staging_store::fetch_attempt(transaction, attempt_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("시도 {attempt_id} 가 사라졌다"))?;
    crate::lease_store::revoke_within(transaction, &attempt.lease_id, now_unix_ms)
        .map_err(|e| e.to_string())?;
    // ★ 2026-09-24 (결함 237 · 253 · 재검수 80 · 83) — 되찾음 판정은 "되찾은 시각 >= 마지막 FRESH 시각" 이다. 시계가 뒤로 가면 방금 되찾은
    //   노드가 다시 후보가 됐고(237), 그걸 max(지금, 마지막 FRESH) 로 막았더니 시계가 **미래로 튀었던** FRESH 가 있으면 owner-resume 뒤에도
    //   영원히 숨었다(253). 그래서 되찾을 때 그 노드의 FRESH 기록을 **지운다** — 비교할 옛 값이 없어지고, 되찾기 뒤의 FRESH 만 남는다.
    //   (FRESH 기록은 단조 갱신이라, 지우지 않으면 미래 값이 계속 이긴다.)
    let seen_table = transaction
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'coordinator_node_session_seen'",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .is_some();
    if seen_table {
        transaction
            .execute(
                "DELETE FROM coordinator_node_session_seen WHERE node_id = ?1",
                rusqlite::params![node_id],
            )
            .map_err(|e| e.to_string())?;
    }
    let reclaimed_at = now_unix_ms;
    transaction
        .execute(
            "INSERT INTO coordinator_node_reclaims(node_id, reclaimed_at_unix_ms) VALUES (?1, ?2)
             ON CONFLICT(node_id) DO UPDATE SET reclaimed_at_unix_ms = excluded.reclaimed_at_unix_ms",
            rusqlite::params![node_id, reclaimed_at.to_be_bytes().to_vec()],
        )
        .map_err(|e| e.to_string())?;
    Ok(Some(format!(
        "OWNER_PREEMPTED job_id={job_id} attempt_id={attempt_id} node_id={node_id} resume_step={}",
        resume
            .as_ref()
            .map(|found| found.step.to_string())
            .unwrap_or_else(|| "-".to_string())
    )))
}

/// "소유자가 되찾음" 표시. `pool_snapshot()` 이 읽는다(있을 때만).
pub(crate) fn ensure_reclaim_table(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS coordinator_node_reclaims (
                node_id TEXT PRIMARY KEY,
                reclaimed_at_unix_ms BLOB NOT NULL CHECK(length(reclaimed_at_unix_ms) = 8)
            );",
        )
        .map_err(|e| e.to_string())
}
