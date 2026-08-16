//! 카오스 테스트용 체크포인트 writer.
//!
//! 인자: <root> <num_checkpoints> <files_per_ckpt> <file_bytes> <slow_ms>
//!
//! 매 체크포인트마다 데이터 파일들을 쓰고 매니페스트를 마지막에 쓴다.
//! `slow_ms` 로 쓰기 사이에 지연을 넣어 테스트가 임의 시점에 kill 할 수 있게 한다.
//! 각 체크포인트 확정 후 stdout 에 `COMMITTED <id> <step>` 을 출력한다.

use std::io::Write;
use std::path::PathBuf;

use gputeer_checkpoint::writer::{manifest_for, write_checkpoint};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 6 {
        eprintln!("usage: ckpt_writer <root> <n> <files> <bytes> <slow_ms>");
        std::process::exit(2);
    }
    let root = PathBuf::from(&a[1]);
    let n: u64 = a[2].parse().unwrap();
    let files_per: usize = a[3].parse().unwrap();
    let bytes: usize = a[4].parse().unwrap();
    let slow_ms: u64 = a[5].parse().unwrap();

    std::fs::create_dir_all(&root).unwrap();

    for i in 0..n {
        let step = (i + 1) * 100;
        let id = format!("ckpt-{step:08}");
        let files: Vec<(String, Vec<u8>)> = (0..files_per)
            .map(|f| {
                // step 마다 내용이 달라야 해시 검증이 의미를 갖는다
                let byte = ((step as usize + f) % 251) as u8;
                (format!("shard-{f}.bin"), vec![byte; bytes])
            })
            .collect();

        let m = manifest_for(&id, "job-chaos", "att-1", step, 42, &files);
        match write_checkpoint(&root, &m, &files, slow_ms) {
            Ok(_) => {
                println!("COMMITTED {id} {step}");
                let _ = std::io::stdout().flush();
            }
            Err(e) => {
                eprintln!("ERR {e}");
                std::process::exit(1);
            }
        }
    }
    println!("DONE");
}
