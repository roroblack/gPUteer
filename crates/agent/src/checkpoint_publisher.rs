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
    let mut ready: Vec<(u64, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .filter_map(|entry| {
            step_of(&entry.file_name().to_string_lossy()).map(|step| (step, entry.path()))
        })
        .filter(|(step, _)| !state.published.contains(step))
        .collect();
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

/// 작업이 끝난 뒤 — 스레드를 세우고 **마지막으로 한 번 더** 훑는다(끝나기 직전에 쓴 체크포인트를 놓치지 않는다).
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
    publish_ready_steps(
        &publisher.ctx,
        &publisher.out_dir,
        &mut state,
        false,
        now_unix_ms(),
    );
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
}
