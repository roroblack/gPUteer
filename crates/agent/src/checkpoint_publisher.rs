//! 실행 중 체크포인트 게시기 — 작업이 만든 체크포인트를 **공유 저장소**로 옮긴다(신뢰망 남은 일 E).
//!
//! # 작업과의 약속(워크로드 계약)
//!
//! ```text
//! GPUTEER_CHECKPOINT_DIR   작업이 체크포인트를 쓰는 폴더. 한 번의 체크포인트 = 하위 폴더 하나 `step-<숫자>`.
//!                          ★ 다 쓴 뒤에 그 이름으로 **바꿔야** 한다(예: `step-3.tmp` 에 쓰고 `step-3` 으로 rename).
//!                            게시기는 `step-<숫자>` 이름만 본다 — 쓰는 중인 폴더를 올리지 않기 위해서다
//!                          ★★ 2026-09-23 (결함 224 · 검수 75) — 이 약속은 **강제되지 않는다.** 게시기는 작업이 쓰기를 끝냈는지
//!                            알 길이 없다. 그래서 실행 중에는 같은 step 폴더가 **두 번 연속 같은 모습**(이름 · 크기 · 수정 시각)일
//!                            때만 올린다 — 완화이지 보장이 아니다(훑기 간격보다 오래 멈췄다 이어 쓰는 작업은 여전히 잘린다).
//!                            작업이 끝난 뒤의 마지막 훑기는 기다리지 않는다(더 쓰는 쪽이 없다)
//!                          폴더 안에는 파일만 둔다(하위 폴더 금지 — shared 모듈이 거부한다)
//! GPUTEER_RESUME_DIR       이어서 시작할 때만 있다. 공유 저장소에서 검증한 뒤 되살린 파일들이 있다
//! GPUTEER_JOB_ID · GPUTEER_ATTEMPT_ID
//! ```
//!
//! # 게시 한 번
//!
//! ```text
//! 1  step-N 의 파일을 공유 저장소에 확정한다(StagedCheckpoint — write-once · fsync · manifest.json 마지막)
//! 2  그 매니페스트를 proto 로 옮겨 **이 노드의 키로 서명**한다(producer_node_id = 이 Agent)
//! 3  서명된 매니페스트를 `<checkpoint_id>.checkpoint.pb` 로 write-once
//! ```
//!
//! 2·3 이 있어야 다른 노드가 "누가 만든 체크포인트인가" 를 검증할 수 있다.
//!
//! # 하지 않는 것
//!
//! ```text
//! Coordinator 에 알리기   wire 로 보내지 않는다. Coordinator 가 장애를 판정할 때 공유 저장소를 직접 읽는다
//!                        (새 세션 · 새 계약이 필요 없다). 그래서 Coordinator 도 같은 공유 저장소를 봐야 한다
//! 오래된 것 지우기        지우지 않는다. 보존 · 정리 정책은 아직 없다(알려진 공백)
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use gputeer_checkpoint::commit::ManifestMeta;
use gputeer_checkpoint::shared;
use gputeer_crypto::{sign, SigningKey};
use prost::Message;

/// 게시에 필요한 사실 — 전부 **검증된 Grant/Lease** 에서 온다.
#[derive(Clone)]
pub struct PublishContext {
    pub shared_root: PathBuf,
    pub job_id: String,
    pub attempt_id: String,
    pub fence_epoch: u64,
    pub node_id: String,
    pub signing_key: SigningKey,
}

fn step_of(name: &str) -> Option<u64> {
    let digits = name.strip_prefix("step-")?;
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // ★ 결함 242 · 255 (재검수 81 · 83) — 앞자리 0(`step-01`)은 **받는다**(한때 거부했더니 그렇게 쓰던 작업의 체크포인트가 경고 없이 사라졌다).
    //   같은 숫자의 폴더가 둘이면(`step-01` · `step-1`) 둘 다 건너뛰고 크게 알린다 — `publish_ready_steps` 가 가른다.
    digits.parse().ok()
}

/// 이 시도의 step N 체크포인트 이름. 시도가 다르면 이름이 다르다 — 이어받은 시도가 같은 step 을 다시 써도 겹치지 않는다.
pub fn checkpoint_id_for(attempt_id: &str, step: u64) -> String {
    format!("{attempt_id}-step-{step}")
}

/// 폴더의 모습 — 파일 이름 · 크기 · 수정 시각. 두 번 연속 같으면 "쓰기가 멈췄다" 로 본다(결함 224 완화).
type Fingerprint = Vec<(String, u64, Option<std::time::SystemTime>)>;

fn fingerprint(dir: &Path) -> Option<Fingerprint> {
    let mut out: Fingerprint = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| {
            let meta = entry.metadata().ok();
            (
                entry.file_name().to_string_lossy().to_string(),
                meta.as_ref().map_or(0, |m| m.len()),
                meta.and_then(|m| m.modified().ok()),
            )
        })
        .collect();
    out.sort();
    Some(out)
}

/// 게시기의 기억 — 올린 step 과, 아직 안 올린 step 의 직전 모습.
#[derive(Default)]
pub struct PublishState {
    pub published: BTreeSet<u64>,
    seen: BTreeMap<u64, Fingerprint>,
    conflicts_reported: BTreeSet<u64>,
}

/// `out_dir` 에서 완성된 `step-<숫자>` 폴더를 찾아 **아직 안 올린 것**을 step 순으로 올린다.
///
/// `require_stable` 이면 직전 훑기와 모습이 같은 폴더만 올린다(실행 중). 작업이 끝난 뒤의 마지막 훑기는 `false` 로 부른다.
pub fn publish_ready_steps(
    ctx: &PublishContext,
    out_dir: &Path,
    state: &mut PublishState,
    require_stable: bool,
    now_unix_ms: u64,
) {
    let Ok(entries) = std::fs::read_dir(out_dir) else {
        return;
    };
    let all: Vec<(u64, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .filter_map(|entry| {
            step_of(&entry.file_name().to_string_lossy()).map(|step| (step, entry.path()))
        })
        .collect();
    let mut ready: Vec<(u64, PathBuf)> = Vec::new();
    for (step, dir) in &all {
        if state.published.contains(step) {
            continue;
        }
        if all.iter().filter(|(other, _)| other == step).count() > 1 {
            if state.conflicts_reported.insert(*step) {
                println!(
                    "CHECKPOINT_STEP_NAME_CONFLICT step={step} job_id={} — 같은 번호의 폴더가 여럿이다(예: step-01 · step-1). 어느 쪽도 올리지 않는다",
                    ctx.job_id
                );
            }
            continue;
        }
        ready.push((*step, dir.clone()));
    }
    ready.sort();
    for (step, dir) in ready {
        if require_stable {
            let Some(now) = fingerprint(&dir) else {
                continue;
            };
            let previous = state.seen.insert(step, now.clone());
            if previous.as_ref() != Some(&now) {
                // 처음 봤거나 그새 바뀌었다 — 다음 훑기에서 같으면 올린다. 오류가 아니라 기다림이다.
                continue;
            }
        }
        match publish_one(ctx, step, &dir, now_unix_ms) {
            Ok(checkpoint_id) => {
                println!(
                    "CHECKPOINT_PUBLISHED checkpoint_id={checkpoint_id} step={step} job_id={}",
                    ctx.job_id
                );
                state.published.insert(step);
                state.seen.remove(&step);
            }
            Err(error) => {
                // ★ 조용히 넘기지 않는다 — 이 step 은 이어가기에 쓸 수 없다는 사실을 남긴다.
                //   다음 회차에 다시 시도한다(작업이 아직 쓰는 중이었을 수 있다).
                println!("CHECKPOINT_PUBLISH_FAILED step={step} detail={error}");
            }
        }
    }
}

fn publish_one(
    ctx: &PublishContext,
    step: u64,
    dir: &Path,
    now_unix_ms: u64,
) -> Result<String, String> {
    let checkpoint_id = checkpoint_id_for(&ctx.attempt_id, step);
    // ★ 결함 223 — 데이터는 확정됐는데 서명만 빠진 체크포인트는 되살려 서명한다(전에는 영영 서명되지 않았다).
    let committed = shared::publish_or_recover(
        &ctx.shared_root,
        &ctx.job_id,
        &checkpoint_id,
        dir,
        &ManifestMeta {
            job_id: ctx.job_id.clone(),
            attempt_id: ctx.attempt_id.clone(),
            step,
            fence_epoch: ctx.fence_epoch,
            producer_node_id: ctx.node_id.clone(),
            created_at_unix_ms: now_unix_ms,
        },
    )?;
    let mut manifest = shared::to_unsigned_pb(&committed)?;
    manifest.producer_signature = sign(&ctx.signing_key, &manifest).to_vec();
    shared::write_signed_manifest(
        &ctx.shared_root,
        &ctx.job_id,
        &checkpoint_id,
        &manifest.encode_to_vec(),
    )?;
    Ok(checkpoint_id)
}

/// 실행하는 동안 도는 게시 스레드.
pub struct RunningPublisher {
    stop: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<PublishState>,
    ctx: PublishContext,
    out_dir: PathBuf,
}

pub fn start(ctx: PublishContext, out_dir: PathBuf, interval_ms: u64) -> RunningPublisher {
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let thread_ctx = ctx.clone();
    let thread_dir = out_dir.clone();
    let handle = std::thread::spawn(move || {
        let mut state = PublishState::default();
        let interval = Duration::from_millis(interval_ms.max(50));
        'publish: loop {
            let due = std::time::Instant::now() + interval;
            while std::time::Instant::now() < due {
                if thread_stop.load(Ordering::SeqCst) {
                    break 'publish;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            publish_ready_steps(&thread_ctx, &thread_dir, &mut state, true, now_unix_ms());
        }
        state
    });
    RunningPublisher {
        stop,
        handle,
        ctx,
        out_dir,
    }
}

/// 작업이 끝난 뒤 — 스레드를 세우고 **마지막으로 몇 번 더** 훑는다(끝나기 직전에 쓴 체크포인트를 올린다).
///
/// ★ 2026-09-24 (결함 240 · 재검수 81) — 전에는 마지막 훑기가 **한 번**이었다. 그때 서명 매니페스트 쓰기만 실패하면(NAS 끊김 · 디스크 참)
///   다시 시도할 기회가 없었고, 곧 작업 폴더가 지워져 그 체크포인트는 이어가기에 영영 못 쓰였다. 이제 몇 번 다시 해 보고,
///   그래도 못 올린 step 은 **CHECKPOINT_PUBLISH_LOST** 로 크게 남긴다 — "놓치지 않는다" 를 보장하지는 못한다(공유 저장소가 계속
///   죽어 있으면 잃는다).
pub fn finish(publisher: RunningPublisher) {
    publisher.stop.store(true, Ordering::SeqCst);
    let mut state = match publisher.handle.join() {
        Ok(state) => state,
        Err(_) => {
            println!(
                "CHECKPOINT_PUBLISHER_PANICKED — 게시 스레드가 비정상 종료했다. 마지막 훑기만 한다"
            );
            PublishState::default()
        }
    };
    // 작업이 끝났다 — 더 쓰는 쪽이 없으므로 안정을 기다리지 않는다.
    // ★ 결함 252 (재검수 83) — 0 · 0.5 · 1초 세 번은 1.1초 끊김에도 졌다. 약 15초까지 늘린다(0 · 0.5 · 1 · 2 · 4 · 8초).
    //   그래도 못 올리면 잃는다 — 작업 폴더는 작업이 끝나면 지운다(§0.5 · 남의 PC 에 남의 데이터를 두지 않는다).
    const BACKOFF_MS: [u64; 5] = [500, 1_000, 2_000, 4_000, 8_000];
    const FINAL_TRIES: u32 = BACKOFF_MS.len() as u32 + 1;
    for attempt in 1..=FINAL_TRIES {
        publish_ready_steps(
            &publisher.ctx,
            &publisher.out_dir,
            &mut state,
            false,
            now_unix_ms(),
        );
        // 목록을 못 읽으면 "남은 것 없음" 이 아니라 "모른다" 다 — 다시 해 본다(결함 252).
        let left = unpublished_steps(&publisher.out_dir, &state);
        if matches!(&left, Ok(left) if left.is_empty()) {
            return;
        }
        if attempt == FINAL_TRIES {
            match left {
                Ok(left) => {
                    for step in left {
                        println!(
                            "CHECKPOINT_PUBLISH_LOST step={step} job_id={} — {FINAL_TRIES} 번(약 15초) 해 봤지만 공유 저장소에 서명하지 못한 채 끝난다(이 step 에서는 이어갈 수 없다)",
                            publisher.ctx.job_id
                        );
                    }
                }
                Err(error) => println!(
                    "CHECKPOINT_PUBLISH_LOST step=? job_id={} — 작업 출력 폴더를 읽지 못해 무엇을 못 올렸는지도 모른다: {error}",
                    publisher.ctx.job_id
                ),
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(
            BACKOFF_MS[(attempt as usize - 1).min(BACKOFF_MS.len() - 1)],
        ));
    }
}

/// 아직 못 올린 `step-<숫자>` 들(이름이 겹쳐 건너뛴 것도 — 올리지 못했다). 폴더가 아예 없으면 빈 목록, 읽기 오류는 오류다.
fn unpublished_steps(out_dir: &Path, state: &PublishState) -> Result<Vec<u64>, String> {
    let entries = match std::fs::read_dir(out_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let mut left: Vec<u64> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .filter_map(|entry| step_of(&entry.file_name().to_string_lossy()))
        .filter(|step| !state.published.contains(step))
        .collect();
    left.sort();
    left.dedup();
    Ok(left)
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_complete_step_directories_are_recognised() {
        assert_eq!(step_of("step-3"), Some(3));
        assert_eq!(
            step_of("step-3.tmp"),
            None,
            "쓰는 중인 폴더를 올리면 안 된다"
        );
        assert_eq!(step_of("step-"), None);
        assert_eq!(step_of("stepx-3"), None);
        assert_eq!(step_of("step--3"), None);
        // 결함 255 — 앞자리 0 은 받는다(같은 번호가 둘이면 게시에서 가른다).
        assert_eq!(step_of("step-01"), Some(1));
        assert_eq!(step_of("step-0"), Some(0));
    }

    /// 완성된 step 만 올리고, 서명된 매니페스트를 남기며, 두 번 올리지 않는다.
    #[test]
    fn ready_steps_are_published_once_with_a_signed_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("out");
        std::fs::create_dir_all(out.join("step-1")).unwrap();
        std::fs::write(out.join("step-1").join("state"), b"1").unwrap();
        std::fs::create_dir_all(out.join("step-2.tmp")).unwrap();
        std::fs::write(out.join("step-2.tmp").join("state"), b"2").unwrap();
        let key = SigningKey::from_bytes(&[5u8; 32]);
        let ctx = PublishContext {
            shared_root: temp.path().join("shared"),
            job_id: "job-1".into(),
            attempt_id: "attempt-1".into(),
            fence_epoch: 4,
            node_id: "node-a".into(),
            signing_key: key.clone(),
        };
        let mut state = PublishState::default();
        publish_ready_steps(&ctx, &out, &mut state, false, 10);
        assert_eq!(state.published.iter().copied().collect::<Vec<_>>(), vec![1]);
        let listed = shared::list_signed_manifests(&ctx.shared_root, "job-1").unwrap();
        assert_eq!(listed.len(), 1);
        let manifest =
            gputeer_protocol::pb::CheckpointManifest::decode(listed[0].1.as_slice()).unwrap();
        assert_eq!(manifest.producer_node_id, "node-a");
        assert_eq!(manifest.fence_epoch, 4);
        assert!(!manifest.producer_signature.is_empty());
        shared::verify_on_disk(&ctx.shared_root, &manifest).unwrap();

        // 두 번째 훑기 — 이미 올린 것은 다시 올리지 않고, 이제 완성된 step-2 만 올린다.
        std::fs::rename(out.join("step-2.tmp"), out.join("step-2")).unwrap();
        publish_ready_steps(&ctx, &out, &mut state, false, 20);
        assert_eq!(
            state.published.iter().copied().collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            shared::list_signed_manifests(&ctx.shared_root, "job-1")
                .unwrap()
                .len(),
            2
        );
    }

    fn ctx_in(temp: &Path) -> PublishContext {
        PublishContext {
            shared_root: temp.join("shared"),
            job_id: "job-1".into(),
            attempt_id: "attempt-1".into(),
            fence_epoch: 4,
            node_id: "node-a".into(),
            signing_key: SigningKey::from_bytes(&[5u8; 32]),
        }
    }

    /// 결함 224 — 실행 중에는 두 번 연속 같은 모습일 때만 올린다. 그 사이에 파일이 늘면 기다린다.
    #[test]
    fn a_step_folder_still_being_written_waits_until_it_stops_changing() {
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("out");
        std::fs::create_dir_all(out.join("step-7")).unwrap();
        std::fs::write(out.join("step-7").join("model.bin"), b"m").unwrap();
        let ctx = ctx_in(temp.path());
        let mut state = PublishState::default();
        publish_ready_steps(&ctx, &out, &mut state, true, 10);
        assert!(state.published.is_empty(), "처음 본 폴더를 바로 올렸다");
        // 작업이 아직 쓰는 중이다 — 파일이 늘었다.
        std::fs::write(out.join("step-7").join("optimizer.bin"), b"o").unwrap();
        publish_ready_steps(&ctx, &out, &mut state, true, 20);
        assert!(state.published.is_empty(), "바뀌는 중인 폴더를 올렸다");
        // 멈췄다 — 이번에는 올린다. 두 파일이 다 들어 있다.
        publish_ready_steps(&ctx, &out, &mut state, true, 30);
        assert_eq!(state.published.iter().copied().collect::<Vec<_>>(), vec![7]);
        let listed = shared::list_signed_manifests(&ctx.shared_root, "job-1").unwrap();
        let manifest =
            gputeer_protocol::pb::CheckpointManifest::decode(listed[0].1.as_slice()).unwrap();
        assert_eq!(manifest.files.len(), 2, "잘린 체크포인트를 올렸다");
    }

    /// 결함 223 — 데이터는 확정됐는데 서명 매니페스트가 없으면, 다음 훑기가 되살려 서명한다.
    #[test]
    fn a_checkpoint_whose_signature_was_lost_is_signed_on_the_next_scan() {
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("out");
        std::fs::create_dir_all(out.join("step-1")).unwrap();
        std::fs::write(out.join("step-1").join("state"), b"1").unwrap();
        let ctx = ctx_in(temp.path());
        // 데이터만 확정된 상태(서명 매니페스트 쓰기가 실패했다)를 만든다.
        shared::publish_directory(
            &ctx.shared_root,
            "job-1",
            &checkpoint_id_for("attempt-1", 1),
            &out.join("step-1"),
            &ManifestMeta {
                job_id: "job-1".into(),
                attempt_id: "attempt-1".into(),
                step: 1,
                fence_epoch: 4,
                producer_node_id: "node-a".into(),
                created_at_unix_ms: 5,
            },
        )
        .unwrap();
        assert!(shared::list_signed_manifests(&ctx.shared_root, "job-1")
            .unwrap()
            .is_empty());
        let mut state = PublishState::default();
        publish_ready_steps(&ctx, &out, &mut state, false, 10);
        assert_eq!(state.published.iter().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(
            shared::list_signed_manifests(&ctx.shared_root, "job-1")
                .unwrap()
                .len(),
            1,
            "서명이 빠진 체크포인트를 되살리지 못했다"
        );
    }

    /// 결함 242 · 255 — 같은 번호의 폴더가 둘이면 어느 쪽도 올리지 않는다. 하나뿐이면 앞자리 0 이어도 올린다.
    #[test]
    fn two_folders_with_the_same_step_number_are_both_held_back() {
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("out");
        for name in ["step-01", "step-1"] {
            std::fs::create_dir_all(out.join(name)).unwrap();
            std::fs::write(out.join(name).join("state"), name.as_bytes()).unwrap();
        }
        std::fs::create_dir_all(out.join("step-02")).unwrap();
        std::fs::write(out.join("step-02").join("state"), b"2").unwrap();
        let ctx = ctx_in(temp.path());
        let mut state = PublishState::default();
        publish_ready_steps(&ctx, &out, &mut state, false, 10);
        assert_eq!(
            state.published.iter().copied().collect::<Vec<_>>(),
            vec![2],
            "겹친 번호를 올렸거나 step-02 를 못 올렸다"
        );
        assert_eq!(unpublished_steps(&out, &state).unwrap(), vec![1]);
    }
}
