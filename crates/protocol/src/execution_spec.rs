//! 검증된 `JobManifest` 에서 **실행 지시**를 뽑아내는 순수 kernel.
//!
//! # 이 kernel 이 있는 이유
//!
//! `proto/job.proto` 의 `ExecutionGrant` 는 `manifest`(필드 3)와
//! `manifest_hash`(필드 4)를 **이미 규범으로 갖고 있다.** 그런데
//! 2026-08-27 기준 실측으로:
//!
//! ```text
//! Coordinator   이 필드를 한 번도 채우지 않는다 (참조 0건)
//! Agent         이 필드를 한 번도 읽지 않는다   (참조 0건)
//! ```
//!
//! 그래서 Agent 는 Grant/Lease 를 검증하고 시작 마커를 쓴 뒤
//! **무엇을 실행해야 하는지 모르는 채로** 끝난다. 한편
//! `crates/runtime-windows` 에는 프로세스를 실제로 띄우는 코드가
//! 이미 있다(`create_constrained_child()` — `CreateProcessW`
//! `CREATE_SUSPENDED` -> `AssignProcessToJobObject` -> `ResumeThread`).
//!
//! **띄울 도구는 있고 띄울 대상이 없다.** 이 kernel 이 그 사이를
//! 메우는 첫 조각이다 — 검증된 Manifest 를 "실행 지시" 로 바꾼다.
//!
//! # 이 kernel 이 하지 않는 것
//!
//! ```text
//! 프로세스를 띄우지 않는다        (runtime-windows 의 몫)
//! 명령줄 문자열을 만들지 않는다    (플랫폼마다 인용 규칙이 다르다)
//! 상태를 전이시키지 않는다        (state-machines.md 표를 건드리지 않는다)
//! 서명을 검증하지 않는다          (`Verified<M>` 를 caller 가 넘긴다)
//! 파일시스템·환경변수·시계를 읽지 않는다
//! ```
//!
//! 특히 **상태 전이를 하지 않는다** 는 점이 중요하다.
//! `state-machines.md` §3 은 `CREATED -> STARTING` 의 effect 를
//! "workspace 생성" 으로, `STARTING -> RUNNING` 을 "프로세스 기동 +
//! 첫 progress 수신" 으로 정한다. 이 kernel 은 그 어느 쪽도 하지
//! 않으므로 규범 표를 **우회하지 않는다** — 아직 건드리지 않을 뿐이다.
//!
//! # 검증하는 것과 하지 않는 것
//!
//! 규범에 없는 정책을 발명하지 않는다(`CLAUDE.md` §0.4). 여기서
//! 거부하는 것은 **규범이 이미 정했거나 OS 가 표현할 수 없는 것**뿐이다.
//!
//! ```text
//! 거부   빈 entrypoint            식별자 부재는 fail closed (DoD-41 선례)
//! 거부   NUL 바이트가 든 문자열    Windows·POSIX 어느 쪽도 프로세스 인자로 전달 불가
//! 거부   빈 환경변수 이름          이름 없는 환경변수는 표현 자체가 불가능
//! 거부   환경변수 이름의 '='       POSIX environ 이 표현할 수 없다
//!
//! 안 함  entrypoint 경로 형태 제한   규범에 없다. 발명하지 않는다
//! 안 함  args 개수·길이 제한        규범에 없다
//! 안 함  실행 파일 존재 확인        I/O 다. 이 kernel 밖이다
//! ```
//!
//! # 결정성
//!
//! `env_vars` 는 protobuf `map` 이라 순서가 없다. `signing.md` 규칙 2c
//! 가 canonical 인코딩에서 **key 바이트 오름차순** 정렬을 이미 정했으므로
//! 같은 규칙으로 정렬해 내보낸다 — 같은 Manifest 는 항상 같은 지시를
//! 만든다.

use crate::pb;
use crate::signing::Verified;
use std::collections::BTreeMap;

/// 검증된 Manifest 에서 뽑아낸 실행 지시.
///
/// 플랫폼 중립이다. Windows 명령줄 인용이나 POSIX `execve` 배열로
/// 바꾸는 것은 각 runtime crate 의 몫이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionSpec {
    pub job_id: String,
    /// 실행 대상. 이 kernel 은 존재 여부를 확인하지 않는다.
    pub entrypoint: String,
    /// `entrypoint` 자신은 포함하지 않는다. Manifest 의 `args` 순서 그대로다.
    pub args: Vec<String>,
    /// key 바이트 오름차순으로 정렬됨(`signing.md` 규칙 2c 와 같은 규칙).
    pub env_vars: BTreeMap<String, String>,
}

/// 실행 지시를 만들 수 없는 이유. 판정이 아니라 오류다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionSpecError {
    /// `job_id` 가 비었다.
    BlankJobId,
    /// `entrypoint` 가 비었거나 공백뿐이다.
    BlankEntrypoint,
    /// 문자열에 NUL 바이트가 있다 — 어떤 OS 도 프로세스 인자로 전달할 수 없다.
    NulByte { field: SpecField },
    /// 환경변수 이름이 비었다.
    BlankEnvVarName,
    /// 환경변수 이름에 `=` 가 있다 — POSIX `environ` 이 표현할 수 없다.
    EnvVarNameContainsEquals { name: String },
}

/// 어느 자리에서 문제가 났는지 가리킨다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecField {
    JobId,
    Entrypoint,
    Arg { index: usize },
    EnvVarName { name: String },
    EnvVarValue { name: String },
}

impl std::fmt::Display for ExecutionSpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlankJobId => write!(f, "job_id 가 비었다"),
            Self::BlankEntrypoint => write!(f, "entrypoint 가 비었다"),
            Self::NulByte { field } => write!(f, "NUL 바이트가 들어 있다: {field:?}"),
            Self::BlankEnvVarName => write!(f, "환경변수 이름이 비었다"),
            Self::EnvVarNameContainsEquals { name } => {
                write!(f, "환경변수 이름에 '=' 가 있다: {name:?}")
            }
        }
    }
}

impl std::error::Error for ExecutionSpecError {}

/// 검증된 Manifest 에서 실행 지시를 만든다.
///
/// **`&Verified<pb::JobManifest>` 만 받는다.** raw `pb::JobManifest` 로는
/// 호출할 수 없다 — 서명 검증 전 필드를 실행 지시로 바꾸는 것을 타입
/// 수준에서 막는다(`CLAUDE.md` §0.2).
///
/// ```compile_fail
/// # use gputeer_protocol::{pb, execution_spec::derive_execution_spec};
/// let raw = pb::JobManifest::default();
/// // raw 는 Verified 가 아니므로 컴파일되지 않는다.
/// let _ = derive_execution_spec(&raw);
/// ```
pub fn derive_execution_spec(
    manifest: &Verified<pb::JobManifest>,
) -> Result<ExecutionSpec, ExecutionSpecError> {
    // ★ 서명 검증이 끝난 뒤에만 필드를 본다.
    let manifest = manifest.get();

    if manifest.job_id.trim().is_empty() {
        return Err(ExecutionSpecError::BlankJobId);
    }
    reject_nul(&manifest.job_id, SpecField::JobId)?;

    if manifest.entrypoint.trim().is_empty() {
        return Err(ExecutionSpecError::BlankEntrypoint);
    }
    reject_nul(&manifest.entrypoint, SpecField::Entrypoint)?;

    for (index, arg) in manifest.args.iter().enumerate() {
        // ★ 빈 인자는 거부하지 않는다. 빈 문자열은 정당한 인자이고
        //   규범이 금지한 적이 없다 — 발명하지 않는다.
        reject_nul(arg, SpecField::Arg { index })?;
    }

    // `map` 은 순서가 없다. `BTreeMap` 으로 모아 key 바이트 오름차순을
    // 강제한다 — 같은 Manifest 가 항상 같은 지시를 만들어야 한다.
    let mut env_vars = BTreeMap::new();
    for (name, value) in &manifest.env_vars {
        if name.is_empty() {
            return Err(ExecutionSpecError::BlankEnvVarName);
        }
        if name.contains('=') {
            return Err(ExecutionSpecError::EnvVarNameContainsEquals { name: name.clone() });
        }
        reject_nul(name, SpecField::EnvVarName { name: name.clone() })?;
        reject_nul(value, SpecField::EnvVarValue { name: name.clone() })?;
        env_vars.insert(name.clone(), value.clone());
    }

    Ok(ExecutionSpec {
        job_id: manifest.job_id.clone(),
        entrypoint: manifest.entrypoint.clone(),
        args: manifest.args.clone(),
        env_vars,
    })
}

fn reject_nul(value: &str, field: SpecField) -> Result<(), ExecutionSpecError> {
    if value.contains('\0') {
        return Err(ExecutionSpecError::NulByte { field });
    }
    Ok(())
}
