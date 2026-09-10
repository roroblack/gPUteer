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
/// 만료를 안 주면 쓰는 기본 기간. `proto/job.proto` 의 "기본 issued_at + 7일".
const SEVEN_DAYS_MS: u64 = 7 * 24 * 60 * 60 * 1000;

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
    //
    // ★ 이것은 `CLAUDE.md` §1 의 "지어내지 않는다" 위반이 **아니다** —
    //   규범이 그 기본값을 정해 뒀으므로 옮겨 적는 것이다. 다만
    //   **제출자가 말하지 않은 값이 서명 대상에 들어간다**는 사실은
    //   같으므로, 그 경로를 실제로 도는 테스트를 둔다
    //   (`tests/submit.rs::omitting_the_expiry_uses_the_documented_seven_day_default`).
    let expires_at_unix_ms = match flags.get("--expires-at-unix-ms") {
        Some(raw) => raw
            .parse::<u64>()
            .map_err(|e| format!("--expires-at-unix-ms 파싱 실패: {e}"))?,
        // ★★ 2026-09-10 독립 검수 지적 — 여기가 `issued + 7일` 이었고
        //   **덧셈이 넘칠 수 있었다.** `--issued-at-unix-ms` 는 u64 를
        //   그대로 받으므로 큰 값을 주면:
        //     오버플로 검사 켜진 빌드   panic (스택 트레이스가 나온다)
        //     꺼진 빌드                 값이 되감겨 만료가 발급보다 앞서고,
        //                               아래 역전 검사에 걸려 거부된다
        //   **빌드 설정에 따라 동작이 달라졌다.** 재현했다:
        //     --issued-at-unix-ms 18446744073709551615
        //     -> panicked at submit.rs:78 "attempt to add with overflow"
        //   사용자 입력으로 패닉이 나면 그건 오류 보고가 아니다.
        None => issued_at_unix_ms
            .checked_add(SEVEN_DAYS_MS)
            .ok_or_else(|| {
                format!(
                    "SUBMIT_REFUSED: ISSUED_AT_TOO_LARGE — --issued-at-unix-ms({issued_at_unix_ms})에 기본 만료 7일을 더하면 u64 를 넘는다. 만료를 직접 주거나 발급 시각을 확인하라"
                )
            })?,
    };

    // ★★ **발급 < 만료 를 여기서 본다** (2026-09-07 독립 검수 지적).
    //
    //   전에는 이 검사가 **없었다.** 역전된 시각도 그대로 서명한 뒤
    //   바깥 `verify` 에 넘겼다. 파일은 안 남지만(검증이 쓰기보다 앞이다)
    //   **서명은 이미 한 뒤**이고, 무엇보다 그 경로를 도는 테스트가
    //   하나도 없었다.
    //
    //   `stage-job` 이 Lease 시각에 대해 하는 것과 같은 검사다 — 같은
    //   성격의 값에 한쪽만 검사가 있으면, 있는 쪽을 보고 없는 쪽도
    //   막힌다고 믿게 된다.
    if issued_at_unix_ms >= expires_at_unix_ms {
        return Err(format!(
            "SUBMIT_REFUSED: 발급({issued_at_unix_ms})이 만료({expires_at_unix_ms}) 보다 뒤이거나 같다 — 만들자마자 만료된 Manifest 는 아무도 못 쓴다"
        ));
    }

    // ── 스케줄에 필요한 선언 ────────────────────────────────────────
    //
    // ★ **전부 선택 사항이다. 그러나 안 주면 이 Job 은 스케줄되지
    //   않는다.** proto 의 분류 축은 모두 `*_UNSPECIFIED = 0` 이고,
    //   `manifest_requirements.rs` 가 그것을 값으로 바꾸지 않고 거부한다
    //   — 제출자가 **말하지 않은 것을 말한 것으로** 만들지 않기
    //   위해서다(`CLAUDE.md` §1).
    //
    //   그래서 여기서도 기본값을 지어내지 않는다. 안 주면 안 준 대로
    //   서명되고, `gputeer plan-job` 이 **어느 축이 비었는지 이름을
    //   대며** 거부한다.
    //
    //   ★ 이 조각 전까지 `submit` 은 이 축들을 **선언할 방법 자체가
    //     없었다** — 즉 이 명령이 만든 어떤 Manifest 도 스케줄될 수
    //     없었다. 변환기의 fail-closed 규칙이 그 사실을 드러냈다.
    let workload_class = enum_flag(&flags, "--workload-class", &[
        ("TRAINING", pb::WorkloadClass::Training as i32),
        ("INFERENCE", pb::WorkloadClass::Inference as i32),
        ("PREPROCESSING", pb::WorkloadClass::Preprocessing as i32),
        ("EVALUATION", pb::WorkloadClass::Evaluation as i32),
        ("RENDERING", pb::WorkloadClass::Rendering as i32),
        ("OTHER", pb::WorkloadClass::Other as i32),
    ])?;
    let side_effect_class = enum_flag(&flags, "--side-effect-class", &[
        ("PURE", pb::SideEffectClass::Pure as i32),
        ("IDEMPOTENT", pb::SideEffectClass::Idempotent as i32),
        ("SIDE_EFFECTING", pb::SideEffectClass::SideEffecting as i32),
    ])?;
    let sensitivity = enum_flag(&flags, "--dataset-sensitivity", &[
        ("PUBLIC", pb::Sensitivity::Public as i32),
        ("INTERNAL", pb::Sensitivity::Internal as i32),
        ("SENSITIVE", pb::Sensitivity::Sensitive as i32),
    ])?;
    let minimum_security_tier = enum_flag(&flags, "--minimum-security-tier", &[
        ("S0", pb::SecurityTier::S0 as i32),
        ("S1", pb::SecurityTier::S1 as i32),
        ("S2", pb::SecurityTier::S2 as i32),
        ("S3", pb::SecurityTier::S3 as i32),
        ("S4", pb::SecurityTier::S4 as i32),
        ("S5", pb::SecurityTier::S5 as i32),
    ])?;
    let minimum_isolation_class = enum_flag(&flags, "--minimum-isolation-class", &[
        ("RESTRICTED", pb::IsolationClass::Restricted as i32),
        ("CONTAINED", pb::IsolationClass::Contained as i32),
        ("VIRTUALIZED", pb::IsolationClass::Virtualized as i32),
    ])?;
    let minimum_key_protection = enum_flag(&flags, "--minimum-key-protection", &[
        ("K0", pb::KeyProtection::K0 as i32),
        ("K1", pb::KeyProtection::K1 as i32),
        ("K2", pb::KeyProtection::K2 as i32),
    ])?;

    // 자원 요구. `--gpu-count` 를 준 경우에만 `ResourceRequest` 를 만든다
    // — 안 주면 메시지 자체가 없고 변환기가 그렇게 보고한다.
    let resources = match flags.get("--gpu-count") {
        None => None,
        Some(_) => Some(pb::ResourceRequest {
            gpu: Some(pb::GpuRequest {
                min_count: u32_flag(&flags, "--gpu-count")?.unwrap_or(0),
                min_vram_bytes: u64_flag(&flags, "--gpu-min-vram-bytes")?.unwrap_or(0),
                allowed_gpu_models: match flags.get("--allowed-gpu-models") {
                    Some(raw) if !raw.is_empty() => {
                        raw.split(',').map(|s| s.trim().to_string()).collect()
                    }
                    _ => Vec::new(),
                },
                ..Default::default()
            }),
            cpu_cores: u32_flag(&flags, "--cpu-cores")?.unwrap_or(0),
            ram_bytes: u64_flag(&flags, "--ram-bytes")?.unwrap_or(0),
            workspace_bytes: u64_flag(&flags, "--workspace-bytes")?.unwrap_or(0),
            ..Default::default()
        }),
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
        side_effect_class,
        minimum_security_tier,
        minimum_isolation_class,
        minimum_key_protection,
        resources,
        workload: if workload_class == 0 {
            None
        } else {
            Some(pb::WorkloadHint {
                class: workload_class,
                ..Default::default()
            })
        },
        dataset: if sensitivity == 0 {
            None
        } else {
            Some(pb::DatasetRef {
                sensitivity,
                ..Default::default()
            })
        },
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

    // ★★ 2026-09-10 — 여기도 `fs::write` 한 줄이었다. `issue-grant` 에서
    //   같은 줄이 독립 검수에 걸렸고(멀쩡한 산출물을 말없이 덮고, 중간에
    //   실패하면 잘린 채 남는다), **이 파일에도 똑같이 있었다.**
    //   한쪽만 고치면 다음 사람이 다른 쪽을 다시 발견한다 — 그래서
    //   도우미를 `crate::out_file` 한 곳에 뒀다.
    let overwrite = matches!(
        flags.get("--overwrite-existing-manifest").map(String::as_str),
        Some("true")
    );
    crate::out_file::write_new(
        &out_path,
        &manifest.encode_to_vec(),
        overwrite,
        "SUBMIT_REFUSED",
        "Manifest",
    )?;

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

/// 이름으로 받은 enum 값을 번호로 옮긴다. **안 주면 0(UNSPECIFIED)** 이고
/// 그건 "선언하지 않았다" 는 사실 그대로다 — 기본값을 지어내지 않는다.
///
/// 모르는 이름은 거부한다. 오타를 조용히 `UNSPECIFIED` 로 흘리면
/// 제출자는 선언했다고 믿는데 스케줄러는 못 본다.
fn enum_flag(
    flags: &BTreeMap<String, String>,
    key: &str,
    table: &[(&str, i32)],
) -> Result<i32, String> {
    let Some(raw) = flags.get(key) else {
        return Ok(0);
    };
    table
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(raw))
        .map(|(_, value)| *value)
        .ok_or_else(|| {
            format!(
                "{key} 값 {raw:?} 를 모른다 — 쓸 수 있는 값: {}",
                table
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn u32_flag(flags: &BTreeMap<String, String>, key: &str) -> Result<Option<u32>, String> {
    flags
        .get(key)
        .map(|raw| {
            raw.parse::<u32>()
                .map_err(|e| format!("{key} 파싱 실패: {e}"))
        })
        .transpose()
}

fn u64_flag(flags: &BTreeMap<String, String>, key: &str) -> Result<Option<u64>, String> {
    flags
        .get(key)
        .map(|raw| {
            raw.parse::<u64>()
                .map_err(|e| format!("{key} 파싱 실패: {e}"))
        })
        .transpose()
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
