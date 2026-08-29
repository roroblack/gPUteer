//! `gputeer submit` — 제출자가 작업을 서명해 파일로 내보낸다.
//!
//! # 왜 별도 명령인가
//!
//! `JobManifest` 는 **제출자가 서명하는 메시지**다(`signing.md`).
//! 그런데 그 전 조각에서는 Coordinator 가 `--submitter-seed` 로 제출자
//! 개인키를 받아 직접 서명했다 — 테스트 배선이었지만, 그 모양이 그대로
//! 남으면 **Coordinator 가 제출자 키를 갖는 구조**가 된다. 그러면 Agent
//! 의 "nested Manifest 독립 검증" 이 아무것도 증명하지 못한다.
//!
//! 이 명령이 그 키를 Coordinator 밖으로 꺼낸다.
//!
//! ```text
//! 이전   Coordinator 가 제출자 키를 갖고 서명           (위험한 배선)
//! 이제   제출자가 서명한 파일을 Coordinator 가 실어 나름 (올바른 권한)
//! ```
//!
//! # 이 명령이 하지 않는 것
//!
//! ```text
//! 네트워크 전송      파일로만 낸다. wire ingress 는 후속 조각이다
//! 자원 요구 채우기   GPU/VRAM/데이터셋 요구는 아직 이 경로에 없다
//! 키 보관           seed 를 인자로 받는다. 운영용 key protection 은 후속
//! ```
//!
//! ★ **seed 를 명령줄 인자로 받는 것은 운영 방식이 아니다.** 명령줄은
//!   프로세스 목록·셸 이력에 남는다. 지금은 selftest 재현성을 위한
//!   배선이며, 실제 키 보관은 `crates/crypto` 의 `PersistentKeyring`
//!   계열로 옮겨야 한다.

use std::collections::BTreeMap;

use gputeer_crypto::{sign, SigningKey};
use gputeer_protocol::pb;
use prost::Message;

/// `gputeer submit` 진입점.
pub fn run(args: &[String]) -> Result<String, String> {
    let flags = parse_flags(args)?;

    let job_id = require(&flags, "--job-id")?;
    let entrypoint = require(&flags, "--entrypoint")?;
    let submitter_device_id = require(&flags, "--submitter-device-id")?;
    let seed_hex = require(&flags, "--submitter-seed")?;
    let out_path = require(&flags, "--out")?;

    // 쉼표로 나눈다. 빈 조각은 버리지 않는다 — 빈 인자는 정당하고,
    // `derive_execution_spec()` 도 그것을 허용한다.
    let args_list: Vec<String> = match flags.get("--args") {
        Some(raw) if !raw.is_empty() => raw.split(',').map(str::to_owned).collect(),
        _ => Vec::new(),
    };

    // `KEY=VALUE` 를 쉼표로 나눈다. 첫 `=` 만 경계다 — 값에 `=` 가
    // 있는 것은 정당하다(`--env FLAGS=--a=1`).
    let mut env_vars: BTreeMap<String, String> = BTreeMap::new();
    if let Some(raw) = flags.get("--env") {
        for pair in raw.split(',').filter(|p| !p.is_empty()) {
            let (name, value) = pair
                .split_once('=')
                .ok_or_else(|| format!("--env 항목에 '=' 가 없다: {pair:?}"))?;
            env_vars.insert(name.to_owned(), value.to_owned());
        }
    }

    let issued_at_unix_ms = require_u64(&flags, "--issued-at-unix-ms")?;
    // 기본 7일 — `proto/job.proto` 의 "기본 issued_at + 7일" 주석 그대로.
    let expires_at_unix_ms = match flags.get("--expires-at-unix-ms") {
        Some(raw) => raw
            .parse::<u64>()
            .map_err(|e| format!("--expires-at-unix-ms 파싱 실패: {e}"))?,
        None => issued_at_unix_ms + 7 * 24 * 60 * 60 * 1000,
    };

    let mut manifest = pb::JobManifest {
        schema_version: 1,
        job_id: job_id.clone(),
        entrypoint,
        args: args_list,
        env_vars: env_vars.into_iter().collect(),
        submitter_device_id,
        issued_at_unix_ms,
        expires_at_unix_ms,
        ..Default::default()
    };

    let seed = hex_to_seed(&seed_hex)?;
    let key = SigningKey::from_bytes(&seed);
    manifest.submitter_signature = sign(&key, &manifest).to_vec();

    // ★ 서명 뒤에 바로 자기 검증한다. 서명하자마자 깨진 파일을 내보내는
    //   것보다, 여기서 실패하는 편이 훨씬 싸다.
    let derived = gputeer_protocol::execution_spec::derive_execution_spec(
        &gputeer_protocol::verify(
            &manifest,
            1,
            &AlwaysValid,
            issued_at_unix_ms,
            &mut gputeer_protocol::signing::NoReplayCheck,
        )
        .map_err(|e| format!("방금 서명한 Manifest 가 자기 검증을 통과하지 못했다: {e:?}"))?,
    )
    .map_err(|e| format!("방금 서명한 Manifest 에서 실행 지시를 만들 수 없다: {e}"))?;

    std::fs::write(&out_path, manifest.encode_to_vec())
        .map_err(|e| format!("Manifest 파일 쓰기 실패({out_path}): {e}"))?;

    Ok(format!(
        "SUBMITTED job_id={} entrypoint={} args={} env_vars={} out={}",
        job_id,
        derived.entrypoint,
        derived.args.len(),
        derived.env_vars.len(),
        out_path
    ))
}

/// 자기 검증 전용 — 서명을 방금 우리가 만들었으므로 여기서 다시 Ed25519
/// 를 도는 것은 목적이 아니다. 확인하려는 것은 **구조와 수명**이다
/// (`schema_version`·만료·canonical 인코딩·파생 일관성).
///
/// ★ 이것을 Agent 쪽에서 쓰면 안 된다. 여기서만 쓰이는 이유는 "내가
///   방금 만든 것" 이기 때문이다.
struct AlwaysValid;

impl gputeer_protocol::signing::SignatureVerifier for AlwaysValid {
    fn verify_signature(
        &self,
        _signer_id: &str,
        _message: &[u8],
        _signature: &[u8],
    ) -> Result<(), gputeer_protocol::VerifyOutcome> {
        Ok(())
    }
}

fn parse_flags(args: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < args.len() {
        let key = &args[i];
        if !key.starts_with("--") {
            return Err(format!("플래그가 아닌 인자: {key:?}"));
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("{key} 에 값이 없다"))?;
        out.insert(key.clone(), value.clone());
        i += 2;
    }
    Ok(out)
}

fn require(flags: &BTreeMap<String, String>, key: &str) -> Result<String, String> {
    flags
        .get(key)
        .cloned()
        .ok_or_else(|| format!("{key} 가 필요하다"))
}

fn require_u64(flags: &BTreeMap<String, String>, key: &str) -> Result<u64, String> {
    require(flags, key)?
        .parse::<u64>()
        .map_err(|e| format!("{key} 파싱 실패: {e}"))
}

fn hex_to_seed(hex: &str) -> Result<[u8; 32], String> {
    if hex.len() != 64 {
        return Err(format!("seed 는 64자리 hex 여야 한다(길이 {})", hex.len()));
    }
    let mut seed = [0u8; 32];
    for (index, slot) in seed.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
            .map_err(|e| format!("seed hex 파싱 실패: {e}"))?;
    }
    Ok(seed)
}
