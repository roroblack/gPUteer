//! NVML 을 **런타임에 동적으로 열어** 실제 GPU 사실을 읽는 계층.
//!
//! # 왜 이 크레이트가 따로 있는가
//!
//! `crates/scheduler` 의 `gpu_scope_candidate()`(`DoD-55`)는 GPU 관측을
//! **입력으로 받는** 순수 kernel 이다. 계산은 이미 있는데 그 입력을
//! 실제 하드웨어에서 만드는 곳이 없었다 — 이 크레이트가 그 자리다.
//!
//! # 이 크레이트가 하지 않는 것
//!
//! ```text
//! provenance 판정   읽은 값이 "권위 있는 관측" 인지 정하지 않는다.
//!                   scheduler 의 ProvenanceGate 는 caller 가 채운다 —
//!                   여기서 Verified 를 붙이면 "NVML 이 말했으니 맞다" 가
//!                   되어, 서명·멤버십 검증을 우회하는 뒷문이 된다.
//! 서명·저장·전이    아무것도 안 한다. 조회만 한다.
//! GPU 점유·해제     할당은 이 계층의 일이 아니다.
//! ```
//!
//! 그래서 scheduler 의 타입을 반환하지 않고 **자기 타입**을 반환한다.
//! 변환은 호출부가 명시적으로 한다 — 그 지점이 provenance 를 정하는
//! 지점이기 때문에, 자동으로 넘어가면 안 된다.
//!
//! # 왜 `nvml-wrapper` 를 안 쓰고 직접 여는가
//!
//! 이 저장소의 개발 기계에는 NVIDIA GPU 가 없다(`CLAUDE.md` 환경 주의사항).
//! 링크 시점에 NVML 을 요구하면 **빌드 자체가 GPU 있는 기계를 요구**하게
//! 되어, GPU 없는 곳에서는 컴파일도 못 한다. `libloading` 으로 실행 시점에
//! 열면 라이브러리가 없을 때 typed error 로 실패할 뿐 빌드는 통과한다.
//!
//! # "GPU 0개" 와 "확인 불가" 를 구분한다
//!
//! NVML 이 없거나 초기화에 실패하면 **빈 목록을 돌려주지 않고** 오류를
//! 낸다. 둘을 같게 만들면 "이 기계에 GPU 가 없다" 와 "GPU 가 있는지
//! 모른다" 가 구분되지 않아, 스케줄러가 후자를 전자로 착각한다
//! (`CLAUDE.md` §1 — 모르면 비워 두고, 추정으로 채우지 않는다).

use std::ffi::c_void;

mod ffi;

/// 한 GPU 에 대해 NVML 이 실제로 답한 값.
///
/// 필드가 `Option` 인 것은 **NVML 이 그 항목을 지원하지 않는 경우**가
/// 실재하기 때문이다(예: MIG 는 데이터센터 GPU 전용이라 소비자 GPU 에서
/// `NOT_SUPPORTED` 가 온다). 지원 안 함을 0 이나 기본값으로 채우지 않는다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NvmlGpu {
    /// NVML 이 보고한 UUID. 이 크레이트가 만드는 유일한 안정 식별자다.
    pub uuid: String,
    /// 이 부팅에서의 장치 순번. 재부팅을 넘어 안정적이지 않다 —
    /// 식별자로 쓰지 마라. 진단용으로만 남긴다.
    pub index: u32,
    pub name: String,
    pub total_vram_bytes: u64,
    pub free_vram_bytes: u64,
    pub used_vram_bytes: u64,
    /// `"8.9"` 같은 문자열. scheduler 의 `compute_capability` 와 같은 모양이다.
    pub compute_capability: Option<String>,
    /// MIG 가 **켜져 있는가**. `None` 은 이 장치가 MIG 를 지원하지 않는다는
    /// 뜻이고, `Some(false)` 는 지원하지만 꺼져 있다는 뜻이다 — 다르다.
    pub mig_enabled: Option<bool>,
}

impl NvmlGpu {
    /// 이 GPU 가 `scheduler` 의 `Partitioned` 로 보고돼야 하는가.
    ///
    /// ★ MIG 가 켜져 있으면 하나의 물리 GPU 가 여러 인스턴스로 쪼개져
    ///   있으므로, 전체 VRAM 을 쓸 수 있다고 가정하면 안 된다.
    ///   `gpu_scope_candidate()` 는 `Partitioned` 를 **항상 거부**하므로
    ///   (`DoD-55`), 이 신호를 정확히 만들어야 그 거부가 실제로 걸린다.
    pub fn is_partitioned(&self) -> bool {
        self.mig_enabled == Some(true)
    }
}

/// 한 번의 조회로 얻은 전체 스냅샷.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NvmlSnapshot {
    pub driver_version: String,
    /// NVML 이 보고한 CUDA 드라이버 버전(예: 13020 = 13.2).
    pub cuda_driver_version: i32,
    /// 장치 목록. **UUID 오름차순으로 정렬돼 있다** — 열거 순서가
    /// 실행마다 달라져도 같은 스냅샷이 나오게 하기 위해서다.
    pub gpus: Vec<NvmlGpu>,
}

/// 조회가 실패한 이유. **전부 "값을 모른다" 이지 "GPU 가 없다" 가 아니다.**
#[derive(Debug, thiserror::Error)]
pub enum NvmlError {
    #[error("NVML_UNAVAILABLE: NVML 라이브러리를 열 수 없다 — {detail}")]
    LibraryUnavailable { detail: String },
    #[error("NVML_SYMBOL_MISSING: NVML 에 필요한 함수가 없다({symbol}) — {detail}")]
    SymbolMissing { symbol: String, detail: String },
    #[error("NVML_CALL_FAILED: {call} 이(가) 실패했다(코드 {code}: {meaning})")]
    CallFailed {
        call: String,
        code: i32,
        meaning: &'static str,
    },
    #[error("NVML_BAD_STRING: {call} 이(가) 돌려준 문자열이 UTF-8 이 아니다")]
    BadString { call: String },
    /// 버퍼가 꽉 차고 NUL 이 없다 — 잘렸을 수 있다는 뜻이다.
    ///
    /// ★ 이걸 `BadString` 과 합치면 "인코딩이 이상하다" 와
    ///   "버퍼 상한을 잘못 잡았다" 를 구분할 수 없다 — 후자는
    ///   이 크레이트의 버그고 전자는 드라이버 문제다(독립 검수 지적).
    #[error("NVML_UNTERMINATED_STRING: {call} 의 버퍼에 NUL 이 없다 — 버퍼 상한이 작아 잘렸을 수 있다")]
    UnterminatedString { call: String },
}

/// NVML 을 열어 전체 스냅샷을 읽고 다시 닫는다.
///
/// # 왜 한 함수가 전부 하는가
///
/// 핸들을 밖으로 내보내면 호출부가 `nvmlShutdown()` 을 안 부르거나 순서를
/// 틀릴 수 있다. 열기·조회·닫기를 한 호출 안에 가두면 그 실수가 불가능하다.
/// 조회가 잦은 경로가 생기면 그때 캐싱을 별도로 설계한다 — 지금 미리
/// 만들지 않는다.
///
/// # 장치가 0개인 것은 오류가 아니다
///
/// NVML 이 정상적으로 열렸는데 장치가 0개면 `Ok` 에 빈 목록을 담아
/// 돌려준다. 그건 확인된 사실이다. 반대로 NVML 을 못 열면 `Err` 다 —
/// 그건 사실이 아니라 **모름**이다.
pub fn observe() -> Result<NvmlSnapshot, NvmlError> {
    let nvml = ffi::Nvml::load()?;
    // 초기화에 성공한 뒤로는 어떤 경로로 나가든 반드시 닫는다.
    nvml.init()?;
    let result = observe_with(&nvml);
    nvml.shutdown_ignoring_error();
    result
}

/// 열거 순서가 아니라 **UUID 순**으로 세운다.
///
/// NVML 의 열거 순서는 **재부팅 사이에 안정적이지 않다**. 그 순서를
/// 그대로 두면 **같은 기계의 두 스냅샷이 달라 보인다** — 스케줄러가 같은
/// 풀을 다른 풀로 읽는다.
///
/// ★ 2라운드 정정 — 전에 `CUDA_DEVICE_ORDER` 를 원인으로 적었는데 그건
///   **CUDA** 의 열거 순서를 바꾸는 변수이지 NVML index 를 바꾸지 않는다.
///   NVML index 가 CUDA index 와 다를 수 있다는 것은 별개의 사실이다.
///
/// ★ 이 정렬을 함수로 떼어낸 이유는 **실물 GPU 없이 재기 위해서**다
///   (`DoD-56` 이 "GPU 1장으로는 증명되지 않는다" 며 남긴 부채). 관측
///   경로 안에 묻어 두면, 관측 결과를 다시 정렬해 자기와 비교하는
///   공허한 테스트밖에 못 쓴다 — 정렬을 통째로 지워도 그 테스트는
///   통과한다.
///
/// ★ `index` 는 **바꾸지 않는다.** 그건 NVML 이 준 사실이고, 정렬은
///   보는 순서일 뿐이다. 둘을 섞으면 관측과 표현이 뒤엉킨다.
pub fn normalize_gpu_order(mut gpus: Vec<NvmlGpu>) -> Vec<NvmlGpu> {
    gpus.sort_by(|left, right| left.uuid.cmp(&right.uuid));
    gpus
}

fn observe_with(nvml: &ffi::Nvml) -> Result<NvmlSnapshot, NvmlError> {
    let driver_version = nvml.driver_version()?;
    let cuda_driver_version = nvml.cuda_driver_version()?;
    let count = nvml.device_count()?;

    let mut gpus = Vec::with_capacity(count as usize);
    for index in 0..count {
        let device: *mut c_void = nvml.device_handle(index)?;
        let (total, free, used) = nvml.memory_info(device)?;
        gpus.push(NvmlGpu {
            uuid: nvml.device_uuid(device)?,
            index,
            name: nvml.device_name(device)?,
            total_vram_bytes: total,
            free_vram_bytes: free,
            used_vram_bytes: used,
            compute_capability: nvml.compute_capability(device)?,
            mig_enabled: nvml.mig_enabled(device)?,
        });
    }

    let gpus = normalize_gpu_order(gpus);

    Ok(NvmlSnapshot {
        driver_version,
        cuda_driver_version,
        gpus,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GPU 가 없는 기계에서도 도는 검사.
    ///
    /// ★ 이 저장소의 개발 기계에는 NVIDIA GPU 가 없다. 그러니 "실제
    ///   GPU 를 읽었다" 는 여기서 증명할 수 없고, 증명하는 척해서도
    ///   안 된다(`CLAUDE.md` §4). 여기서 고정하는 것은 **NVML 이 없을 때
    ///   조용히 빈 목록을 만들지 않는다**는 계약뿐이다.
    #[test]
    fn missing_nvml_is_an_error_not_an_empty_list() {
        match observe() {
            Err(NvmlError::LibraryUnavailable { .. }) => {
                // NVML 없는 기계 — 기대한 실패다.
            }
            Err(other) => {
                // NVML 은 있는데 다른 이유로 실패했다. 그것도 오류로 보고돼야
                // 하며 빈 목록이면 안 된다 — 이 분기도 계약을 만족한다.
                let text = other.to_string();
                assert!(
                    text.starts_with("NVML_"),
                    "NVML 오류가 식별 가능한 접두사를 갖지 않는다: {text}"
                );
            }
            Ok(snapshot) => {
                // NVML 이 있는 기계(예: x600). 읽힌 값이 자기모순이 없어야 한다.
                for gpu in &snapshot.gpus {
                    assert!(
                        !gpu.uuid.is_empty(),
                        "UUID 가 비어 있다 — 식별자로 쓸 수 없다"
                    );
                    assert!(
                        gpu.used_vram_bytes + gpu.free_vram_bytes <= gpu.total_vram_bytes,
                        "used+free 가 total 을 넘는다(uuid={}): {} + {} > {}",
                        gpu.uuid,
                        gpu.used_vram_bytes,
                        gpu.free_vram_bytes,
                        gpu.total_vram_bytes
                    );
                }
                // ★ 여기서는 **이 기계가 실제로 정렬된 목록을 돌려줬는지**
                //   만 본다. 정렬 규칙 자체의 순서 독립성은 아래
                //   `gpu_order_is_the_same_for_every_enumeration_order` 가
                //   합성 GPU 로 잰다 — 관측 결과를 다시 정렬해 자기와
                //   비교하면 정렬을 통째로 지워도 통과한다(공허).
                assert!(
                    snapshot
                        .gpus
                        .windows(2)
                        .all(|pair| pair[0].uuid <= pair[1].uuid),
                    "장치 목록이 UUID 순이 아니다 — 스냅샷이 실행마다 달라진다"
                );
            }
        }
    }

    /// MIG 판정이 세 상태를 실제로 구분하는가.
    ///
    /// `None`(지원 안 함)을 `Some(false)`(꺼짐)와 같게 다루면, 지원하지
    /// 않는 장치를 "MIG 꺼진 정상 장치" 로 단정하게 된다.
    #[test]
    fn mig_states_are_three_not_two() {
        let base = NvmlGpu {
            uuid: "GPU-0".into(),
            index: 0,
            name: "test".into(),
            total_vram_bytes: 1,
            free_vram_bytes: 1,
            used_vram_bytes: 0,
            compute_capability: None,
            mig_enabled: None,
        };
        assert!(!base.is_partitioned(), "지원 안 함을 분할됨으로 봤다");
        assert!(
            !NvmlGpu {
                mig_enabled: Some(false),
                ..base.clone()
            }
            .is_partitioned(),
            "MIG 꺼짐을 분할됨으로 봤다"
        );
        assert!(
            NvmlGpu {
                mig_enabled: Some(true),
                ..base
            }
            .is_partitioned(),
            "MIG 켜짐을 분할됨으로 보지 않았다 — scheduler 의 PARTITIONED 거부가 안 걸린다"
        );
    }

    /// 합성 GPU 4장. `index` 는 열거 순서를 흉내내려고 일부러 UUID 순과
    /// **어긋나게** 준다 — 정렬이 index 를 따라가면 바로 드러난다.
    fn synthetic_gpus() -> Vec<NvmlGpu> {
        let make = |uuid: &str, index: u32, name: &str, total: u64| NvmlGpu {
            uuid: uuid.into(),
            index,
            name: name.into(),
            total_vram_bytes: total,
            free_vram_bytes: total / 2,
            used_vram_bytes: total / 4,
            compute_capability: Some("8.9".into()),
            mig_enabled: Some(false),
        };
        vec![
            make("GPU-dddd", 0, "D", 4_000),
            make("GPU-bbbb", 1, "B", 2_000),
            make("GPU-aaaa", 2, "A", 1_000),
            make("GPU-cccc", 3, "C", 3_000),
        ]
    }

    /// ★ `DoD-56` 이 "GPU 1장으로는 증명되지 않는다" 며 남긴 부채.
    ///
    /// 실제 **4! = 24개 순열 전부**를 넣어 결과가 같은지 본다. 개수나
    /// UUID 목록만 비교하지 않고 **행 전체**를 비교하며, 기대 행은
    /// 검수 대상 함수가 아니라 **입력에서 손으로** 고른다.
    #[test]
    fn gpu_order_is_the_same_for_every_enumeration_order() {
        let base = synthetic_gpus();

        // ★ 기대값을 **입력에서 손으로** 고른다 — `normalize_gpu_order()` 로
        //   만들면 그 함수가 `name`·VRAM·capability·MIG 를 일관되게
        //   훼손해도 기대값이 함께 훼손돼 통과한다(독립 검수 1라운드가
        //   찾은 것 — 이 조각에서 **두 번째** 같은 유형이다. 처음엔
        //   버퍼 크기를 상수로 만들어 같은 일이 있었다).
        //
        //   `synthetic_gpus()` 의 index 는 0=dddd · 1=bbbb · 2=aaaa · 3=cccc
        //   이므로 UUID 오름차순은 [2, 1, 3, 0] 이다.
        let expected: Vec<NvmlGpu> = [2usize, 1, 3, 0]
            .iter()
            .map(|i| base[*i].clone())
            .collect();

        // 손으로 고른 것이 실제로 UUID 오름차순인지 대조한다 — 순서를
        // 잘못 적었으면 이 테스트 전체가 틀린 것을 재게 된다.
        assert!(
            expected.windows(2).all(|p| p[0].uuid < p[1].uuid),
            "손으로 고른 기대 순서가 UUID 오름차순이 아니다"
        );
        assert_eq!(
            expected.iter().map(|g| g.index).collect::<Vec<_>>(),
            vec![2, 1, 3, 0],
            "기대 행이 의도한 원본 행이 아니다"
        );

        // ★ **서로 다른** 24개인지 본다(독립 검수 2라운드 지적). 개수만
        //   세면, 생성기가 정렬된 같은 순열을 24번 돌려줘도 전부 통과한다
        //   — 그러면 정렬을 지우는 뮤테이션조차 안 잡힌다. 생성기는
        //   테스트 코드이므로 낡을 수 있다.
        let mut orders = std::collections::BTreeSet::new();
        for perm in permutations(base) {
            orders.insert(perm.iter().map(|g| g.index).collect::<Vec<_>>());
            assert_eq!(
                normalize_gpu_order(perm.clone()),
                expected,
                "열거 순서 {:?} 에서 다른 결과가 나왔다",
                perm.iter().map(|g| g.index).collect::<Vec<_>>()
            );
        }
        assert_eq!(
            orders.len(),
            24,
            "서로 다른 순열이 24개가 아니다 — {}개뿐이라 순서 독립성을 재지 못한다",
            orders.len()
        );
    }

    /// 정렬이 **실제로 필요한지** 대조한다 — 이게 없으면 "입력이 이미
    /// 정렬돼 있었다" 는 경우와 구분되지 않는다.
    #[test]
    fn the_synthetic_input_is_not_already_sorted() {
        let base = synthetic_gpus();
        assert!(
            !base.windows(2).all(|p| p[0].uuid <= p[1].uuid),
            "합성 입력이 이미 정렬돼 있다 — 이 테스트가 정렬을 재지 못한다"
        );
    }

    /// 24개 순열을 만든다(Heap 알고리즘).
    fn permutations(items: Vec<NvmlGpu>) -> Vec<Vec<NvmlGpu>> {
        fn go(k: usize, items: &mut Vec<NvmlGpu>, out: &mut Vec<Vec<NvmlGpu>>) {
            if k == 1 {
                out.push(items.clone());
                return;
            }
            for i in 0..k {
                go(k - 1, items, out);
                if k % 2 == 0 {
                    items.swap(i, k - 1);
                } else {
                    items.swap(0, k - 1);
                }
            }
        }
        let mut items = items;
        let mut out = Vec::new();
        let k = items.len();
        go(k, &mut items, &mut out);
        out
    }
}
