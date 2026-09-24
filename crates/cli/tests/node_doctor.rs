//! `gputeer node-doctor` — 노드 준비 점검이 빠진 것을 **FAIL 로** 보여 주는가, 갖춘 것은 OK 인가.
//!
//! ★ GPU · 컨테이너 런타임이 없는 기계(개발 기계 · CI)에서도 돈다 — 그 둘은 WARN/FAIL 의 **모양**만 본다.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_gputeer"))
}

fn keygen(dir: &Path) -> PathBuf {
    let seed = dir.join("node.seed");
    let out = Command::new(cli_bin())
        .args(["keygen", "--out", seed.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    seed
}

fn doctor(args: &[&str]) -> (bool, String) {
    let out = Command::new(cli_bin())
        .arg("node-doctor")
        .args(args)
        .output()
        .unwrap();
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn line<'a>(report: &'a str, name: &str) -> &'a str {
    report
        .lines()
        .find(|l| l.starts_with(&format!("CHECK {name} ")))
        .unwrap_or_else(|| panic!("CHECK {name} 줄이 없다:\n{report}"))
}

#[test]
fn a_ready_node_passes_the_start_conditions() {
    let dir = tempfile::tempdir().unwrap();
    let seed = keygen(dir.path());
    let coordinator = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = coordinator.local_addr().unwrap().to_string();
    let shared = dir.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    let free_port = {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        probe.local_addr().unwrap().port()
    };
    let port = free_port.to_string();
    let node_dir = dir.path().join("node");
    let (_, report) = doctor(&[
        "--seed-file",
        seed.to_str().unwrap(),
        "--node-dir",
        node_dir.to_str().unwrap(),
        "--connect",
        &address,
        "--shared-checkpoint-root",
        shared.to_str().unwrap(),
        "--owner-panel-port",
        &port,
    ]);
    for name in [
        "seed",
        "node_dir",
        "coordinator",
        "shared_checkpoint_root",
        "owner_panel_port",
    ] {
        let l = line(&report, name);
        assert!(l.contains(" OK "), "{name} 가 OK 가 아니다:\n{report}");
    }
    // GPU 는 이 기계에 따라 OK(보임) 또는 WARN(NVML 없음) — FAIL 이 아니어야 한다(--gpu-pin 을 주지 않았다).
    assert!(!line(&report, "gpu").contains(" FAIL "), "{report}");
    assert!(node_dir.is_dir(), "노드 폴더를 만들지 않았다");
    assert_eq!(
        std::fs::read_dir(&node_dir).unwrap().count(),
        0,
        "점검이 흔적을 남겼다"
    );
    assert!(report.contains("NODE_DOCTOR ok="), "{report}");
}

#[test]
fn what_would_stop_the_agent_is_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let bad_seed = dir.path().join("bad.seed");
    std::fs::write(&bad_seed, "not-a-seed").unwrap();
    // 닫힌 포트 — 잠깐 열었다 닫는다.
    let closed = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().to_string()
    };
    let busy = TcpListener::bind("127.0.0.1:0").unwrap();
    let busy_port = busy.local_addr().unwrap().port().to_string();
    let missing_runtime = dir.path().join("no-such-runtime");
    let (success, report) = doctor(&[
        "--seed-file",
        bad_seed.to_str().unwrap(),
        "--node-dir",
        dir.path().join("node").to_str().unwrap(),
        "--connect",
        &closed,
        "--shared-checkpoint-root",
        dir.path().join("not-mounted").to_str().unwrap(),
        "--owner-panel-port",
        &busy_port,
        "--container-runtime",
        missing_runtime.to_str().unwrap(),
        "--container-runtime-kind",
        "podman",
    ]);
    assert!(!success, "FAIL 이 있는데 성공으로 끝났다:\n{report}");
    for name in [
        "seed",
        "coordinator",
        "shared_checkpoint_root",
        "owner_panel_port",
        "container_runtime",
    ] {
        assert!(
            line(&report, name).contains(" FAIL "),
            "{name} 가 FAIL 이 아니다:\n{report}"
        );
    }
    assert!(line(&report, "node_dir").contains(" OK "), "{report}");
    drop(busy);
}

#[test]
fn a_missing_required_option_is_refused() {
    let (success, report) = doctor(&["--seed-file", "x"]);
    assert!(!success);
    assert!(report.contains("NODE_DOCTOR_ARGS"), "{report}");
    let (success, report) = doctor(&[
        "--seed-file",
        "x",
        "--node-dir",
        "y",
        "--connect",
        "127.0.0.1:1",
        "--container-runtime",
        "podman",
    ]);
    assert!(!success);
    assert!(report.contains("함께 준다"), "{report}");
}
