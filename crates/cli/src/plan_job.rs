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
//! 6  적격이 있으면 start_planning + enqueue(plan_id)
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

    // 저장 당시의 서명자와 지금 검증된 서명자가 같은가.
    //
    // ★ **이 분기는 테스트로 고정되지 않았다** — 뮤테이션 P2 로 통째로
    //   지워도 10건이 전부 통과한다. 숨기지 않고 적는다.
    //
    //   도달하기 어려운 이유가 있다. 검증된 서명자는 Manifest 안의
    //   `submitter_device_id` 에서 나오고, 저장된 값도 **반입 시점의 같은
    //   검증**에서 나왔다. 그러니 둘이 갈리려면 저장 뒤에 DB 행이
    //   손대져야 한다 — 즉 이건 **DB 변조 방어**이지 정상 경로의 관문이
    //   아니다. 그걸 재려면 SQL 로 행을 직접 고치는 테스트가 필요하고,
    //   그건 이 조각에서 만들지 않았다.
    //
    //   지우지 않는 이유는 비용이 0 이고 막는 것이 실재하기 때문이다.
    //   다만 **"이 관문이 지키고 있다" 고 말할 근거는 아직 없다.**
    if verified.signer_id() != binding.signer_id_at_submission {
        return Err(format!(
            "PLAN_REFUSED: 저장 당시 서명자({})와 지금 검증된 서명자({})가 다르다",
            binding.signer_id_at_submission,
            verified.signer_id()
        ));
    }

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

    // ★ **이미 큐에 있으면 `PLANNING` 으로 되돌리지 않는다.**
    //
    //   `start_planning()` 은 `SUBMITTED` 에서만 받고 `QUEUED -> PLANNING`
    //   을 거부한다 — 상태기계가 뒤로 가지 않는다. 그런데 재계획은
    //   정상적인 일이다(운영자가 같은 명령을 두 번 돌린다).
    //
    //   그래서 큐에 이미 있으면 그 단계를 건너뛰고 `enqueue()` 만 부른다.
    //   그쪽은 **같은 `plan_id` 면 멱등**이고 다르면 `PlanConflict` 를
    //   낸다 — inventory 가 바뀌어 계획이 달라졌으면 조용히 옛 계획을
    //   유지하지 않고 시끄럽게 실패하는 것이 옳다.
    let already_queued = jobs
        .get(job_id)
        .map_err(|e| format!("Job 조회 실패: {e}"))?
        .map(|job| job.state == gputeer_coordinator::job_store::JobState::Queued)
        .unwrap_or(false);
    if !already_queued {
        jobs.start_planning(job_id, now_unix_ms)
            .map_err(|e| format!("PLAN_REFUSED: PLANNING 진입 실패: {e}"))?;
    }
    let job = jobs
        .enqueue(job_id, &plan_id, now_unix_ms)
        .map_err(|e| format!("PLAN_REFUSED: QUEUED 진입 실패: {e}"))?;

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
///   쓰면 재시도가 늘 충돌한다.
///
/// ★ 그리고 **다른 계획이면 달라야 한다.** inventory 가 바뀌어 적격
///   노드 집합이 달라졌으면 그건 다른 계획이고, 재실행이
///   `PlanConflict` 로 시끄럽게 실패하는 것이 옳다 — 조용히 옛 계획을
///   유지하면 운영자는 새 노드가 반영된 줄 안다.
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
