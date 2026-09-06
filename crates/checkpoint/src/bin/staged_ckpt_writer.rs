//! 확정 절차(`commit.rs`)의 **프로세스 kill** 테스트용 writer.
//!
//! 인자: `<root> <checkpoint_id> <n_files> <file_bytes> <abort_after_files>`
//!
//! `abort_after_files`
//! ```text
//! -1   중단하지 않는다 (정상 확정까지 간다)
//!  0   stage 하기 전에 죽는다
//!  k   k 번째 데이터 파일을 확정한 직후에 죽는다
//!       (k == n_files 이면 데이터는 전부 확정되고 매니페스트 직전에 죽는다)
//! ```
//!
//! stdout
//! ```text
//! STAGED <논리 이름> <저장 이름>
//! DIED_AFTER <k>
//! COMMITTED <checkpoint_id>
//! ```
//!
//! ★ `exit()` 는 destructor 를 돌리지 않는다 — 그래서 "죽었다" 를 충실히
//!   흉내 낸다. `StagedCheckpoint` 는 `Drop` 에서 아무것도 쓰지 않으므로
//!   `abort()` 와 관측 가능한 차이가 없고, Windows 에서 크래시 대화상자를
//!   띄우지 않아 테스트가 멈추지 않는다. stdout 은 exit 전에 직접 flush 한다.

use std::io::Write;
use std::path::PathBuf;
use std::process::exit;

use gputeer_checkpoint::commit::{ManifestMeta, StagedCheckpoint};

/// 죽었다는 뜻의 종료 코드. 정상 종료(0)·인자 오류(2)·확정 실패(1)와 겹치지 않는다.
const DIED_EXIT_CODE: i32 = 70;

fn die(after: usize) -> ! {
    println!("DIED_AFTER {after}");
    let _ = std::io::stdout().flush();
    exit(DIED_EXIT_CODE);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 6 {
        eprintln!("usage: staged_ckpt_writer <root> <id> <n_files> <bytes> <abort_after_files>");
        exit(2);
    }

    let root = PathBuf::from(&args[1]);
    let checkpoint_id = args[2].clone();
    let n_files: usize = args[3].parse().expect("n_files");
    let bytes: usize = args[4].parse().expect("bytes");
    let abort_after: i64 = args[5].parse().expect("abort_after_files");

    let mut session = match StagedCheckpoint::begin(&root, &checkpoint_id) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("ERR begin: {error}");
            exit(1);
        }
    };

    if abort_after == 0 {
        die(0);
    }

    for index in 0..n_files {
        let logical = format!("shard-{index}.bin");
        let data = vec![((index + 1) % 251) as u8; bytes];

        match session.stage(&logical, &data) {
            Ok(file) => {
                println!("STAGED {} {}", file.logical_name, file.stored_name);
                let _ = std::io::stdout().flush();
            }
            Err(error) => {
                eprintln!("ERR stage {logical}: {error}");
                exit(1);
            }
        }

        if abort_after >= 0 && abort_after as usize == index + 1 {
            die(index + 1);
        }
    }

    let meta = ManifestMeta {
        job_id: "job-staged".to_string(),
        attempt_id: "att-1".to_string(),
        step: 100,
        fence_epoch: 42,
        producer_node_id: "node-staged".to_string(),
        created_at_unix_ms: 0,
    };

    match session.commit(&meta) {
        Ok(_) => {
            println!("COMMITTED {checkpoint_id}");
            let _ = std::io::stdout().flush();
        }
        Err(error) => {
            eprintln!("ERR commit: {error}");
            exit(1);
        }
    }
}
