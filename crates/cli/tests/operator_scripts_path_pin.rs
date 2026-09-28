//! 결함 554 — 운영자 셸 스크립트(`admit-node.sh` · `refresh-inventory.sh` · `make-invite.sh`)가 **root 로 돌 때** 부른 계정의 PATH 에 심어 둔
//! 명령(`dirname` · `sed` …)을 실행하지 않는가.
//!
//! 세 스크립트는 `set -eu` 바로 뒤, `/usr/bin/id -u` 가 0 이면 PATH 를 시스템 폴더로 고정한다. 이 시험은 PATH 앞에 가짜 명령 폴더를 두고
//!   · root 로 돌리면 — 가짜가 **한 번도** 불리지 않는다(표식 파일이 없다)
//!   · 대조군: root 가 아니면 — 가짜가 불린다(가로채기가 실제로 가능한 입력임을 보인다 · 시험이 공허하지 않다)
//! 를 본다.
//!
//! root 로 도는 방법:
//!   이미 root(예 WSL 기본 계정)   그대로 root · 대조군은 `setpriv` 로 nobody(65534)로 내려서
//!   root 가 아님(예 CI)           `unshare -r`(사용자 네임스페이스 안의 uid 0) · 대조군은 그대로
//! 도구가 없거나 커널이 막으면 `ENVIRONMENT-BLOCKED` 를 찍고 끝난다 — 통과로 세지 않는다(`CLAUDE.md` §4).
#![cfg(target_os = "linux")]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 가짜로 심는 명령 — 스크립트가 PATH 고정 뒤 처음 부르는 것(`dirname` · `sed`)과 그 뒤에 흔히 부르는 것.
const PLANTED: &[&str] = &[
    "dirname", "sed", "tail", "tr", "date", "mkdir", "grep", "head", "cat", "cut", "mv", "cp", "rm",
];

fn install_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/trusted-party/install")
}

fn text(command: &mut Command) -> Option<String> {
    let output = command.output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// 어떻게 root 로 · root 가 아니게 돌릴지. 못 정하면 `Err(사유)`.
enum Mode {
    AlreadyRoot,
    UserNamespace,
}

fn mode() -> Result<Mode, String> {
    let uid = text(Command::new("/usr/bin/id").arg("-u")).ok_or("id -u 를 읽지 못했다")?;
    if uid == "0" {
        // 대조군을 nobody 로 내릴 수 있어야 한다 — "그 일이 실제로 된다" 로 판정한다.
        match text(Command::new("setpriv").args([
            "--reuid=65534",
            "--regid=65534",
            "--clear-groups",
            "/usr/bin/id",
            "-u",
        ])) {
            Some(id) if id == "65534" => Ok(Mode::AlreadyRoot),
            other => Err(format!(
                "root 인데 setpriv 로 nobody 로 내리지 못했다({other:?})"
            )),
        }
    } else {
        match text(Command::new("unshare").args(["-r", "/usr/bin/id", "-u"])) {
            Some(id) if id == "0" => Ok(Mode::UserNamespace),
            other => Err(format!("unshare -r 로 uid 0 을 얻지 못했다({other:?})")),
        }
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    evil: PathBuf,
    marker: PathBuf,
}

/// 가짜 명령 폴더 · 환경 파일 · 가입 파일을 만든다. nobody 도 읽고 표식을 쓸 수 있게 권한을 연다(시험 폴더 안에서만).
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
    let evil = root.join("evil-bin");
    std::fs::create_dir(&evil).unwrap();
    std::fs::set_permissions(&evil, std::fs::Permissions::from_mode(0o755)).unwrap();
    let marker = root.join("planted-was-run");
    for name in PLANTED {
        let path = evil.join(name);
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\necho \"{name}\" >> '{}'\nexit 1\n",
                marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let etc = root.join("etc");
    std::fs::create_dir(&etc).unwrap();
    std::fs::set_permissions(&etc, std::fs::Permissions::from_mode(0o777)).unwrap();
    std::fs::write(
        etc.join("gputeer.env"),
        "GPUTEER_BIN=\nGPUTEER_CONTROL_DB=\n",
    )
    .unwrap();
    std::fs::write(root.join("join.json"), "{}\n").unwrap();
    for file in [etc.join("gputeer.env"), root.join("join.json")] {
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    Fixture {
        _dir: dir,
        root,
        evil,
        marker,
    }
}

fn arguments(script: &str, f: &Fixture) -> Vec<String> {
    let env = f.root.join("etc/gputeer.env").display().to_string();
    match script {
        "admit-node.sh" => vec![
            "--env-file".into(),
            env,
            "--join-file".into(),
            f.root.join("join.json").display().to_string(),
        ],
        "refresh-inventory.sh" => vec!["--env-file".into(), env],
        "make-invite.sh" => vec![
            "--env-file".into(),
            env,
            "--out".into(),
            f.root.join("invite.env").display().to_string(),
        ],
        other => panic!("모르는 스크립트 {other}"),
    }
}

/// 스크립트를 돌린다 — `as_root` 면 root 로, 아니면 root 가 아닌 계정으로. PATH 맨 앞에 가짜 명령 폴더를 둔다. 스크립트의 종료 코드는 보지 않는다
/// (가짜 · 빈 설정 때문에 어차피 실패한다) — 보는 것은 가짜가 불렸는가뿐이다.
fn run(script: &str, f: &Fixture, mode: &Mode, as_root: bool) -> bool {
    let _ = std::fs::remove_file(&f.marker);
    let path = format!("{}:/usr/sbin:/usr/bin:/sbin:/bin", f.evil.display());
    let script_path = install_dir().join(script);
    let mut command = match (mode, as_root) {
        (Mode::AlreadyRoot, true) | (Mode::UserNamespace, false) => Command::new("/bin/sh"),
        (Mode::AlreadyRoot, false) => {
            let mut c = Command::new("setpriv");
            c.args([
                "--reuid=65534",
                "--regid=65534",
                "--clear-groups",
                "/bin/sh",
            ]);
            c
        }
        (Mode::UserNamespace, true) => {
            let mut c = Command::new("unshare");
            c.args(["-r", "/bin/sh"]);
            c
        }
    };
    command
        .arg(&script_path)
        .args(arguments(script, f))
        .env_clear()
        .env("PATH", path)
        .env("HOME", &f.root)
        .current_dir(&f.root);
    let output = command.output().expect("스크립트를 띄우지 못했다");
    let ran = f.marker.exists();
    if !as_root && !ran {
        eprintln!(
            "대조군 출력({script}): {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    ran
}

#[test]
fn operator_scripts_run_as_root_never_execute_commands_planted_in_the_callers_path() {
    let mode = match mode() {
        Ok(mode) => mode,
        Err(why) => {
            println!("ENVIRONMENT-BLOCKED: 결함 554 의 root 경로를 측정하지 않았다 — {why}");
            return;
        }
    };
    for script in ["admit-node.sh", "refresh-inventory.sh", "make-invite.sh"] {
        let f = fixture();
        // 대조군 — root 가 아니면 PATH 고정이 없어 가짜가 불린다(이 입력이 실제로 가로챌 수 있다).
        assert!(
            run(script, &f, &mode, false),
            "{script}: root 가 아닌데도 가짜 명령이 불리지 않았다 — 시험 입력이 가로채기를 재현하지 못한다(공허한 시험)"
        );
        // root 로 돌면 가짜가 한 번도 불리지 않는다.
        assert!(
            !run(script, &f, &mode, true),
            "{script}: root 로 돌았는데 부른 계정의 PATH 에 심은 명령이 실행됐다: {:?}",
            std::fs::read_to_string(&f.marker).unwrap_or_default()
        );
    }
}
