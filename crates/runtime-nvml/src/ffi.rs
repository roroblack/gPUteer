//! NVML C ABI 를 런타임에 여는 얇은 층.
//!
//! # 여기서 지키는 규칙
//!
//! ```text
//! 반환 코드를 무시하지 않는다   0(SUCCESS) 이 아니면 전부 typed error 다
//! NOT_SUPPORTED 는 오류가 아니다  "이 장치는 그 기능이 없다" 는 사실이므로
//!                                None 으로 올린다
//! 버퍼는 NVML 이 정한 상한을 쓴다  임의로 정하면 잘린 문자열을 값으로 쓴다
//! ```
//!
//! ★ **`_v2` 심볼을 쓴다.** 구버전 심볼(`nvmlInit`, `nvmlDeviceGetCount`)은
//!   같은 이름으로 다른 구조체 레이아웃을 기대하는 경우가 있어, 섞어 쓰면
//!   조용히 잘못된 값을 읽는다.

use std::ffi::{c_char, c_int, c_uint, c_void, CStr};

use libloading::{Library, Symbol};

use crate::NvmlError;

/// NVML 반환 코드 중 이 크레이트가 이름으로 구분하는 것들.
const NVML_SUCCESS: c_int = 0;
const NVML_ERROR_NOT_SUPPORTED: c_int = 3;

/// `nvmlDeviceGetName` 의 버퍼 상한(NVML_DEVICE_NAME_V2_BUFFER_SIZE).
///
/// ★ 첫 구현은 64(NVML_DEVICE_NAME_BUFFER_SIZE)를 썼다 — 독립 검수가
///   지적했다. 64 는 오래된 일반 이름 상수이고 **이 함수용 상한은
///   96** 이다. 이름이 63바이트를 넘는 장치가 오면
///   `INSUFFICIENT_SIZE` 로 **관측 전체**가 실패한다 — 장치 하나의
///   긴 이름 때문에 GPU 목록 전체를 못 읽는다.
const NAME_BUFFER: usize = 96;
/// `nvmlDeviceGetUUID` 상한(NVML_DEVICE_UUID_V2_BUFFER_SIZE).
const UUID_BUFFER: usize = 96;
/// `nvmlSystemGetDriverVersion` 상한(NVML_SYSTEM_DRIVER_VERSION_BUFFER_SIZE).
const DRIVER_BUFFER: usize = 80;

/// 반환 코드를 사람이 읽는 이름으로 바꾼다.
///
/// 숫자만 남기면 로그를 보고 원인을 알 수 없다 — `CLAUDE.md` §3
/// "오류 메시지가 사실을 잘못 전하지 않게 한다" 의 최소 이행이다.
fn meaning(code: c_int) -> &'static str {
    match code {
        0 => "SUCCESS",
        1 => "UNINITIALIZED",
        2 => "INVALID_ARGUMENT",
        3 => "NOT_SUPPORTED",
        4 => "NO_PERMISSION",
        6 => "NOT_FOUND",
        7 => "INSUFFICIENT_SIZE",
        9 => "DRIVER_NOT_LOADED",
        10 => "TIMEOUT",
        12 => "LIBRARY_NOT_FOUND",
        13 => "FUNCTION_NOT_FOUND",
        15 => "GPU_IS_LOST",
        999 => "UNKNOWN",
        _ => "(이 크레이트가 이름을 모르는 코드)",
    }
}

/// 이 플랫폼에서 시도할 NVML 라이브러리 이름들.
///
/// ★ Windows 는 `nvml.dll` 이 `System32` 에 있으면 이름만으로 열리지만,
///   드라이버 버전에 따라 NVSMI 디렉터리에만 있는 경우가 있어 그 경로도
///   시도한다. Linux 는 `.so.1`(ABI 고정 심볼릭 링크)을 먼저 본다 —
///   `.so` 는 개발 패키지에만 있어서 실행 환경에 없을 수 있다.
fn candidate_paths() -> Vec<String> {
    if cfg!(windows) {
        let mut paths = vec!["nvml.dll".to_string()];
        if let Ok(program_files) = std::env::var("ProgramW6432") {
            paths.push(format!(
                "{program_files}/NVIDIA Corporation/NVSMI/nvml.dll"
            ));
        }
        paths
    } else {
        vec![
            "libnvidia-ml.so.1".to_string(),
            "libnvidia-ml.so".to_string(),
        ]
    }
}

pub(crate) struct Nvml {
    library: Library,
}

impl Nvml {
    pub(crate) fn load() -> Result<Self, NvmlError> {
        let mut failures = Vec::new();
        for path in candidate_paths() {
            // SAFETY: NVML 은 프로세스 전역 초기화를 하는 정상적인 공유
            // 라이브러리다. 여는 것 자체는 임의 코드 실행이 아니지만,
            // `Library::new` 는 정적 초기화자를 돌리므로 unsafe 다.
            match unsafe { Library::new(&path) } {
                Ok(library) => return Ok(Self { library }),
                Err(error) => failures.push(format!("{path}: {error}")),
            }
        }
        Err(NvmlError::LibraryUnavailable {
            detail: failures.join(" / "),
        })
    }

    fn symbol<T>(&self, name: &str) -> Result<Symbol<'_, T>, NvmlError> {
        // SAFETY: 아래 각 호출부가 NVML 헤더와 같은 시그니처를 선언한다.
        // 이름이 틀리면 여기서 typed error 로 끝난다 — 틀린 시그니처로
        // 부르는 것보다 못 찾는 편이 안전하다.
        unsafe { self.library.get(name.as_bytes()) }.map_err(|error| NvmlError::SymbolMissing {
            symbol: name.to_string(),
            detail: error.to_string(),
        })
    }

    fn check(call: &str, code: c_int) -> Result<(), NvmlError> {
        if code == NVML_SUCCESS {
            Ok(())
        } else {
            Err(NvmlError::CallFailed {
                call: call.to_string(),
                code,
                meaning: meaning(code),
            })
        }
    }

    pub(crate) fn init(&self) -> Result<(), NvmlError> {
        let f: Symbol<unsafe extern "C" fn() -> c_int> = self.symbol("nvmlInit_v2")?;
        Self::check("nvmlInit_v2", unsafe { f() })
    }

    /// 닫기 실패는 조회 결과를 바꾸지 않으므로 눈에만 보이게 남긴다.
    ///
    /// ★ 조용히 버리지는 않는다(`CLAUDE.md` §3). 다만 이미 반환할 1차
    ///   결과가 있는 상황에서 두 오류를 하나로 합칠 방법이 없어,
    ///   `runtime-windows` 의 `TerminateProcess` 실패 처리와 같은 방식으로
    ///   표준 오류에 남긴다.
    pub(crate) fn shutdown_ignoring_error(&self) {
        match self.symbol::<unsafe extern "C" fn() -> c_int>("nvmlShutdown") {
            Ok(f) => {
                let code = unsafe { f() };
                if code != NVML_SUCCESS {
                    eprintln!(
                        "gputeer-runtime-nvml: nvmlShutdown 실패(코드 {code}: {}) — \
                         NVML 이 초기화된 채 남아 있을 수 있다",
                        meaning(code)
                    );
                }
            }
            Err(error) => eprintln!("gputeer-runtime-nvml: nvmlShutdown 심볼 없음 — {error}"),
        }
    }

    pub(crate) fn device_count(&self) -> Result<u32, NvmlError> {
        let f: Symbol<unsafe extern "C" fn(*mut c_uint) -> c_int> =
            self.symbol("nvmlDeviceGetCount_v2")?;
        let mut count: c_uint = 0;
        Self::check("nvmlDeviceGetCount_v2", unsafe { f(&mut count) })?;
        Ok(count)
    }

    pub(crate) fn device_handle(&self, index: u32) -> Result<*mut c_void, NvmlError> {
        let f: Symbol<unsafe extern "C" fn(c_uint, *mut *mut c_void) -> c_int> =
            self.symbol("nvmlDeviceGetHandleByIndex_v2")?;
        let mut device: *mut c_void = std::ptr::null_mut();
        Self::check("nvmlDeviceGetHandleByIndex_v2", unsafe {
            f(index, &mut device)
        })?;
        Ok(device)
    }

    pub(crate) fn driver_version(&self) -> Result<String, NvmlError> {
        let f: Symbol<unsafe extern "C" fn(*mut c_char, c_uint) -> c_int> =
            self.symbol("nvmlSystemGetDriverVersion")?;
        let mut buffer = vec![0i8 as c_char; DRIVER_BUFFER];
        Self::check("nvmlSystemGetDriverVersion", unsafe {
            f(buffer.as_mut_ptr(), DRIVER_BUFFER as c_uint)
        })?;
        read_c_string(&buffer, "nvmlSystemGetDriverVersion")
    }

    pub(crate) fn cuda_driver_version(&self) -> Result<i32, NvmlError> {
        let f: Symbol<unsafe extern "C" fn(*mut c_int) -> c_int> =
            self.symbol("nvmlSystemGetCudaDriverVersion_v2")?;
        let mut version: c_int = 0;
        Self::check("nvmlSystemGetCudaDriverVersion_v2", unsafe {
            f(&mut version)
        })?;
        Ok(version)
    }

    pub(crate) fn device_uuid(&self, device: *mut c_void) -> Result<String, NvmlError> {
        let f: Symbol<unsafe extern "C" fn(*mut c_void, *mut c_char, c_uint) -> c_int> =
            self.symbol("nvmlDeviceGetUUID")?;
        let mut buffer = vec![0i8 as c_char; UUID_BUFFER];
        Self::check("nvmlDeviceGetUUID", unsafe {
            f(device, buffer.as_mut_ptr(), UUID_BUFFER as c_uint)
        })?;
        read_c_string(&buffer, "nvmlDeviceGetUUID")
    }

    pub(crate) fn device_name(&self, device: *mut c_void) -> Result<String, NvmlError> {
        let f: Symbol<unsafe extern "C" fn(*mut c_void, *mut c_char, c_uint) -> c_int> =
            self.symbol("nvmlDeviceGetName")?;
        let mut buffer = vec![0i8 as c_char; NAME_BUFFER];
        Self::check("nvmlDeviceGetName", unsafe {
            f(device, buffer.as_mut_ptr(), NAME_BUFFER as c_uint)
        })?;
        read_c_string(&buffer, "nvmlDeviceGetName")
    }

    /// `(total, free, used)` 바이트.
    pub(crate) fn memory_info(&self, device: *mut c_void) -> Result<(u64, u64, u64), NvmlError> {
        // nvmlMemory_t 는 { unsigned long long total, free, used } 다.
        #[repr(C)]
        struct NvmlMemory {
            total: u64,
            free: u64,
            used: u64,
        }
        let f: Symbol<unsafe extern "C" fn(*mut c_void, *mut NvmlMemory) -> c_int> =
            self.symbol("nvmlDeviceGetMemoryInfo")?;
        let mut memory = NvmlMemory {
            total: 0,
            free: 0,
            used: 0,
        };
        Self::check("nvmlDeviceGetMemoryInfo", unsafe {
            f(device, &mut memory)
        })?;
        Ok((memory.total, memory.free, memory.used))
    }

    pub(crate) fn compute_capability(
        &self,
        device: *mut c_void,
    ) -> Result<Option<String>, NvmlError> {
        let f: Symbol<unsafe extern "C" fn(*mut c_void, *mut c_int, *mut c_int) -> c_int> =
            self.symbol("nvmlDeviceGetCudaComputeCapability")?;
        let (mut major, mut minor): (c_int, c_int) = (0, 0);
        let code = unsafe { f(device, &mut major, &mut minor) };
        if code == NVML_ERROR_NOT_SUPPORTED {
            return Ok(None);
        }
        Self::check("nvmlDeviceGetCudaComputeCapability", code)?;
        Ok(Some(format!("{major}.{minor}")))
    }

    /// MIG 가 켜져 있는가. 지원하지 않는 장치는 `None`.
    pub(crate) fn mig_enabled(&self, device: *mut c_void) -> Result<Option<bool>, NvmlError> {
        // NVML_DEVICE_MIG_ENABLE = 1, DISABLE = 0.
        let f: Symbol<unsafe extern "C" fn(*mut c_void, *mut c_uint, *mut c_uint) -> c_int> =
            match self.symbol("nvmlDeviceGetMigMode") {
                Ok(f) => f,
                // 오래된 NVML 에는 이 심볼 자체가 없다 — 그건 "MIG 개념이
                // 없는 드라이버" 라는 사실이므로 오류가 아니라 None 이다.
                Err(NvmlError::SymbolMissing { .. }) => return Ok(None),
                Err(other) => return Err(other),
            };
        let (mut current, mut pending): (c_uint, c_uint) = (0, 0);
        let code = unsafe { f(device, &mut current, &mut pending) };
        if code == NVML_ERROR_NOT_SUPPORTED {
            return Ok(None);
        }
        Self::check("nvmlDeviceGetMigMode", code)?;
        Ok(Some(mig_enabled_from_modes(current, pending)))
    }

}

/// NUL 로 끝나는 C 문자열을 읽는다.
///
/// ★ NVML 이 버퍼를 다 채우고 NUL 을 안 넣는 경우를 대비해, 버퍼 마지막
///   바이트를 강제로 NUL 로 만들지 않고 **NUL 이 없으면 오류**로 낸다.
///   잘린 문자열을 값으로 쓰면 UUID 가 다른데 같아 보일 수 있다.
fn read_c_string(buffer: &[c_char], call: &str) -> Result<String, NvmlError> {
    let bytes: Vec<u8> = buffer.iter().map(|value| *value as u8).collect();
    let nul = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| NvmlError::UnterminatedString {
            call: call.to_string(),
        })?;
    CStr::from_bytes_with_nul(&bytes[..=nul])
        .map_err(|_| NvmlError::BadString {
            call: call.to_string(),
        })?
        .to_str()
        .map(str::to_owned)
        .map_err(|_| NvmlError::BadString {
            call: call.to_string(),
        })
}

/// `nvmlDeviceGetMigMode` 가 돌려준 두 값 중 **무엇으로 판정하는가**.
///
/// ★ `pending` 이 아니라 `current` 다. `pending` 은 **재부팅 뒤에 적용될**
///   값이라, 그걸로 지금을 판단하면 아직 분할되지 않은 GPU 를 분할됐다고
///   보거나 그 반대가 된다.
///
/// ★ 이 판정을 함수로 떼어낸 이유는 **GPU 없이 재기 위해서**다(`DoD-56` 이
///   "지원 장치가 없어 합성값 단위 테스트뿐" 이라 남긴 부채). FFI 호출 안에
///   묻어 두면 `pending` 으로 바꾸는 회귀를 아무도 잡지 못한다.
pub(crate) fn mig_enabled_from_modes(current: c_uint, pending: c_uint) -> bool {
    // `pending` 은 **의도적으로 안 쓴다** — 쓰면 위 주석이 거짓이 된다.
    let _ = pending;
    current == NVML_DEVICE_MIG_ENABLE
}

/// `NVML_DEVICE_MIG_ENABLE`. 꺼짐은 0 이다.
const NVML_DEVICE_MIG_ENABLE: c_uint = 1;

#[cfg(test)]
mod tests {
    use super::*;

    /// 버퍼 크기를 **숫자로** 못박는다.
    ///
    /// ★ 내 뮤테이션이 이 공백을 잡았다 — 아래 경계 테스트들은 버퍼를
    ///   `NAME_BUFFER` 로 만들기 때문에, 상수를 64 로 줄이면 **테스트
    ///   버퍼도 같이 줄어** 그대로 통과했다. 재려는 대상으로 기대값을
    ///   만든 셈이다(`DoD-64` 가 겪은 것과 같은 유형).
    ///
    /// 그래서 값 자체를 리터럴로 고정한다. 첫 구현은 64
    /// (`NVML_DEVICE_NAME_BUFFER_SIZE`)를 썼고, 독립 검수가 v2 API 의
    /// 상한이 **96**(`NVML_DEVICE_NAME_V2_BUFFER_SIZE`)임을 짚었다 —
    /// 63바이트 넘는 이름이 오면 잘리거나 오류가 된다.
    #[test]
    fn the_string_buffers_match_the_nvml_v2_size() {
        assert_eq!(
            NAME_BUFFER, 96,
            "이름 버퍼가 NVML v2 상한이 아니다 — 긴 장치 이름이 잘린다"
        );
        assert_eq!(
            UUID_BUFFER, 96,
            "UUID 버퍼가 NVML v2 상한이 아니다 — 다른 장치가 같아 보일 수 있다"
        );
    }

    /// ★ `DoD-56` 이 "63바이트 넘는 이름을 본 적 없다" 며 남긴 부채.
    ///
    /// 실제 장치 없이도 **경계 자체는** 잴 수 있다 — 버퍼를 손으로 만든다.
    #[test]
    fn a_name_that_exactly_fills_the_buffer_without_nul_is_an_error() {
        // 96바이트를 꽉 채우고 NUL 이 없다 — NVML 이 자를 수 있는 상황.
        let full: Vec<c_char> = vec![b'x' as c_char; NAME_BUFFER];
        let error = read_c_string(&full, "nvmlDeviceGetName")
            .expect_err("NUL 이 없으면 오류여야 한다");
        assert!(
            matches!(error, NvmlError::UnterminatedString { ref call } if call == "nvmlDeviceGetName"),
            "잘린 문자열을 값으로 받아들였다: {error}"
        );
    }

    /// 경계 **바로 아래**는 정상이어야 한다 — "전부 거부" 로 바꿔도
    /// 위 테스트가 통과하므로 이 대조가 없으면 공허해진다.
    #[test]
    fn a_name_one_byte_short_of_the_buffer_reads_back_whole() {
        let mut buffer: Vec<c_char> = vec![b'x' as c_char; NAME_BUFFER - 1];
        buffer.push(0);
        let name = read_c_string(&buffer, "nvmlDeviceGetName").expect("읽혀야 한다");
        assert_eq!(
            name.len(),
            NAME_BUFFER - 1,
            "경계 바로 아래 이름이 잘렸다"
        );
        assert!(name.chars().all(|c| c == 'x'), "내용이 바뀌었다: {name}");
    }

    /// UUID 버퍼도 같은 경계를 갖는다 — 이름만 재고 UUID 를 안 재면
    /// 한쪽만 고친 회귀를 놓친다.
    #[test]
    fn the_uuid_buffer_has_the_same_boundary() {
        let full: Vec<c_char> = vec![b'G' as c_char; UUID_BUFFER];
        assert!(
            read_c_string(&full, "nvmlDeviceGetUUID").is_err(),
            "NUL 없는 UUID 를 값으로 받아들였다 — 다른 장치가 같아 보일 수 있다"
        );
    }

    /// ★ `DoD-56` 이 남긴 부채 — **`current` 로 판정하는가.**
    ///
    /// 두 값을 어긋나게 줘서, `pending` 으로 바꾸면 **두 경우 다** 답이
    /// 뒤집히게 만든다.
    #[test]
    fn mig_mode_is_read_from_current_not_pending() {
        assert!(
            !mig_enabled_from_modes(0, 1),
            "재부팅 뒤에 켜질 예정인 GPU 를 지금 분할됐다고 봤다"
        );
        assert!(
            mig_enabled_from_modes(1, 0),
            "지금 분할된 GPU 를 재부팅 뒤 꺼질 예정이라고 안 봤다"
        );
        // 두 값이 같으면 어느 쪽을 읽든 답이 같다 — 판별력이 없으므로
        // 그것만으로는 이 계약을 고정하지 못한다. 대조로만 둔다.
        assert!(mig_enabled_from_modes(1, 1));
        assert!(!mig_enabled_from_modes(0, 0));
    }
}
