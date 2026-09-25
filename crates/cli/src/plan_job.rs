//! `gputeer plan-job` — 저장된 Job 을 **실제로 스케줄 가능한지 확인해**
//! `SUBMITTED → PLANNING → QUEUED` 로 올린다.
//!
//! # ★ `QUEUED` 는 아무 때나 붙일 수 있는 딱지가 아니다
//!
//! `job_store.rs` 의 `enqueue()` 문서가 못박아 뒀다 — 이 전이는
//! **`PLAN_READY`, 즉 실행 가능한 계획이 최소 하나 있다**는 뜻이고
//! `plan_id` 가 그 계획을 가리킨다.
//!
//! 그래서 이 명령은 아무 문자열이나 넣고 상태만 올리지 않는다. 실제로
//! 후보를 계산해 **하나라도 적격일 때만** 올린다. 적격이 하나도 없으면
//! 올리지 않고 **왜 떨어졌는지 노드별로 말한다** — 그게 운영자가 다음에
//! 무엇을 고쳐야 하는지다.
//!
//! ```text
//! 1  job store 에서 저장된 Manifest binding 을 꺼낸다
//! 2  운영자 keyring 으로 **다시 검증한다**            <- 아래 참조
//! 3  Verified 뒤에만 JobRequirements 로 옮긴다
//! 4  같은 control DB 의 inventory 로 pool_snapshot 을 만든다
//! 5  evaluate_eligibility() 로 후보를 가린다
//! 6  적격이 있으면 plan_and_enqueue(plan_id) — PLANNING · QUEUED 를 한 트랜잭션으로(결함 402)
//!    없으면 아무 상태도 안 바꾸고 이유를 보고한다
//! ```
//!
//! # ★ 왜 저장된 Manifest 를 **다시** 검증하는가
//!
//! `DoD-50` 이 `get_manifest_binding()` 을 만들면서 명시했다 — 반환값은
//! **raw** 이고 "authoritative key directory 재검증 전 scheduler/Grant 에
//! 사용할 수 없다". 저장은 그때 검증됐다는 사실을 기록할 뿐이고, **지금**
//! 그 서명자를 여전히 믿는지는 다른 질문이다(그 사이 폐기·격리됐을 수
//! 있다). 그래서 이 명령은 운영자 keyring 으로 다시 검증하고,
//! `Verified<M>` 를 새로 얻은 뒤에만 필드를 읽는다.
//!
//! 저장 당시의 서명자와 지금 검증된 서명자가 다르면 거부한다 — 같은
//! `job_id` 에 다른 사람의 서명이 붙는 것을 그냥 지나치지 않는다.
//!
//! # ★ 왜 DB 가 **하나**인가
//!
//! staging 예약이 자기 연결로 inventory 테이블을 읽는다
//! (`staging_store.rs`). job 과 inventory 가 다른 파일이면 후보는 골라도
//! 예약이 안 붙는다 — `tests/shared_control_db.rs` 가 그 사실을 재고
//! 있다. 그래서 이 명령은 `--control-db` **하나**만 받는다.
//!
//! # 이 명령이 하지 않는 것
//!
//! ```text
//! 안 한다   노드 예약(STAGING)      그건 orchestrate 의 일이고 아직
//!                                   production 호출자가 없다
//! 안 한다   Grant 발급              ③ 이다
//! 안 한다   device→member 해석      운영자가 --submitter-member 로 선언한다
//! 안 한다   freshness 정책 결정      운영자가 --max-snapshot-age-ms 로 준다
//! ```

use std::collections::BTreeMap;

use gputeer_coordinator::inventory_store::CoordinatorInventoryStore;
use gputeer_coordinator::job_store::CoordinatorJobStore;
use gputeer_coordinator::manifest_requirements::job_requirements_from_manifest;
use gputeer_crypto::{Ed25519Verifier, PersistentKeyring, PlaintextPolicy};
use gputeer_protocol::signing::{verify, NoReplayCheck};
use gputeer_scheduler::{evaluate_eligibility, EligibilityResolution, Policy};

/// `gputeer plan-job` 진입점.
pub fn run(args: &[String]) -> Result<String, String> {
    let flags = parse_flags(args)?;

    let job_id = require(&flags, "--job-id")?;
    let control_db = require(&flags, "--control-db")?;
    let keyring_path = require(&flags, "--submitter-keyring")?;
    let submitter_member = require(&flags, "--submitter-member")?;
    // ★ freshness 는 **정책**이다. 기본값을 여기서 정하면 운영자가
    //   선택하지 않은 값으로 후보가 떨어지거나 살아난다. 반드시 받는다.
    let max_snapshot_age_ms: u64 = require(&flags, "--max-snapshot-age-ms")?
        .parse()
        .map_err(|e| format!("--max-snapshot-age-ms 파싱 실패: {e}"))?;

    let now_unix_ms = now_unix_ms();

    let mut jobs = CoordinatorJobStore::open(control_db)
        .map_err(|e| format!("job store 를 열지 못했다({control_db}): {e}"))?;
    // 영속성 판정은 저장소에 물어본다 — 여기서 다시 만들지 않는다.
    if !jobs.is_durable() {
        return Err(format!(
            "--control-db 가 영속이 아니다({control_db:?}) — 올린 상태가 프로세스와 함께 사라지는데 로그에는 성공으로 찍힌다"
        ));
    }

    let binding = jobs
        .get_manifest_binding(job_id)
        .map_err(|e| format!("Manifest binding 조회 실패: {e}"))?
        .ok_or_else(|| format!("PLAN_REFUSED: {job_id} 에 저장된 Manifest 가 없다"))?;

    // ── 2. 지금 다시 검증한다 ────────────────────────────────────────
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
    let verifier = Ed25519Verifier::new(&keyring);

    let verified = verify(
        &binding.manifest,
        1,
        &verifier,
        now_unix_ms,
        &mut NoReplayCheck,
    )
    .map_err(|e| {
        format!(
            "PLAN_REFUSED: 저장된 Manifest 를 지금 다시 검증하지 못했다: {e:?} — 저장될 때는 유효했더라도 그 사이 서명자가 폐기·격리됐거나 Manifest 가 만료됐을 수 있다"
        )
    })?;

    // ★★ **여기 있던 "저장 당시 서명자 == 지금 검증된 서명자" 대조를
    //   지웠다 — 도달할 수 없는 코드였다.**
    //
    //   뮤테이션으로 지워도 아무 테스트가 안 깨지길래 "DB 변조 방어라
    //   재기 어렵다" 고 적어 뒀는데, 실제로 변조해 보니 **더 앞에서
    //   막혔다.** 저장소의 load 가 이미 둘을 강제한다:
    //
    //       job_store.rs:906   manifest.submitter_device_id == job.submitter_device_id
    //       job_store.rs:912   signer_id_at_submission      == job.submitter_device_id
    //
    //   그리고 `Verified::signer_id()` 는 **메시지의 필드**에서 온다
    //   (`signing.rs:843`).
    //
    //   ★★ **어느 필드인지가 증명의 마지막 칸이다** (2026-09-07 독립
    //     검수 지적). 처음엔 `signing.rs:843` 까지만 적었는데, 그 줄은
    //     "추상 메서드 `msg.signer_id()` 를 복사한다" 만 보여 줄 뿐
    //     `JobManifest` 가 **무엇을** 돌려주는지는 말하지 않는다.
    //     그 칸이 비면 증명이 형식적으로 성립하지 않는다.
    //
    //       signable.rs:61-63
    //         impl Signable for pb::JobManifest {
    //             fn signer_id(&self) -> &str { &self.submitter_device_id }
    //
    //   넷을 합치면 두 값은 항상 같다.
    //
    //   ★ `DoD-62` 에서 **똑같은 실수를 했다** — 거기서도 내가 넣은
    //     재대조를 `fetch_report_binding` 이 이미 하고 있었다. 두 번째다.
    //     패턴이 보인다: **"한 번 더 확인해서 나쁠 것 없다" 가 아니다.**
    //     도달 못 하는 검사는 지키는 게 없으면서 지키는 것처럼 읽힌다.
    //
    //   지금 이 자리를 지키는 것은 저장소의 대조이고, 그것이 느슨해지면
    //   `tampering_the_stored_signer_is_stopped_by_the_store_before_planning`
    //   이 실패하며 알려 준다.

    // ── 3. Verified 뒤에만 필드를 읽는다 ─────────────────────────────
    let requirements = job_requirements_from_manifest(&verified, submitter_member)
        .map_err(|e| format!("PLAN_REFUSED: {e}"))?;

    // ── 4~5. 같은 DB 의 inventory 로 후보를 가린다 ───────────────────
    let snapshot = {
        let mut inventory = CoordinatorInventoryStore::open(control_db)
            .map_err(|e| format!("inventory store 를 열지 못했다({control_db}): {e}"))?;
        inventory
            .pool_snapshot(now_unix_ms)
            .map_err(|e| format!("pool snapshot 실패: {e}"))?
    };
    let report = evaluate_eligibility(
        &snapshot,
        &requirements,
        &Policy {
            maximum_snapshot_age_ms: max_snapshot_age_ms,
            silent_after_ms: None,
        },
    );

    if report.resolution == EligibilityResolution::NoEligibleCandidates {
        // ★ 상태를 바꾸지 않는다. `QUEUED` 는 "실행 가능한 계획이 있다"
        //   는 뜻인데 없기 때문이다. 대신 **왜** 떨어졌는지 말한다 —
        //   "후보가 없다" 만으로는 운영자가 무엇을 고칠지 모른다.
        let mut lines = vec![format!(
            "PLAN_REFUSED: 적격 노드가 없다 — {} 개 후보를 봤다",
            report.rejected.len()
        )];
        for rejected in &report.rejected {
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
        return Err(lines.join("\n"));
    }

    // ── 6. 계획이 실제로 있다. 이제 올린다 ──────────────────────────
    let plan_id = derive_plan_id(job_id, &binding.manifest_hash, &snapshot, &report);

    // ★ **이미 큐에 있으면 `PLANNING` 으로 되돌리지 않는다.** 재계획은 정상적인 일이다(운영자가 같은 명령을 두 번 돌린다) — 같은 `plan_id` 면
    //   멱등이고 다르면 `PlanConflict` 다(inventory 가 바뀌어 계획이 달라졌으면 옛 계획을 조용히 유지하지 않고 시끄럽게 실패한다).
    // ★★ 2026-09-25 (결함 402) — `SUBMITTED -> PLANNING -> QUEUED` 를 **한 트랜잭션**으로 한다. 전에는 두 커밋이라 둘째가 실패하면 PLANNING 에
    //   남았고, 그 사이 Manifest 가 만료되면 이 명령이 서명 검증에서 먼저 거부해 다시 돌려도 풀 수 없었다.
    // ★ 결함 424 · 428 (재검수 107 · 108) — 큐 진입 시각은 저장소가 **쓰기 잠금을 잡은 뒤** 읽는다. 명령 시작 때 읽은 값은 검증 · 스냅샷 · 잠금 대기만큼
    //   앞당겨졌고, 그것을 고치려 둔 max(시작, 지금)은 시계가 되돌아가면 순서를 뒤집었다. ★ 벽시계 역행 자체는 막지 못한다(결함 431 · 433 — 운영 조건: 실행 중 시계를 뒤로 되돌리지 않는다).
    let job = jobs
        .plan_and_enqueue_now(job_id, &plan_id)
        .map_err(|e| format!("PLAN_REFUSED: 계획 · 큐 진입 실패: {e}"))?;

    Ok(format!(
        "QUEUED job_id={job_id} plan_id={plan_id} state={:?} eligible={} rejected={}",
        job.state,
        report.eligible.len(),
        report.rejected.len()
    ))
}

/// 계획 식별자를 **계획 내용에서** 만든다.
///
/// ★ 같은 입력이면 같은 값이어야 한다 — `enqueue()` 는 재시도 시 같은
///   `plan_id` 를 요구하고 다르면 `PlanConflict` 를 낸다. 무작위 값을
///   쓰면 재시도 시 같은 `plan_id` 를 보장하지 못한다 — 이전 값과 다르면
///   `PlanConflict` 가 난다(재검수 33·34 — 전에는 "늘 충돌한다", 그 뒤엔 조건 없이
///   "충돌한다" 고 적었다).
///
/// ★ 그리고 **다른 계획이면 달라야 한다.** inventory 가 바뀌어 적격
///   노드 집합이 달라졌으면 그건 다른 계획이고, 재실행이
///   `PlanConflict` 로 시끄럽게 실패하는 것이 옳다 — 조용히 옛 계획을
///   유지하면 운영자가 새 노드가 반영됐다고 오인할 우려가 있다(재검수 33 —
///   전에는 "반영된 줄 안다" 고 운영자의 판단을 단정했다).
///
/// 길이 프리픽스를 붙여 인접 필드가 서로 스며들지 않게 한다(canonical
/// 인코딩과 같은 이유).
fn derive_plan_id(
    job_id: &str,
    manifest_hash: &[u8; 32],
    snapshot: &gputeer_scheduler::PoolSnapshot,
    report: &gputeer_scheduler::EligibilityReport,
) -> String {
    let mut input = Vec::new();
    let mut push = |bytes: &[u8]| {
        input.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        input.extend_from_slice(bytes);
    };
    push(b"gputeer/v1/plan-id");
    push(job_id.as_bytes());
    push(manifest_hash);

    // 적격 노드와 **그 노드를 본 inventory revision**. revision 이 빠지면
    // 같은 노드 집합의 다른 상태가 같은 계획으로 보인다.
    let revisions: BTreeMap<&str, u64> = snapshot
        .candidates
        .iter()
        .filter_map(|c| c.inventory_revision.map(|r| (c.node_id.as_str(), r)))
        .collect();
    for candidate in &report.eligible {
        push(candidate.node_id.as_bytes());
        push(
            &revisions
                .get(candidate.node_id.as_str())
                .copied()
                .unwrap_or(0)
                .to_le_bytes(),
        );
    }
    hex(&gputeer_protocol::canonical::blake3_256(&input))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as u64
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
