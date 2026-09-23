//! 실행 중 체크포인트 게시기 — 작업이 만든 체크포인트를 **공유 저장소**로 옮긴다(신뢰망 남은 일 E).
//!
//! # 작업과의 약속(워크로드 계약)
//!
//! ```text
//! GPUTEER_CHECKPOINT_DIR   작업이 체크포인트를 쓰는 폴더. 한 번의 체크포인트 = 하위 폴더 하나 `step-<숫자>`.
//!                          ★ 다 쓴 뒤에 그 이름으로 **바꿔야** 한다(예: `step-3.tmp` 에 쓰고 `step-3` 으로 rename).
//!                            게시기는 `step-<숫자>` 이름만 본다 — 쓰는 중인 폴더를 올리지 않기 위해서다
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

use std::collections::BTreeSet;
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

/// `out_dir` 에서 완성된 `step-<숫자>` 폴더를 찾아 **아직 안 올린 것**을 step 순으로 올린다.
pub fn publish_ready_steps(
    ctx: &PublishContext,
    out_dir: &Path,
    published: &mut BTreeSet<u64>,
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
        .filter(|(step, _)| !published.contains(step))
        .collect();
    ready.sort();
    for (step, dir) in ready {
        match publish_one(ctx, step, &dir, now_unix_ms) {
            Ok(checkpoint_id) => {
                println!(
                    "CHECKPOINT_PUBLISHED checkpoint_id={checkpoint_id} step={step} job_id={}",
                    ctx.job_id
                );
                published.insert(step);
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
    let committed = shared::publish_directory(
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
    let mut manifest = shared::to_unsigned_pb(&committed.manifest)?;
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
    handle: std::thread::JoinHandle<BTreeSet<u64>>,
    ctx: PublishContext,
    out_dir: PathBuf,
}

pub fn start(ctx: PublishContext, out_dir: PathBuf, interval_ms: u64) -> RunningPublisher {
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let thread_ctx = ctx.clone();
    let thread_dir = out_dir.clone();
    let handle = std::thread::spawn(move || {
        let mut published = BTreeSet::new();
        let interval = Duration::from_millis(interval_ms.max(50));
        'publish: loop {
            let due = std::time::Instant::now() + interval;
            while std::time::Instant::now() < due {
                if thread_stop.load(Ordering::SeqCst) {
                    break 'publish;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            publish_ready_steps(&thread_ctx, &thread_dir, &mut published, now_unix_ms());
        }
        published
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
    let mut published = match publisher.handle.join() {
        Ok(published) => published,
        Err(_) => {
            println!(
                "CHECKPOINT_PUBLISHER_PANICKED — 게시 스레드가 비정상 종료했다. 마지막 훑기만 한다"
            );
            BTreeSet::new()
        }
    };
    publish_ready_steps(
        &publisher.ctx,
        &publisher.out_dir,
        &mut published,
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
        let mut published = BTreeSet::new();
        publish_ready_steps(&ctx, &out, &mut published, 10);
        assert_eq!(published.iter().copied().collect::<Vec<_>>(), vec![1]);
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
        publish_ready_steps(&ctx, &out, &mut published, 20);
        assert_eq!(published.iter().copied().collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(
            shared::list_signed_manifests(&ctx.shared_root, "job-1")
                .unwrap()
                .len(),
            2
        );
    }
}
