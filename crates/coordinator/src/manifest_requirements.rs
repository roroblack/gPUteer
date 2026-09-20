//! 검증된 `JobManifest` 를 scheduler 의 `JobRequirements` 로 옮기는
//! **순수 변환기**.
//!
//! # 이것이 왜 별도 조각인가
//!
//! 여러 evidence 가 "`JobManifest` → `JobRequirements` 변환기가 없다" 고
//! 적어 뒀다. 그게 없어서 저장된 Job 을 scheduler 에 태울 수 없었다.
//!
//! # 이 모듈이 하지 않는 것
//!
//! ```text
//! 안 한다   서명 검증          `&Verified<M>` 만 받는다 — 이미 끝난 일이다
//! 안 한다   device→member 해석  caller 가 넘긴 값을 쓸 뿐이다(아래 참조)
//! 안 한다   시계 읽기·I/O·DB    순수 함수다
//! 안 한다   후보 선택           그건 `evaluate_eligibility()` 의 일이다
//! ```
//!
//! # ★ `submitter_member_id` 를 왜 인자로 받는가
//!
//! **`JobManifest` 에 그 값이 없다.** `team_id` 와 `submitter_device_id`
//! 는 있지만 멤버 식별자는 없다. scheduler 는 그것으로 "제3자 Job 인가"
//! 를 판정하므로(`filter.rs` — 노드 소유자와 다르면 제3자) 없으면
//! `MissingFact::JobSubmitterMemberId` 로 후보가 전부 떨어진다.
//!
//! device→member 를 권위 있게 해석하는 경로는 이 저장소에 아직 없다
//! (멤버십은 사용자 결정 4건과 `COMMITTED` 부재로 막혀 있다). 그래서
//! **이 커널은 그것을 해석하지 않고 요구한다** — 호출부가 어디서
//! 가져왔는지 진술해야 하고, 오늘 정직한 출처는 운영자 선언뿐이다.
//! 여기서 몰래 `team_id` 를 멤버로 쓰면 그 순간 없는 권위를 지어낸다.
//!
//! # ★ `UNSPECIFIED` 를 값으로 바꾸지 않는다
//!
//! proto 의 모든 분류 축이 `*_UNSPECIFIED = 0` 을 갖는다. 그것을
//! "기본값" 으로 옮기면 제출자가 **말하지 않은 것을 말한 것으로**
//! 만든다 — 예컨대 `SIDE_EFFECT_CLASS_UNSPECIFIED` 를 `Pure` 로 읽으면,
//! 외부 부작용을 선언하지 않은 Job 이 "부작용 없음" 으로 스케줄된다.
//! `CLAUDE.md` §1 — 모르면 비워 두고, 여기서는 **거부한다.**

use gputeer_protocol::{pb, Verified};
use gputeer_scheduler::{
    IsolationClass, JobRequirements, KeyProtection, SecurityTier, Sensitivity, SideEffectClass,
    WorkloadClass,
};

/// 변환할 수 없는 이유. 운영자가 **무엇을 고쳐야 하는지**로 나눈다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestConversionError {
    /// 제출자가 선언하지 않은 축이다. Manifest 를 다시 만들어야 한다.
    Unspecified(&'static str),
    /// 이 빌드가 모르는 enum 값이다 — 제출자가 더 새로운 스키마를 썼다.
    UnknownEnumValue { field: &'static str, value: i32 },
    /// 메시지 자체가 없다(`resources` 처럼 중첩 메시지가 통째로 빠졌다).
    MissingMessage(&'static str),
    /// caller 가 넘겨야 하는 값이 비었다.
    MissingCallerFact(&'static str),
    /// 수치 자원을 선언하지 않았다.
    ///
    /// ★★ 2026-09-10 독립 검수 지적으로 생겼다. proto3 의 숫자에는
    ///   "없음" 이 없어서 **생략과 0 이 구분되지 않는다.** 그전에는
    ///   생략된 CPU·RAM·workspace 를 `Some(0)` 으로 넘겼고, 그것은
    ///   scheduler 에게 **"이 Job 은 0 개를 요구한다"** 는 선언된
    ///   사실로 읽혔다. 그러면 CPU 여유가 없는 노드에도 맞는다.
    ///
    ///   `CLAUDE.md` §1 — 값을 모르면 비워 둔다. 추정으로 채우면 그
    ///   오류가 조용히 스케줄링 결정까지 간다.
    ///
    /// ★ 규범이 답을 준다. proto 는 **기본값을 의도한 자리마다 주석을
    ///   달아 뒀다** — `min_count` 는 "기본 1", `allocation_mode` 는
    ///   "미지정 시 EXCLUSIVE", `allowed_gpu_models` 는 "빈 목록 =
    ///   제약 없음", `max_egress_bps` 는 "0 = 노드 정책을 따름".
    ///   그런데 `cpu_cores`·`ram_bytes`·`workspace_bytes` 에는 **없다.**
    ///   즉 그 셋에 의미를 준 것은 규범이 아니라 이 파일이었다.
    UndeclaredAmount(&'static str),
}

impl std::fmt::Display for ManifestConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unspecified(field) => write!(
                f,
                "Manifest 가 {field} 를 선언하지 않았다 — 말하지 않은 것을 기본값으로 채우지 않는다"
            ),
            Self::UnknownEnumValue { field, value } => write!(
                f,
                "이 빌드가 모르는 {field} 값({value}) — 더 새로운 스키마로 서명된 Manifest 일 수 있다"
            ),
            Self::UndeclaredAmount(field) => write!(
                f,
                "Manifest 가 {field} 를 선언하지 않았다(0) — 0 을 요구량으로 읽으면 자원이 없는 노드에도 맞는다"
            ),
            Self::MissingMessage(name) => {
                write!(f, "Manifest 에 {name} 이 통째로 없다")
            }
            Self::MissingCallerFact(name) => write!(
                f,
                "{name} 은 Manifest 에 없다 — 호출부가 권위 있는 출처에서 가져와 넘겨야 한다"
            ),
        }
    }
}

impl std::error::Error for ManifestConversionError {}

/// 검증된 Manifest 를 scheduler 입력으로 옮긴다.
///
/// `submitter_member_id` 는 **caller 가 해석한 값**이다 — 모듈 문서의
/// "왜 인자로 받는가" 를 보라.
pub fn job_requirements_from_manifest(
    manifest: &Verified<pb::JobManifest>,
    submitter_member_id: &str,
) -> Result<JobRequirements, ManifestConversionError> {
    // ★ 여기 위에서는 어떤 필드도 읽지 않았다 — `Verified::get()` 이
    //   유일한 입구다(`CLAUDE.md` §0.2).
    let m = manifest.get();

    if submitter_member_id.trim().is_empty() {
        return Err(ManifestConversionError::MissingCallerFact(
            "submitter_member_id",
        ));
    }

    let resources = m
        .resources
        .as_ref()
        .ok_or(ManifestConversionError::MissingMessage("resources"))?;
    let gpu = resources
        .gpu
        .as_ref()
        .ok_or(ManifestConversionError::MissingMessage("resources.gpu"))?;
    let workload = m
        .workload
        .as_ref()
        .ok_or(ManifestConversionError::MissingMessage("workload"))?;
    let dataset = m
        .dataset
        .as_ref()
        .ok_or(ManifestConversionError::MissingMessage("dataset"))?;

    Ok(JobRequirements {
        submitter_member_id: Some(submitter_member_id.to_string()),
        workload_class: Some(workload_class(workload.class)?),
        side_effect_class: Some(side_effect_class(m.side_effect_class)?),
        sensitivity: Some(sensitivity(dataset.sensitivity)?),
        minimum_security_tier: Some(security_tier(m.minimum_security_tier)?),
        minimum_isolation_class: Some(isolation_class(m.minimum_isolation_class)?),
        minimum_key_protection: Some(key_protection(m.minimum_key_protection)?),
        // ★ proto 가 "기본 1" 이라 적었으므로 0 은 미선언이 아니라
        //   **선언된 기본값**이다. 이건 지어내는 것이 아니라 규범을
        //   그대로 옮기는 것이다(`proto/common.proto` GpuRequest.min_count).
        minimum_gpu_count: Some(if gpu.min_count == 0 { 1 } else { gpu.min_count }),
        // 0 은 "VRAM 하한 없음" 이다 — 규범이 하한을 요구하지 않는다.
        minimum_vram_bytes_per_gpu: Some(gpu.min_vram_bytes),
        // proto: "빈 목록 = 제약 없음".
        allowed_gpu_models: gpu.allowed_gpu_models.clone(),
        // ★ 0 은 "0 개를 요구한다" 가 아니라 **선언하지 않았다** 이다.
        //   proto 가 이 셋에는 기본값 주석을 안 달았다 — 위
        //   `UndeclaredAmount` 주석 참조.
        cpu_cores: Some(nonzero(resources.cpu_cores as u64, "resources.cpu_cores")? as u32),
        ram_bytes: Some(nonzero(resources.ram_bytes, "resources.ram_bytes")?),
        workspace_bytes: Some(nonzero(
            resources.workspace_bytes,
            "resources.workspace_bytes",
        )?),
    })
}

/// 0 이면 **선언하지 않은 것**으로 보고 거부한다.
///
/// ★ `min_vram_bytes` 에는 쓰지 않는다 — 그쪽은 "하한 없음" 이라는
///   읽기가 실제로 쓸모가 있고, 0 말고 그것을 표현할 방법이 없다.
///   다만 그 읽기도 규범에 근거가 없다(`proto/common.proto:188` 에
///   주석이 없다). 그건 규범 결정이라 여기서 정하지 않는다 —
///   `docs/plans/_열린_작업.md` 에 올려 뒀다.
fn nonzero(value: u64, field: &'static str) -> Result<u64, ManifestConversionError> {
    if value == 0 {
        return Err(ManifestConversionError::UndeclaredAmount(field));
    }
    Ok(value)
}

// ---------------------------------------------------------------- enum 축
//
// ★ 각 함수가 **exhaustive `match`** 다. proto 에 새 변형이 생겨
//   `crates/protocol` 이 그것을 알게 되면 **컴파일이 깨진다** — 손으로
//   쓴 표가 조용히 낡는 것을 막는다(이 저장소에서 세 번 겪었다).

fn workload_class(value: i32) -> Result<WorkloadClass, ManifestConversionError> {
    let parsed = pb::WorkloadClass::try_from(value).map_err(|_| {
        ManifestConversionError::UnknownEnumValue {
            field: "workload.class",
            value,
        }
    })?;
    match parsed {
        pb::WorkloadClass::Unspecified => {
            Err(ManifestConversionError::Unspecified("workload.class"))
        }
        pb::WorkloadClass::Training => Ok(WorkloadClass::Training),
        pb::WorkloadClass::Inference => Ok(WorkloadClass::Inference),
        pb::WorkloadClass::Preprocessing => Ok(WorkloadClass::Preprocessing),
        pb::WorkloadClass::Evaluation => Ok(WorkloadClass::Evaluation),
        pb::WorkloadClass::Rendering => Ok(WorkloadClass::Rendering),
        pb::WorkloadClass::Other => Ok(WorkloadClass::Other),
    }
}

fn side_effect_class(value: i32) -> Result<SideEffectClass, ManifestConversionError> {
    let parsed = pb::SideEffectClass::try_from(value).map_err(|_| {
        ManifestConversionError::UnknownEnumValue {
            field: "side_effect_class",
            value,
        }
    })?;
    match parsed {
        pb::SideEffectClass::Unspecified => {
            Err(ManifestConversionError::Unspecified("side_effect_class"))
        }
        pb::SideEffectClass::Pure => Ok(SideEffectClass::Pure),
        pb::SideEffectClass::Idempotent => Ok(SideEffectClass::Idempotent),
        pb::SideEffectClass::SideEffecting => Ok(SideEffectClass::SideEffecting),
    }
}

fn sensitivity(value: i32) -> Result<Sensitivity, ManifestConversionError> {
    let parsed = pb::Sensitivity::try_from(value).map_err(|_| {
        ManifestConversionError::UnknownEnumValue {
            field: "dataset.sensitivity",
            value,
        }
    })?;
    match parsed {
        pb::Sensitivity::Unspecified => {
            Err(ManifestConversionError::Unspecified("dataset.sensitivity"))
        }
        pb::Sensitivity::Public => Ok(Sensitivity::Public),
        pb::Sensitivity::Internal => Ok(Sensitivity::Internal),
        pb::Sensitivity::Sensitive => Ok(Sensitivity::Sensitive),
    }
}

fn security_tier(value: i32) -> Result<SecurityTier, ManifestConversionError> {
    let parsed = pb::SecurityTier::try_from(value).map_err(|_| {
        ManifestConversionError::UnknownEnumValue {
            field: "minimum_security_tier",
            value,
        }
    })?;
    match parsed {
        pb::SecurityTier::Unspecified => Err(ManifestConversionError::Unspecified(
            "minimum_security_tier",
        )),
        pb::SecurityTier::S0 => Ok(SecurityTier::S0),
        pb::SecurityTier::S1 => Ok(SecurityTier::S1),
        pb::SecurityTier::S2 => Ok(SecurityTier::S2),
        pb::SecurityTier::S3 => Ok(SecurityTier::S3),
        pb::SecurityTier::S4 => Ok(SecurityTier::S4),
        pb::SecurityTier::S5 => Ok(SecurityTier::S5),
    }
}

fn isolation_class(value: i32) -> Result<IsolationClass, ManifestConversionError> {
    let parsed = pb::IsolationClass::try_from(value).map_err(|_| {
        ManifestConversionError::UnknownEnumValue {
            field: "minimum_isolation_class",
            value,
        }
    })?;
    match parsed {
        pb::IsolationClass::Unspecified => Err(ManifestConversionError::Unspecified(
            "minimum_isolation_class",
        )),
        pb::IsolationClass::Restricted => Ok(IsolationClass::Restricted),
        pb::IsolationClass::Contained => Ok(IsolationClass::Contained),
        pb::IsolationClass::Virtualized => Ok(IsolationClass::Virtualized),
    }
}

fn key_protection(value: i32) -> Result<KeyProtection, ManifestConversionError> {
    let parsed = pb::KeyProtection::try_from(value).map_err(|_| {
        ManifestConversionError::UnknownEnumValue {
            field: "minimum_key_protection",
            value,
        }
    })?;
    match parsed {
        pb::KeyProtection::Unspecified => Err(ManifestConversionError::Unspecified(
            "minimum_key_protection",
        )),
        pb::KeyProtection::K0 => Ok(KeyProtection::K0),
        pb::KeyProtection::K1 => Ok(KeyProtection::K1),
        pb::KeyProtection::K2 => Ok(KeyProtection::K2),
    }
}

#[cfg(test)]
mod tests {
    use gputeer_crypto::{sign, Ed25519Verifier, InMemoryKeyring, SigningKey};
    use gputeer_protocol::signing::{verify, NoReplayCheck};

    use super::*;

    const DEVICE: &str = "01JSUBMITTERCONVERT00001";
    const MEMBER: &str = "member-alice";

    /// 모든 축이 채워진 Manifest. `tweak` 으로 한 군데만 비워 반례를 만든다.
    fn verified(tweak: impl FnOnce(&mut pb::JobManifest)) -> Verified<pb::JobManifest> {
        let key = SigningKey::from_bytes(&[11u8; 32]);
        let mut manifest = pb::JobManifest {
            schema_version: 1,
            job_id: "01JJOBCONVERT00000000001".to_string(),
            team_id: "team-a".to_string(),
            entrypoint: "train.py".to_string(),
            submitter_device_id: DEVICE.to_string(),
            issued_at_unix_ms: 10,
            expires_at_unix_ms: 10_000,
            resources: Some(pb::ResourceRequest {
                gpu: Some(pb::GpuRequest {
                    min_vram_bytes: 8 * 1024 * 1024 * 1024,
                    min_count: 2,
                    allowed_gpu_models: vec!["RTX 4070 SUPER".to_string()],
                    ..Default::default()
                }),
                cpu_cores: 8,
                ram_bytes: 16 * 1024 * 1024 * 1024,
                workspace_bytes: 64 * 1024 * 1024 * 1024,
                ..Default::default()
            }),
            workload: Some(pb::WorkloadHint {
                class: pb::WorkloadClass::Training as i32,
                ..Default::default()
            }),
            dataset: Some(pb::DatasetRef {
                sensitivity: pb::Sensitivity::Internal as i32,
                ..Default::default()
            }),
            side_effect_class: pb::SideEffectClass::Pure as i32,
            minimum_security_tier: pb::SecurityTier::S2 as i32,
            minimum_isolation_class: pb::IsolationClass::Contained as i32,
            minimum_key_protection: pb::KeyProtection::K1 as i32,
            ..Default::default()
        };
        tweak(&mut manifest);
        manifest.submitter_signature = sign(&key, &manifest).to_vec();
        let mut keys = InMemoryKeyring::new();
        keys.insert(DEVICE, key.verifying_key());
        verify(
            &manifest,
            1,
            &Ed25519Verifier::new(keys),
            100,
            &mut NoReplayCheck,
        )
        .expect("test Manifest signature must verify")
    }

    #[test]
    fn a_complete_manifest_becomes_complete_requirements() {
        let requirements =
            job_requirements_from_manifest(&verified(|_| {}), MEMBER).expect("변환 실패");

        // ★ 기대값을 **손으로** 적는다. 변환 함수를 다시 돌려 만들면
        //   그 함수가 일관되게 틀려도 통과한다(이 저장소에서 세 번 나왔다).
        assert_eq!(requirements.submitter_member_id.as_deref(), Some(MEMBER));
        assert_eq!(requirements.workload_class, Some(WorkloadClass::Training));
        assert_eq!(requirements.side_effect_class, Some(SideEffectClass::Pure));
        assert_eq!(requirements.sensitivity, Some(Sensitivity::Internal));
        assert_eq!(requirements.minimum_security_tier, Some(SecurityTier::S2));
        assert_eq!(
            requirements.minimum_isolation_class,
            Some(IsolationClass::Contained)
        );
        assert_eq!(requirements.minimum_key_protection, Some(KeyProtection::K1));
        assert_eq!(requirements.minimum_gpu_count, Some(2));
        assert_eq!(
            requirements.minimum_vram_bytes_per_gpu,
            Some(8 * 1024 * 1024 * 1024)
        );
        assert_eq!(requirements.allowed_gpu_models, vec!["RTX 4070 SUPER"]);
        assert_eq!(requirements.cpu_cores, Some(8));
        assert_eq!(requirements.ram_bytes, Some(16 * 1024 * 1024 * 1024));
        assert_eq!(requirements.workspace_bytes, Some(64 * 1024 * 1024 * 1024));
    }

    /// ★★ **선언하지 않은 축은 기본값으로 채우지 않는다.**
    ///
    /// 여섯 축 각각을 `UNSPECIFIED` 로 두고 **각각 그 이름으로** 거부되는지
    /// 본다. "뭔가 거부됐다" 만 보면 여섯 중 하나만 막는 구현도, 한 축을
    /// 막으면서 다른 축 이름을 보고하는 구현도 통과한다.
    #[test]
    fn every_unspecified_axis_is_refused_by_its_own_name() {
        type Blank = Box<dyn Fn(&mut pb::JobManifest)>;
        let cases: Vec<(&'static str, Blank)> = vec![
            (
                "workload.class",
                Box::new(|m: &mut pb::JobManifest| {
                    m.workload.as_mut().unwrap().class = pb::WorkloadClass::Unspecified as i32;
                }),
            ),
            (
                "side_effect_class",
                Box::new(|m: &mut pb::JobManifest| {
                    m.side_effect_class = pb::SideEffectClass::Unspecified as i32;
                }),
            ),
            (
                "dataset.sensitivity",
                Box::new(|m: &mut pb::JobManifest| {
                    m.dataset.as_mut().unwrap().sensitivity = pb::Sensitivity::Unspecified as i32;
                }),
            ),
            (
                "minimum_security_tier",
                Box::new(|m: &mut pb::JobManifest| {
                    m.minimum_security_tier = pb::SecurityTier::Unspecified as i32;
                }),
            ),
            (
                "minimum_isolation_class",
                Box::new(|m: &mut pb::JobManifest| {
                    m.minimum_isolation_class = pb::IsolationClass::Unspecified as i32;
                }),
            ),
            (
                "minimum_key_protection",
                Box::new(|m: &mut pb::JobManifest| {
                    m.minimum_key_protection = pb::KeyProtection::Unspecified as i32;
                }),
            ),
        ];
        for (field, blank) in cases {
            let manifest = verified(|m| blank(m));
            assert_eq!(
                job_requirements_from_manifest(&manifest, MEMBER),
                Err(ManifestConversionError::Unspecified(field)),
                "{field} 를 비웠는데 그 이름으로 거부하지 않았다"
            );
        }
    }

    /// ★★ 선언하지 않은 자원량은 0 으로 채우지 않는다 — 셋 **각각**.
    ///
    /// 2026-09-10 재검수가 짚었다: `UndeclaredAmount` 를 넣었는데 그것을
    /// 되돌려도 실패하는 테스트가 없었다. 위 fixture 가 셋 다 양수라서다.
    /// 셋을 한꺼번에 0 으로 두면 첫 번째 검사만 있어도 통과하므로 하나씩 둔다.
    #[test]
    fn each_undeclared_resource_amount_is_refused_by_its_own_name() {
        let cases: [(&str, fn(&mut pb::ResourceRequest)); 3] = [
            ("resources.cpu_cores", |r| r.cpu_cores = 0),
            ("resources.ram_bytes", |r| r.ram_bytes = 0),
            ("resources.workspace_bytes", |r| r.workspace_bytes = 0),
        ];
        for (field, zero) in cases {
            let manifest = verified(|m| zero(m.resources.as_mut().unwrap()));
            assert_eq!(
                job_requirements_from_manifest(&manifest, MEMBER),
                Err(ManifestConversionError::UndeclaredAmount(field)),
                "{field} 를 0 으로 뒀는데 그 이름으로 거부하지 않았다"
            );
        }
    }

    /// 대조군 — `min_vram_bytes` 0 은 **일부러** 받는다("하한 없음").
    ///
    /// ★ 이게 없으면 "0 이면 전부 거부" 로 바꿔도 위 테스트가 통과한다.
    ///   같은 0 인데 결과가 달라야 한다 — 경로가 갈라지는 대조군이다.
    #[test]
    fn a_zero_vram_floor_is_still_accepted_as_no_floor() {
        let manifest = verified(|m| {
            m.resources
                .as_mut()
                .unwrap()
                .gpu
                .as_mut()
                .unwrap()
                .min_vram_bytes = 0;
        });
        let requirements =
            job_requirements_from_manifest(&manifest, MEMBER).expect("VRAM 하한 0 은 받아야 한다");
        assert_eq!(requirements.minimum_vram_bytes_per_gpu, Some(0));
    }

    /// 중첩 메시지가 통째로 없으면 그것도 이름으로 말한다.
    #[test]
    fn a_missing_nested_message_is_named() {
        assert_eq!(
            job_requirements_from_manifest(&verified(|m| m.resources = None), MEMBER),
            Err(ManifestConversionError::MissingMessage("resources"))
        );
        assert_eq!(
            job_requirements_from_manifest(
                &verified(|m| m.resources.as_mut().unwrap().gpu = None),
                MEMBER
            ),
            Err(ManifestConversionError::MissingMessage("resources.gpu"))
        );
        assert_eq!(
            job_requirements_from_manifest(&verified(|m| m.workload = None), MEMBER),
            Err(ManifestConversionError::MissingMessage("workload"))
        );
        assert_eq!(
            job_requirements_from_manifest(&verified(|m| m.dataset = None), MEMBER),
            Err(ManifestConversionError::MissingMessage("dataset"))
        );
    }

    /// ★ `submitter_member_id` 는 Manifest 에 **없다.** caller 가 안 주면
    ///   `team_id` 로 몰래 대체하지 않고 거부한다 — 그게 없는 권위를
    ///   지어내는 것이다.
    #[test]
    fn an_absent_submitter_member_is_refused_rather_than_taken_from_team_id() {
        for blank in ["", "   "] {
            assert_eq!(
                job_requirements_from_manifest(&verified(|_| {}), blank),
                Err(ManifestConversionError::MissingCallerFact(
                    "submitter_member_id"
                )),
                "{blank:?} 를 받아들였다"
            );
        }
        // 대조 — Manifest 의 team_id 가 결과에 새어 들어가지 않는다.
        let requirements = job_requirements_from_manifest(&verified(|_| {}), MEMBER).unwrap();
        assert_eq!(requirements.submitter_member_id.as_deref(), Some(MEMBER));
        assert_ne!(requirements.submitter_member_id.as_deref(), Some("team-a"));
    }

    /// proto 가 "기본 1" 이라 적은 것은 지어내는 것이 아니라 규범이다.
    #[test]
    fn a_zero_gpu_count_becomes_the_documented_default_of_one() {
        let requirements = job_requirements_from_manifest(
            &verified(|m| {
                m.resources
                    .as_mut()
                    .unwrap()
                    .gpu
                    .as_mut()
                    .unwrap()
                    .min_count = 0
            }),
            MEMBER,
        )
        .expect("변환 실패");
        assert_eq!(requirements.minimum_gpu_count, Some(1));

        // 대조 — 0 이 아닌 값은 그대로 간다. 아니면 "항상 1" 로도 통과한다.
        let requirements = job_requirements_from_manifest(
            &verified(|m| {
                m.resources
                    .as_mut()
                    .unwrap()
                    .gpu
                    .as_mut()
                    .unwrap()
                    .min_count = 4
            }),
            MEMBER,
        )
        .expect("변환 실패");
        assert_eq!(requirements.minimum_gpu_count, Some(4));
    }

    /// 빈 모델 목록은 "제약 없음" 이다 — proto 가 그렇게 적었다.
    #[test]
    fn an_empty_model_list_means_no_constraint() {
        let requirements = job_requirements_from_manifest(
            &verified(|m| {
                m.resources
                    .as_mut()
                    .unwrap()
                    .gpu
                    .as_mut()
                    .unwrap()
                    .allowed_gpu_models
                    .clear()
            }),
            MEMBER,
        )
        .expect("변환 실패");
        assert!(requirements.allowed_gpu_models.is_empty());
    }

    /// 이 빌드가 모르는 enum 값을 조용히 넘기지 않는다.
    ///
    /// ★ 더 새로운 스키마로 서명된 Manifest 를 "기본값" 으로 읽으면
    ///   새 보안 축을 구버전이 무시한다(`CLAUDE.md` §0.2).
    #[test]
    fn an_enum_value_this_build_does_not_know_is_refused() {
        let error =
            job_requirements_from_manifest(&verified(|m| m.minimum_security_tier = 99), MEMBER)
                .expect_err("모르는 값을 받아들였다");
        assert_eq!(
            error,
            ManifestConversionError::UnknownEnumValue {
                field: "minimum_security_tier",
                value: 99
            }
        );
    }

    /// ★★ **여섯 축의 모든 유효 변형이 제 값으로 넘어가는가.**
    ///
    /// 2026-09-07 독립 검수 지적으로 추가했다. 그전까지 이 파일이
    /// 직접 검사한 것은 **30개 유효 변형 중 12개**뿐이었다 —
    /// `Unspecified` 여섯과 성공 경로의 **대표값 여섯**. 나머지 18개는
    /// 아무도 안 봤다.
    ///
    /// ★ 내가 evidence 초안에 "축 전수 뮤테이션을 돌렸다" 고 적었는데,
    ///   그 '전수' 는 **거부 축 여섯**이었지 **값 서른**이 아니었다.
    ///   검수가 그 차이를 짚었다.
    ///
    /// ★★ 기대값을 **손으로 적는다.** 변환 함수를 다시 돌려 기대값을
    ///   만들면 자기가 자기를 확인하는 것이라 아무것도 증명하지 않는다.
    #[test]
    fn every_valid_variant_of_every_axis_maps_to_its_own_value() {
        // workload.class — proto 의 여섯 변형
        for (raw, want) in [
            (pb::WorkloadClass::Training, WorkloadClass::Training),
            (pb::WorkloadClass::Inference, WorkloadClass::Inference),
            (
                pb::WorkloadClass::Preprocessing,
                WorkloadClass::Preprocessing,
            ),
            (pb::WorkloadClass::Evaluation, WorkloadClass::Evaluation),
            (pb::WorkloadClass::Rendering, WorkloadClass::Rendering),
            (pb::WorkloadClass::Other, WorkloadClass::Other),
        ] {
            let got = job_requirements_from_manifest(
                &verified(|m| m.workload.as_mut().unwrap().class = raw as i32),
                MEMBER,
            )
            .unwrap_or_else(|e| panic!("workload.class {raw:?} 를 거부했다: {e:?}"));
            assert_eq!(
                got.workload_class,
                Some(want),
                "workload.class {raw:?} 가 엉뚱한 값으로 갔다"
            );
        }

        for (raw, want) in [
            (pb::SideEffectClass::Pure, SideEffectClass::Pure),
            (pb::SideEffectClass::Idempotent, SideEffectClass::Idempotent),
            (
                pb::SideEffectClass::SideEffecting,
                SideEffectClass::SideEffecting,
            ),
        ] {
            let got = job_requirements_from_manifest(
                &verified(|m| m.side_effect_class = raw as i32),
                MEMBER,
            )
            .unwrap_or_else(|e| panic!("side_effect_class {raw:?} 를 거부했다: {e:?}"));
            assert_eq!(got.side_effect_class, Some(want));
        }

        for (raw, want) in [
            (pb::Sensitivity::Public, Sensitivity::Public),
            (pb::Sensitivity::Internal, Sensitivity::Internal),
            (pb::Sensitivity::Sensitive, Sensitivity::Sensitive),
        ] {
            let got = job_requirements_from_manifest(
                &verified(|m| m.dataset.as_mut().unwrap().sensitivity = raw as i32),
                MEMBER,
            )
            .unwrap_or_else(|e| panic!("sensitivity {raw:?} 를 거부했다: {e:?}"));
            assert_eq!(got.sensitivity, Some(want));
        }

        for (raw, want) in [
            (pb::SecurityTier::S0, SecurityTier::S0),
            (pb::SecurityTier::S1, SecurityTier::S1),
            (pb::SecurityTier::S2, SecurityTier::S2),
            (pb::SecurityTier::S3, SecurityTier::S3),
            (pb::SecurityTier::S4, SecurityTier::S4),
            (pb::SecurityTier::S5, SecurityTier::S5),
        ] {
            let got = job_requirements_from_manifest(
                &verified(|m| m.minimum_security_tier = raw as i32),
                MEMBER,
            )
            .unwrap_or_else(|e| panic!("security_tier {raw:?} 를 거부했다: {e:?}"));
            assert_eq!(got.minimum_security_tier, Some(want));
        }

        for (raw, want) in [
            (pb::IsolationClass::Restricted, IsolationClass::Restricted),
            (pb::IsolationClass::Contained, IsolationClass::Contained),
            (pb::IsolationClass::Virtualized, IsolationClass::Virtualized),
        ] {
            let got = job_requirements_from_manifest(
                &verified(|m| m.minimum_isolation_class = raw as i32),
                MEMBER,
            )
            .unwrap_or_else(|e| panic!("isolation_class {raw:?} 를 거부했다: {e:?}"));
            assert_eq!(got.minimum_isolation_class, Some(want));
        }

        for (raw, want) in [
            (pb::KeyProtection::K0, KeyProtection::K0),
            (pb::KeyProtection::K1, KeyProtection::K1),
            (pb::KeyProtection::K2, KeyProtection::K2),
        ] {
            let got = job_requirements_from_manifest(
                &verified(|m| m.minimum_key_protection = raw as i32),
                MEMBER,
            )
            .unwrap_or_else(|e| panic!("key_protection {raw:?} 를 거부했다: {e:?}"));
            assert_eq!(got.minimum_key_protection, Some(want));
        }
    }
}
