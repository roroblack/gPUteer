//! `gputeer selftest` — 지금 구현된 계층을 끝에서 끝까지 한 번 돌린다.
//!
//! # 무엇을 꿰는가
//!
//! ```text
//! PersistentKeyring   키를 만들고 디스크에 저장한다        (§11 K0/K1)
//! sign()              메시지에 서명한다                    (§3)
//! prost encode        wire bytes 로 만든다
//! decode_and_verify   ingress 진입점을 통과시킨다          (§8)
//! DurableReplayGuard  nonce 를 영속 저장소에 기록한다       (§10)
//! write_checkpoint    체크포인트를 쓴다                    (ADR-026)
//! find_resume_point_for  재개 지점을 찾는다
//! ```
//!
//! # ★ 정상 경로만 보지 않는다
//!
//! `RULE.md` §6 · `CLAUDE.md` §4 — 정상 경로 통과만으로 "된다" 고 하지 않는다.
//! 각 단계마다 **거부되어야 하는 것이 실제로 거부되는지**도 확인한다.
//!
//! 그러지 않으면 "전부 통과" 가 "아무것도 검사하지 않는다" 와 구분되지 않는다.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use gputeer_checkpoint::writer::{find_resume_point_for, manifest_for, write_checkpoint};
use gputeer_crypto::{
    decode_and_verify, sign, DurableReplayGuard, KeyDirectorySource, KeyProtection,
    PersistentKeyring, PlaintextPolicy, SecretSigningKey, SigningKey,
};
use gputeer_protocol::pb;
use gputeer_protocol::signing::{ReplayStatus, VerifyError, VerifyOutcome};
use prost::Message;

use gputeer_crypto::ingress::{Clock, IngressError};

const DEVICE: &str = "01JBXDEVICE0000000000000001";
const JOB: &str = "01JBXJOB000000000000000001";
const ATTEMPT: &str = "01JBXATTEMPT00000000000001";

/// selftest 는 **고정 시각**을 쓴다.
///
/// ★ `SystemTime::now()` 를 쓰면 결과가 날마다 달라져 재현할 수 없다.
///   시각에 의존하는 검사(만료·skew)를 결정적으로 돌리기 위해 고정한다.
const NOW: u64 = 1_755_200_000_000;

struct FixedClock(u64);

impl Clock for FixedClock {
    fn now_unix_ms(&self) -> u64 {
        self.0
    }
}

/// 한 항목의 결과. **세지 않고 버리지 않는다** (`CLAUDE.md` §3).
struct Report {
    lines: String,
    passed: usize,
    failed: usize,
}

impl Report {
    fn new() -> Self {
        Self {
            lines: String::new(),
            passed: 0,
            failed: 0,
        }
    }

    /// `ok` 가 참이면 통과. 거짓이면 `detail` 과 함께 실패로 센다.
    fn check(&mut self, label: &str, ok: bool, detail: &str) {
        if ok {
            self.passed += 1;
            let _ = writeln!(self.lines, "  ok    {label}");
        } else {
            self.failed += 1;
            let _ = writeln!(self.lines, "  실패  {label}\n          {detail}");
        }
    }

    fn note(&mut self, text: &str) {
        let _ = writeln!(self.lines, "        {text}");
    }

    fn section(&mut self, title: &str) {
        let _ = writeln!(self.lines, "\n{title}");
    }
}

fn grant(key: &SigningKey, nonce_seed: u8, expires_at: u64) -> pb::ExecutionGrant {
    let mut m = pb::ExecutionGrant {
        schema_version: 1,
        grant_id: "01JBXGRANT000000000000001".into(),
        attempt_id: ATTEMPT.into(),
        coordinator_device_id: DEVICE.into(),
        issued_at_unix_ms: NOW,
        expires_at_unix_ms: expires_at,
        nonce: (0u8..16).map(|i| i.wrapping_add(nonce_seed)).collect(),
        ..Default::default()
    };
    m.coordinator_signature = sign(key, &m).to_vec();
    m
}

fn manifest(key: &SigningKey) -> pb::JobManifest {
    let mut m = pb::JobManifest {
        schema_version: 1,
        job_id: JOB.into(),
        team_id: "01JBXTEAM00000000000000001".into(),
        entrypoint: "train.py".into(),
        submitter_device_id: DEVICE.into(),
        issued_at_unix_ms: NOW - 1_000,
        expires_at_unix_ms: NOW + 7 * 24 * 60 * 60 * 1_000,
        ..Default::default()
    };
    m.submitter_signature = sign(key, &m).to_vec();
    m
}

/// 작업 디렉터리를 정한다. `None` 이면 임시 디렉터리(끝나면 지운다).
enum Workspace {
    Temp(tempfile::TempDir),
    Given(PathBuf),
}

impl Workspace {
    fn path(&self) -> &Path {
        match self {
            Self::Temp(d) => d.path(),
            Self::Given(p) => p.as_path(),
        }
    }
}

pub fn run(dir: Option<&str>) -> Result<String, Box<dyn std::error::Error>> {
    let ws = match dir {
        Some(d) => {
            std::fs::create_dir_all(d)?;
            Workspace::Given(PathBuf::from(d))
        }
        None => Workspace::Temp(tempfile::tempdir()?),
    };
    let root = ws.path();

    let mut r = Report::new();
    let _ = writeln!(
        r.lines,
        "gputeer selftest — 구현된 계층을 끝에서 끝까지 한 번 돌린다\n\
         작업 디렉터리: {}\n\
         고정 시각: {NOW} (재현 가능하게 하려고 SystemTime 을 쓰지 않는다)\n\
         {}",
        root.display(),
        "=".repeat(70)
    );

    // ── 1. 키 보관 ────────────────────────────────────────────────
    r.section("1. 키 보관 (signing.md §11)");

    let key_path = root.join("keys.bin");
    // K0(평문)는 **명시적으로 허용해야만** 쓸 수 있다.
    // 기본 거부인지 먼저 확인한다 — 기본값이 안전한지가 중요하다.
    let rejected =
        PersistentKeyring::new(&key_path, KeyProtection::K0Plaintext, PlaintextPolicy::Reject);
    r.check(
        "K0 평문 저장은 기본으로 거부된다",
        rejected.is_err(),
        "명시적 opt-in 없이 평문 키 저장이 허용됐다",
    );

    let mut keyring = PersistentKeyring::new(
        &key_path,
        KeyProtection::K0Plaintext,
        PlaintextPolicy::Allow,
    )?;
    let signing_key = SigningKey::from_bytes(&[7u8; 32]);
    keyring.insert_private(DEVICE, SecretSigningKey::from_signing_key(signing_key.clone()))?;
    r.check("키를 등록했다", true, "");
    r.note(&format!("파일: {}", key_path.display()));

    // ★ 개인키가 사람이 읽는 출력에 새지 않는가.
    let dumped = format!("{keyring:?}");
    let leaked = ["07".repeat(32), format!("{:?}", [7u8; 32])]
        .iter()
        .any(|s| dumped.contains(s.as_str()));
    r.check(
        "개인키가 Debug 출력에 나타나지 않는다",
        !leaked,
        "키링 Debug 출력에 개인키 바이트가 들어 있다",
    );

    // ── 2. 서명 -> wire bytes -> 검증 ─────────────────────────────
    r.section("2. 서명 · 인코딩 · 검증 진입점 (signing.md §8)");

    let mut replay = DurableReplayGuard::open(root.join("replay.sqlite3"))?;
    r.check(
        "replay 저장소가 영속이라고 보고한다",
        replay.is_durable(),
        "is_durable() 이 false 다 — 재시작 후 replay 창이 열린다",
    );

    let g = grant(&signing_key, 0, NOW + 60_000);
    let raw = g.encode_to_vec();
    r.note(&format!("ExecutionGrant wire bytes: {} 바이트", raw.len()));

    let verified = decode_and_verify::<pb::ExecutionGrant>(
        &raw,
        1,
        KeyDirectorySource::Persistent(&keyring),
        &mut replay,
        &FixedClock(NOW),
    );
    match &verified {
        Ok(v) => {
            r.check("정상 Grant 가 검증을 통과한다", true, "");
            r.check(
                "replay 검사를 실제로 거쳤다고 보고한다",
                v.replay_status() == ReplayStatus::Checked,
                "단수명 메시지인데 Checked 가 아니다",
            );
            r.check(
                "부작용 게이트를 통과한다",
                v.require_replay_checked().is_ok(),
                "require_replay_checked() 가 거부했다",
            );
        }
        Err(e) => r.check("정상 Grant 가 검증을 통과한다", false, &format!("{e:?}")),
    }

    // ── 3. 거부되어야 하는 것들 ───────────────────────────────────
    r.section("3. 거부되어야 하는 것 (정상 경로만 보지 않는다)");

    // 3a. 같은 Grant 재전송 -> replay
    let again = decode_and_verify::<pb::ExecutionGrant>(
        &raw,
        1,
        KeyDirectorySource::Persistent(&keyring),
        &mut replay,
        &FixedClock(NOW),
    );
    r.check(
        "같은 Grant 재전송이 replay 로 거부된다",
        matches!(
            &again,
            Err(IngressError::Verification(VerifyError::Outcome(VerifyOutcome::Replay)))
        ),
        &format!("{:?}", again.as_ref().err()),
    );

    // 3b. 서명 위조
    let mut forged = grant(&signing_key, 1, NOW + 60_000);
    forged.coordinator_signature[0] ^= 0xFF;
    let out = decode_and_verify::<pb::ExecutionGrant>(
        &forged.encode_to_vec(),
        1,
        KeyDirectorySource::Persistent(&keyring),
        &mut replay,
        &FixedClock(NOW),
    );
    r.check(
        "서명을 한 비트 뒤집으면 거부된다",
        matches!(
            &out,
            Err(IngressError::Verification(VerifyError::Outcome(
                VerifyOutcome::InvalidSignature
            )))
        ),
        &format!("{:?}", out.as_ref().err()),
    );

    // 3c. 만료
    let expired = grant(&signing_key, 2, NOW - 1);
    let out = decode_and_verify::<pb::ExecutionGrant>(
        &expired.encode_to_vec(),
        1,
        KeyDirectorySource::Persistent(&keyring),
        &mut replay,
        &FixedClock(NOW),
    );
    r.check(
        "만료된 Grant 가 거부된다",
        matches!(
            &out,
            Err(IngressError::Verification(VerifyError::Outcome(VerifyOutcome::Expired)))
        ),
        &format!("{:?}", out.as_ref().err()),
    );

    // 3d. protobuf 가 아닌 바이트 -> 검증 결과가 아니라 디코드 실패
    let out = decode_and_verify::<pb::ExecutionGrant>(
        &[0xFF, 0xFF, 0xFF, 0xFF],
        1,
        KeyDirectorySource::Persistent(&keyring),
        &mut replay,
        &FixedClock(NOW),
    );
    r.check(
        "깨진 바이트는 '검증 실패' 가 아니라 '디코드 실패' 로 보고된다",
        matches!(&out, Err(IngressError::Decode(_))),
        &format!("{:?}", out.as_ref().err()),
    );

    // 3e. 장수명 메시지 — replay 방어가 **없다**는 사실이 드러나는가
    let m = manifest(&signing_key);
    let out = decode_and_verify::<pb::JobManifest>(
        &m.encode_to_vec(),
        1,
        KeyDirectorySource::Persistent(&keyring),
        &mut replay,
        &FixedClock(NOW),
    );
    match &out {
        Ok(v) => {
            r.check(
                "장수명 JobManifest 는 replay 대상이 아니라고 보고한다",
                v.replay_status() == ReplayStatus::NotApplicable,
                "replay 방어가 있는 것처럼 보고했다",
            );
            r.check(
                "그래서 부작용 게이트는 막힌다",
                v.require_replay_checked().is_err(),
                "replay 방어가 없는데 부작용 게이트를 열어 줬다",
            );
        }
        Err(e) => r.check("장수명 JobManifest 검증", false, &format!("{e:?}")),
    }

    // ── 4. 재시작 후에도 replay 를 기억하는가 ─────────────────────
    r.section("4. 재시작 (영속 replay 저장소, signing.md §10)");

    drop(replay);
    let mut reopened = DurableReplayGuard::open(root.join("replay.sqlite3"))?;
    let after_restart = decode_and_verify::<pb::ExecutionGrant>(
        &raw,
        1,
        KeyDirectorySource::Persistent(&keyring),
        &mut reopened,
        &FixedClock(NOW),
    );
    r.check(
        "저장소를 닫았다 열어도 이미 본 nonce 를 기억한다",
        matches!(
            &after_restart,
            Err(IngressError::Verification(VerifyError::Outcome(VerifyOutcome::Replay)))
        ),
        &format!(
            "재시작 후 replay 창이 열렸다: {:?}",
            after_restart.as_ref().err()
        ),
    );

    // ── 5. 체크포인트 ─────────────────────────────────────────────
    r.section("5. 체크포인트 쓰기와 재개 (ADR-026)");

    let ckpt_root = root.join("checkpoints");
    std::fs::create_dir_all(&ckpt_root)?;

    for step in [10u64, 20, 30] {
        let files = vec![(
            "shard-0.bin".to_string(),
            format!("weights at step {step}").into_bytes(),
        )];
        let mut cm = manifest_for(&format!("ckpt-{step:08}"), JOB, ATTEMPT, step, 1, &files);
        cm.created_at_unix_ms = NOW + step;
        write_checkpoint(&ckpt_root, &cm, &files, 0)?;
    }
    r.check("체크포인트 3개를 기록했다", true, "");

    // 남의 job 의 체크포인트를 같은 root 에 심는다 — 골라서는 안 된다.
    let other = vec![("shard-0.bin".to_string(), b"someone else".to_vec())];
    let mut om = manifest_for(
        "ckpt-99999999",
        "01JBXOTHERJOB0000000000001",
        ATTEMPT,
        999,
        1,
        &other,
    );
    om.created_at_unix_ms = NOW + 999;
    write_checkpoint(&ckpt_root, &om, &other, 0)?;

    let resume = find_resume_point_for(&ckpt_root, JOB, ATTEMPT)?;
    match resume {
        Some(found) => {
            r.check(
                "재개 지점이 내 job 의 가장 높은 step 이다",
                found.job_id == JOB && found.step == 30,
                &format!("job={} step={}", found.job_id, found.step),
            );
            r.note(&format!("선택된 체크포인트: {}", found.checkpoint_id));
        }
        None => r.check("재개 지점을 찾았다", false, "후보가 없다"),
    }

    // ── 마무리 ────────────────────────────────────────────────────
    let _ = writeln!(
        r.lines,
        "\n{}\n통과 {} · 실패 {}",
        "=".repeat(70),
        r.passed,
        r.failed
    );

    if r.failed == 0 {
        let _ = writeln!(
            r.lines,
            "\n★ 이것이 증명하지 않는 것\n\
             \x20 - 서비스가 돌아간다  네트워크 수신도 데몬도 없다\n\
             \x20 - Job 이 실행된다    runtime 계층이 미착수다\n\
             \x20 - 성능              한 번씩만 돌렸다. 숫자가 아니라 동작만 봤다\n\
             \x20 - Linux 에서 된다   이 실행은 이 기계에서만 한 것이다 (D-3)"
        );
    }

    Ok(r.lines)
}
