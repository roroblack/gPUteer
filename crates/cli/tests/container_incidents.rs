//! 컨테이너 사건 표식 — 2026-09-27 보수 규칙(재검수 121~124 합의).
//!
//! 사람이 봐야 하는 컨테이너 결과(시작 여부 · 정지 · 로그 · 컨테이너가 불확실)는 `container::run` 이 영속 표식으로 남긴다. 여기서는 그 위 —
//! **표식이 열려 있는 동안 Agent 가 새 작업을 받지 않고 기동하지 않는가**(남은 컨테이너 정리보다 먼저), 그리고 소유자가 **명시적으로**
//! 보고 해제할 수 있는가 — 를 실제 `gputeer` 프로세스로 잰다.

use std::path::Path;
use std::process::Command;

fn gputeer(args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_gputeer"))
        .args(args)
        .output()
        .expect("gputeer 를 띄우지 못했다");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), text)
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("utf-8 경로")
}

#[test]
fn an_open_incident_keeps_the_agent_from_starting_until_the_owner_clears_it() {
    let dir = tempfile::tempdir().unwrap();
    let node = dir.path().join("node");
    std::fs::create_dir_all(&node).unwrap();
    let checkpoint_root = node.join("checkpoints");
    let incidents = node.join("container-incidents");
    std::fs::create_dir_all(&incidents).unwrap();
    std::fs::write(
        incidents.join("gputeer-deadbeef.incident"),
        "container=gputeer-deadbeef\nkind=UNOBSERVED\ndetail=시험\n",
    )
    .unwrap();

    let seed = dir.path().join("node.seed");
    let (ok, keygen) = gputeer(&["keygen", "--out", path_str(&seed)]);
    assert!(ok, "{keygen}");
    let pubkey = keygen
        .lines()
        .find_map(|line| line.strip_prefix("PUBLIC_KEY "))
        .expect("keygen 이 공개키를 찍지 않았다")
        .trim()
        .to_string();
    let fence = dir.path().join("fence.sqlite3");
    // 런타임은 없는 실행 파일이다 — 관문이 없으면 남은 컨테이너 정리가 그것을 부르다 CONTAINER_LEFTOVERS_UNKNOWN 으로 끝난다.
    let missing_runtime = dir.path().join("no-such-runtime.exe");
    let agent = |checkpoint_root: &Path| {
        gputeer(&[
            "agent-stub",
            "--connect",
            "127.0.0.1:9",
            "--own-seed-file",
            path_str(&seed),
            "--peer-pubkey",
            &pubkey,
            "--coordinator-device-id",
            "01JCOORDINATORINCIDENT001",
            "--agent-device-id",
            "01JAGENTINCIDENT000000001",
            "--fence-db",
            path_str(&fence),
            "--checkpoint-root",
            path_str(checkpoint_root),
            "--container-runtime",
            path_str(&missing_runtime),
            "--container-runtime-kind",
            "docker",
        ])
    };

    // 1. 표식이 열려 있으면 기동하지 않는다 — 남은 컨테이너 정리까지 가지 않는다.
    let (ok, first) = agent(&checkpoint_root);
    assert!(!ok, "{first}");
    assert!(first.contains("CONTAINER_INCIDENT_OPEN"), "{first}");
    assert!(first.contains("gputeer-deadbeef.incident"), "{first}");
    assert!(
        !first.contains("CONTAINER_LEFTOVERS"),
        "관문보다 정리가 먼저 돌았다:\n{first}"
    );

    // 2. 소유자가 본다 — 열린 표식과 내용.
    let (ok, listed) = gputeer(&[
        "container-incidents",
        "--checkpoint-root",
        path_str(&checkpoint_root),
    ]);
    assert!(ok, "{listed}");
    assert!(listed.contains("CONTAINER_INCIDENTS open 1"), "{listed}");
    assert!(listed.contains("kind=UNOBSERVED"), "{listed}");

    // 3. 소유자가 명시적으로 해제한다.
    let (ok, cleared) = gputeer(&[
        "container-incidents",
        "--checkpoint-root",
        path_str(&checkpoint_root),
        "--clear-all",
    ]);
    assert!(ok, "{cleared}");
    assert!(
        cleared.contains("CONTAINER_INCIDENTS cleared 1"),
        "{cleared}"
    );
    assert!(std::fs::read_dir(&incidents).unwrap().next().is_none());

    // 4. 해제 뒤에는 관문을 지나 남은 컨테이너 정리로 간다(여기서는 런타임이 없어 거기서 멈춘다 — 관문이 풀렸다는 뜻).
    let (ok, second) = agent(&checkpoint_root);
    assert!(!ok, "{second}");
    assert!(!second.contains("CONTAINER_INCIDENT_OPEN"), "{second}");
    assert!(second.contains("CONTAINER_LEFTOVERS_UNKNOWN"), "{second}");
}

#[test]
fn clearing_needs_a_checkpoint_root_and_a_name() {
    let (ok, text) = gputeer(&["container-incidents"]);
    assert!(!ok && text.contains("--checkpoint-root"), "{text}");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("checkpoints");
    let (ok, text) = gputeer(&[
        "container-incidents",
        "--checkpoint-root",
        path_str(&root),
        "--clear",
    ]);
    assert!(!ok && text.contains("컨테이너 이름"), "{text}");
    // 표식 폴더가 없으면 열린 것이 0 이다(오류가 아니다).
    let (ok, text) = gputeer(&["container-incidents", "--checkpoint-root", path_str(&root)]);
    assert!(ok && text.contains("CONTAINER_INCIDENTS open 0"), "{text}");
}
