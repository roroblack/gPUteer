//! 워크로드를 띄우기 **직전에** 요구와 실제 NVML 관측을 대조하는 층.
//!
//! # 이름이 `check_` 인 이유 — 확인이지 강제가 아니다
//!
//! ```text
//! 한다      요구한 GPU 개수가 지금 실재하는가
//!           스케줄러가 고른 UUID 가 실제 장치와 맞는가
//!           그 장치의 여유 VRAM 이 지금 요구를 넘는가
//!           관측을 못 얻으면 "GPU 없음" 이 아니라 "모름" 으로 거부한다
//!
//! 못 한다   위 사실을 **유지**시키는 것.
//!           통과한 뒤 1ms 만에 다른 프로세스가 VRAM 을 먹어도 이 층은
//!           모르고, 막지도 못한다. 배타 할당·예약·quota 는 전부 별개다.
//! ```
//!
//! ★ `CLAUDE.md` §0.4 — **강제할 수 없는 것을 보장으로 선언하지 않는다.**
//!   소비자 GPU 에는 VRAM quota 강제 수단이 없다(MIG 는 데이터센터 전용,
//!   MPS 는 Linux 전용, cgroup 은 시스템 RAM 만 제한한다). 그래서 이
//!   함수의 통과는 **그 순간의 관측이 요구와 어긋나지 않았다** 는 사실일
//!   뿐이고, 반환 타입에 예약 핸들·lease·토큰이 없는 것도 의도다.
//!   `verify`/`ensure`/`reserve`/`acquire` 같은 이름을 쓰지 않는 이유가
//!   이것이다 — 이름이 강제를 암시하면 호출부가 그렇게 믿는다.
//!
//! ★ 그럼에도 이 확인이 값을 하는 이유는, **어긋남이 이미 확정된 경우**를
//!   실행 전에 잡기 때문이다. 고른 GPU 가 아예 없거나 VRAM 이 이미
//!   모자란 상태에서 워크로드를 띄우면 남의 PC 에서 몇 분 뒤 OOM 으로
//!   죽는다. TOCTOU 창을 못 닫는 것과 이미 틀린 것을 못 잡는 것은 다르다.
//!
//! # "모른다" 를 "없다" 로 바꾸지 않는다
//!
//! NVML 을 못 열거나 호출이 실패하면 값을 0 이나 추정치로 채우지 않고
//! [`GpuPreflightRejection::ObservationUnavailable`] 로 거부한다
//! (`CLAUDE.md` §1). 호출부가 둘을 구분해야 하는 자리를 위해
//! [`GpuPreflightRejection::is_unknown`] 을 따로 둔다 — "이 기계는 요구를
//! 못 맞춘다" 와 "이 기계가 요구를 맞추는지 모른다" 는 다른 사실이고,
//! 후자를 전자로 보고하면 노드가 영구히 부적격으로 낙인찍힌다.
//!
//! # 왜 관측을 인자로 받는가
//!
//! 판정 로직은 [`check_gpu_requirements_against`] 라는 **순수 함수**이고
//! NVML 을 만지지 않는다. 이 저장소의 개발 기계에는 NVIDIA GPU 가 없어서
//! (`CLAUDE.md` 환경 주의사항) 관측을 주입하지 못하면 판정 로직을 한 줄도
//! 못 잰다. NVML 자체 실패 경로까지 재려면 관측 **함수**도 주입할 수
//! 있어야 해서 [`check_gpu_requirements_with`] 가 따로 있다.

use std::collections::BTreeMap;

use crate::{NvmlError, NvmlSnapshot};

/// 실행 직전에 대조할 요구. 스케줄러(`DoD-55`)의 결과를 옮겨 담는 자리다.
///
/// ★ `selected_gpu_uuids` 를 `Option` 이나 "비면 제약 없음" 으로 만들지
///   않았다. 비었을 때 아무것도 확인하지 않고 통과시키면, 확인을 부른
///   호출부가 확인된 줄 알고 넘어간다 — `CLAUDE.md` §3 "조용한 스킵을
///   만들지 않는다". 개수와 목록이 어긋나면 통과가 아니라 거부다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpuRequirements {
    /// 이 워크로드가 필요로 하는 GPU 개수.
    pub required_gpu_count: u32,
    /// GPU **한 장당** 지금 있어야 하는 여유 VRAM(바이트).
    pub minimum_free_vram_bytes_per_gpu: u64,
    /// 스케줄러가 고른 GPU 의 NVML UUID 목록.
    ///
    /// ★ `NvmlGpu::index` 가 아니라 UUID 다. index 는 재부팅을 넘어
    ///   안정적이지 않아서(`lib.rs` 참조) 식별자로 쓰면 다른 장치를
    ///   같은 장치로 본다.
    pub selected_gpu_uuids: Vec<String>,
}

/// 확인에 통과한 GPU 한 장에 대해 **그 순간** 관측된 값.
///
/// ★ 이 숫자들은 확인 시점의 사진이지 앞으로의 약속이 아니다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedGpu {
    pub uuid: String,
    /// 진단용. 식별자로 쓰지 마라.
    pub index: u32,
    pub free_vram_bytes: u64,
    pub total_vram_bytes: u64,
}

/// 확인이 어긋남을 찾지 못했다는 사실.
///
/// ★ **예약·lease·grant 가 아니다.** 그래서 이 타입에는 뒤에 반납할
///   핸들이 없다. 실행 중 VRAM 이 사라지는 것을 이 값이 막지 않는다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpuPreflightOk {
    /// 확인에 쓴 관측의 드라이버 버전. 진단·로그용이다.
    pub driver_version: String,
    /// 확인한 GPU 들. **UUID 오름차순**이라 같은 입력에 같은 값이 나온다.
    pub checked: Vec<CheckedGpu>,
}

/// 확인이 통과하지 못한 이유.
///
/// ★ 한 가지만 빼고 전부 "관측과 요구가 어긋났다" 는 **확인된 사실**이다.
///   [`Self::ObservationUnavailable`] 만 "확인을 못 했다" 이며,
///   [`Self::is_unknown`] 이 그 구분을 코드로 노출한다.
#[derive(Debug, thiserror::Error)]
pub enum GpuPreflightRejection {
    /// NVML 이 값을 주지 못했다. **GPU 가 없다는 뜻이 아니다.**
    #[error("PREFLIGHT_OBSERVATION_UNAVAILABLE: GPU 관측을 얻지 못해 확인할 수 없다(없다는 뜻이 아니다) — {source}")]
    ObservationUnavailable {
        #[source]
        source: NvmlError,
    },

    /// 0장을 요구했다. 확인할 것이 없으면 이 함수를 부르지 마라 —
    /// "0장 요구는 언제나 통과" 로 만들면 요구를 못 채운 경로가
    /// 통과로 둔갑한다(`scheduler` 의 `MinimumGpuCountMustBePositive` 와 같은 이유).
    #[error("PREFLIGHT_REQUIRED_COUNT_IS_ZERO: 요구 GPU 개수가 0이다 — 확인할 대상이 없다")]
    RequiredCountIsZero,

    /// 요구 개수와 스케줄러가 고른 목록의 길이가 다르다. 둘 중 하나가
    /// 틀렸으므로 **어느 쪽을 믿을지 이 층이 고르지 않는다.**
    #[error(
        "PREFLIGHT_SELECTION_COUNT_MISMATCH: 고른 GPU 는 {selected}장인데 요구는 {required}장이다"
    )]
    SelectionCountMismatch { selected: u32, required: u32 },

    #[error("PREFLIGHT_EMPTY_SELECTED_UUID: 고른 UUID 중 빈 값이 있다 — 어느 장치인지 알 수 없다")]
    EmptySelectedUuid,

    /// 앞뒤 공백이 붙은 UUID. 잘라서 받아주면 같은 장치가 두 이름을 갖는다.
    #[error("PREFLIGHT_NON_CANONICAL_SELECTED_UUID: 고른 UUID 에 앞뒤 공백이 있다({uuid:?})")]
    NonCanonicalSelectedUuid { uuid: String },

    /// 같은 UUID 를 두 번 골랐다 — 한 장을 두 장으로 센 것이다.
    #[error("PREFLIGHT_DUPLICATE_SELECTED_UUID: 같은 GPU 를 두 번 골랐다({uuid}) — 한 장이 두 장으로 세어졌다")]
    DuplicateSelectedUuid { uuid: String },

    /// NVML 이 같은 UUID 를 두 번 보고했다. 정상 드라이버에서는 일어나지
    /// 않는다 — 조용히 하나로 합치면 없는 장치를 있다고 세게 된다.
    #[error("PREFLIGHT_DUPLICATE_OBSERVED_UUID: NVML 이 같은 UUID 를 두 번 보고했다({uuid}) — 관측을 믿을 수 없다")]
    DuplicateObservedUuid { uuid: String },

    /// 기계에 있는 장치 수가 요구에 못 미친다.
    #[error("PREFLIGHT_INSUFFICIENT_GPU_COUNT: {required}장을 요구했는데 NVML 이 본 장치는 {present}장이다")]
    InsufficientGpuCount { present: u32, required: u32 },

    /// 스케줄러가 고른 UUID 가 지금 이 기계의 장치와 맞지 않는다.
    #[error("PREFLIGHT_SELECTED_GPU_ABSENT: 고른 GPU {uuid} 가 지금 이 기계에 없다 — 스케줄러의 식별자가 실제 장치와 맞지 않는다")]
    SelectedGpuAbsent { uuid: String },

    /// MIG 가 켜진 장치. 물리 GPU 하나가 여러 인스턴스로 쪼개져 있어
    /// 전체 VRAM 을 이 워크로드가 쓸 수 있다고 가정할 수 없다.
    /// `gpu_scope_candidate()` 가 `Partitioned` 를 항상 거부하는 것과 같은 이유다.
    #[error("PREFLIGHT_SELECTED_GPU_PARTITIONED: 고른 GPU {uuid} 는 MIG 가 켜져 있다 — 여유 VRAM 을 이 워크로드 몫으로 볼 수 없다")]
    SelectedGpuPartitioned { uuid: String },

    /// `used + free > total`. 드라이버가 자기모순인 값을 줬다는 뜻이므로
    /// 이 장치에 대해서는 어떤 판정도 내리지 않는다.
    #[error("PREFLIGHT_INCOHERENT_VRAM_OBSERVATION: {uuid} 의 used({used})+free({free}) 가 total({total}) 을 넘는다 — 이 관측으로는 판정할 수 없다")]
    IncoherentVramObservation {
        uuid: String,
        used: u64,
        free: u64,
        total: u64,
    },

    /// 지금 여유 VRAM 이 요구에 못 미친다.
    #[error("PREFLIGHT_INSUFFICIENT_FREE_VRAM: {uuid} 의 여유 VRAM 이 {free_bytes}바이트로 요구 {required_bytes}바이트에 못 미친다")]
    InsufficientFreeVram {
        uuid: String,
        free_bytes: u64,
        required_bytes: u64,
    },
}

impl GpuPreflightRejection {
    /// 이 거부가 "확인해 보니 어긋났다" 가 아니라 **"확인 자체를 못 했다"** 인가.
    ///
    /// ★ 둘을 같게 다루면 NVML 이 잠깐 안 열린 노드가 "GPU 요구를 못
    ///   맞추는 노드" 로 기록된다. `CLAUDE.md` §1 — 모르는 것은 모른다고
    ///   남긴다.
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::ObservationUnavailable { .. })
    }
}

/// **순수 판정 커널.** 고정된 관측과 요구를 대조한다.
///
/// I/O·현재 시각·난수·전역 상태를 만지지 않는다. 같은 입력에는 항상 같은
/// 결과가 나오며, 여러 항목이 동시에 어긋나면 아래 순서로 **처음 걸린
/// 하나**를 돌려준다.
///
/// ```text
/// 1  요구 자체가 성립하는가   개수 0 · 개수와 목록 길이 불일치 · 빈/공백 UUID · 중복 선택
/// 2  관측이 성립하는가        NVML 이 같은 UUID 를 두 번 보고
/// 3  개수                     기계의 장치 수 < 요구 개수
/// 4  식별자                   고른 UUID 가 실제 장치에 없다
/// 5  분할                     고른 장치에 MIG 가 켜져 있다
/// 6  VRAM                     관측 자기모순 · 여유 VRAM 부족
/// ```
///
/// ★ 식별자를 VRAM 보다 먼저 보는 것은 임의가 아니다. 어느 장치인지
///   모르면 그 장치의 VRAM 을 물을 수 없다. 순서를 뒤집으면 "UUID 가
///   틀렸다" 를 "VRAM 이 모자라다" 로 보고하게 되고, 그건
///   `CLAUDE.md` §3 이 금지하는 **사실을 잘못 전하는 오류 메시지**다.
pub fn check_gpu_requirements_against(
    requirements: &GpuRequirements,
    snapshot: &NvmlSnapshot,
) -> Result<GpuPreflightOk, GpuPreflightRejection> {
    // 1. 요구 자체가 성립하는가.
    if requirements.required_gpu_count == 0 {
        return Err(GpuPreflightRejection::RequiredCountIsZero);
    }
    let selected = &requirements.selected_gpu_uuids;
    let selected_len = u32::try_from(selected.len()).unwrap_or(u32::MAX);
    if selected_len != requirements.required_gpu_count {
        return Err(GpuPreflightRejection::SelectionCountMismatch {
            selected: selected_len,
            required: requirements.required_gpu_count,
        });
    }
    if selected.iter().any(|uuid| uuid.trim().is_empty()) {
        return Err(GpuPreflightRejection::EmptySelectedUuid);
    }
    if let Some(uuid) = selected
        .iter()
        .filter(|uuid| uuid.trim() != uuid.as_str())
        .min()
    {
        return Err(GpuPreflightRejection::NonCanonicalSelectedUuid { uuid: uuid.clone() });
    }
    if let Some(uuid) = first_duplicate(selected.iter().map(String::as_str)) {
        return Err(GpuPreflightRejection::DuplicateSelectedUuid { uuid });
    }

    // 2. 관측이 성립하는가.
    if let Some(uuid) = first_duplicate(snapshot.gpus.iter().map(|gpu| gpu.uuid.as_str())) {
        return Err(GpuPreflightRejection::DuplicateObservedUuid { uuid });
    }
    let observed: BTreeMap<&str, &crate::NvmlGpu> = snapshot
        .gpus
        .iter()
        .map(|gpu| (gpu.uuid.as_str(), gpu))
        .collect();

    // 3. 개수. 고른 UUID 가 다 맞더라도, 기계의 장치 수가 요구에 못
    //    미치는 상황은 별개의 사실이라 별개의 이유로 보고한다.
    let present = u32::try_from(snapshot.gpus.len()).unwrap_or(u32::MAX);
    if present < requirements.required_gpu_count {
        return Err(GpuPreflightRejection::InsufficientGpuCount {
            present,
            required: requirements.required_gpu_count,
        });
    }

    // 보고 순서를 입력 순서에서 떼어낸다 — 같은 집합이면 같은 결과가
    // 나와야 한다(관측은 이미 UUID 순이지만 요구 목록은 아닐 수 있다).
    let mut canonical: Vec<&str> = selected.iter().map(String::as_str).collect();
    canonical.sort_unstable();

    // 4. 식별자.
    let mut matched = Vec::with_capacity(canonical.len());
    for uuid in &canonical {
        match observed.get(uuid) {
            Some(gpu) => matched.push(*gpu),
            None => {
                return Err(GpuPreflightRejection::SelectedGpuAbsent {
                    uuid: (*uuid).to_owned(),
                });
            }
        }
    }

    // 5. 분할.
    for gpu in &matched {
        if gpu.is_partitioned() {
            return Err(GpuPreflightRejection::SelectedGpuPartitioned {
                uuid: gpu.uuid.clone(),
            });
        }
    }

    // 6. VRAM.
    let mut checked = Vec::with_capacity(matched.len());
    for gpu in matched {
        // ★ 자기모순인 관측 위에 판정을 쌓지 않는다. 여기서 통과시키면
        //   "free 가 total 보다 크다" 같은 값으로 충분하다고 답하게 된다.
        if gpu.used_vram_bytes.saturating_add(gpu.free_vram_bytes) > gpu.total_vram_bytes {
            return Err(GpuPreflightRejection::IncoherentVramObservation {
                uuid: gpu.uuid.clone(),
                used: gpu.used_vram_bytes,
                free: gpu.free_vram_bytes,
                total: gpu.total_vram_bytes,
            });
        }
        // ★ **free 로 잰다, total 이 아니라.** total 로 재면 이미 다른
        //   프로세스가 다 쓰고 있는 GPU 도 통과한다 — 그게 남의 PC 에서
        //   OOM 으로 죽는 정확한 경로다.
        if gpu.free_vram_bytes < requirements.minimum_free_vram_bytes_per_gpu {
            return Err(GpuPreflightRejection::InsufficientFreeVram {
                uuid: gpu.uuid.clone(),
                free_bytes: gpu.free_vram_bytes,
                required_bytes: requirements.minimum_free_vram_bytes_per_gpu,
            });
        }
        checked.push(CheckedGpu {
            uuid: gpu.uuid.clone(),
            index: gpu.index,
            free_vram_bytes: gpu.free_vram_bytes,
            total_vram_bytes: gpu.total_vram_bytes,
        });
    }

    Ok(GpuPreflightOk {
        driver_version: snapshot.driver_version.clone(),
        checked,
    })
}

/// 관측 **함수**를 주입해 확인한다.
///
/// ★ 이것이 따로 있는 이유는 **NVML 자체가 실패하는 경로**를 GPU 없이
///   재기 위해서다. [`check_gpu_requirements_against`] 는 스냅샷을 이미
///   받았으므로 "관측을 못 얻었다" 를 표현할 수 없다.
pub fn check_gpu_requirements_with<F>(
    requirements: &GpuRequirements,
    observe_now: F,
) -> Result<GpuPreflightOk, GpuPreflightRejection>
where
    F: FnOnce() -> Result<NvmlSnapshot, NvmlError>,
{
    let snapshot =
        observe_now().map_err(|source| GpuPreflightRejection::ObservationUnavailable { source })?;
    check_gpu_requirements_against(requirements, &snapshot)
}

/// 지금 이 기계의 NVML 을 실제로 읽어 확인한다 — 워크로드 spawn 직전에 부른다.
///
/// ★ 다시 강조한다: **통과는 예약이 아니다.** 이 함수가 `Ok` 를 준
///   직후에도 다른 프로세스가 VRAM 을 가져갈 수 있고, 이 층은 그것을
///   막지 못한다. 배타 할당이 필요하면 그건 별도 계층이 해야 한다
///   (`CLAUDE.md` §0.4).
pub fn check_gpu_requirements_now(
    requirements: &GpuRequirements,
) -> Result<GpuPreflightOk, GpuPreflightRejection> {
    check_gpu_requirements_with(requirements, crate::observe)
}

/// 정렬 없이 첫 중복을 찾는다. 결과의 결정성을 위해 **가장 작은** 중복 값을 돌려준다.
fn first_duplicate<'a, I: Iterator<Item = &'a str>>(items: I) -> Option<String> {
    let mut counts = BTreeMap::<&str, usize>::new();
    for item in items {
        *counts.entry(item).or_default() += 1;
    }
    counts
        .into_iter()
        .find(|(_, count)| *count > 1)
        .map(|(item, _)| item.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NvmlGpu;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn gpu(uuid: &str, index: u32, total: u64, free: u64) -> NvmlGpu {
        NvmlGpu {
            uuid: uuid.into(),
            index,
            name: "synthetic".into(),
            total_vram_bytes: total,
            free_vram_bytes: free,
            used_vram_bytes: total - free,
            compute_capability: Some("8.9".into()),
            mig_enabled: Some(false),
        }
    }

    fn snapshot(gpus: Vec<NvmlGpu>) -> NvmlSnapshot {
        NvmlSnapshot {
            driver_version: "580.00".into(),
            cuda_driver_version: 13_020,
            gpus,
        }
    }

    /// 24GiB 두 장, 각각 20GiB 여유.
    fn two_healthy_gpus() -> NvmlSnapshot {
        snapshot(vec![
            gpu("GPU-aaaa", 1, 24 * GIB, 20 * GIB),
            gpu("GPU-bbbb", 0, 24 * GIB, 20 * GIB),
        ])
    }

    fn require(count: u32, min_free: u64, uuids: &[&str]) -> GpuRequirements {
        GpuRequirements {
            required_gpu_count: count,
            minimum_free_vram_bytes_per_gpu: min_free,
            selected_gpu_uuids: uuids.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    // ---------------------------------------------------------------
    // 정상 경로 — 이것만으로는 완료가 아니다(`RULE.md` §6). 아래 negative
    // 들이 본론이다.
    // ---------------------------------------------------------------

    #[test]
    fn matching_requirements_pass_and_report_the_observed_numbers() {
        let ok = check_gpu_requirements_against(
            // 요구 목록은 일부러 UUID 역순으로 준다 — 결과가 입력 순서를
            // 따라가면 여기서 드러난다.
            &require(2, 16 * GIB, &["GPU-bbbb", "GPU-aaaa"]),
            &two_healthy_gpus(),
        )
        .expect("두 장 다 요구를 넘는데 거부됐다");

        assert_eq!(ok.driver_version, "580.00");
        assert_eq!(
            ok.checked,
            vec![
                CheckedGpu {
                    uuid: "GPU-aaaa".into(),
                    index: 1,
                    free_vram_bytes: 20 * GIB,
                    total_vram_bytes: 24 * GIB,
                },
                CheckedGpu {
                    uuid: "GPU-bbbb".into(),
                    index: 0,
                    free_vram_bytes: 20 * GIB,
                    total_vram_bytes: 24 * GIB,
                },
            ],
            "확인 결과가 UUID 오름차순이 아니거나 관측값을 그대로 옮기지 않았다"
        );
    }

    /// 필요한 만큼만 고른 경우(장치는 2장인데 1장만 요구)도 통과해야 한다.
    #[test]
    fn a_subset_of_the_present_gpus_is_enough() {
        let ok = check_gpu_requirements_against(
            &require(1, 16 * GIB, &["GPU-bbbb"]),
            &two_healthy_gpus(),
        )
        .expect("두 장 중 한 장만 요구했는데 거부됐다");
        assert_eq!(
            ok.checked
                .iter()
                .map(|g| g.uuid.as_str())
                .collect::<Vec<_>>(),
            vec!["GPU-bbbb"],
            "고르지 않은 GPU 까지 확인 결과에 들어갔다"
        );
    }

    /// 경계값. `free == 요구` 는 통과다 — `<` 를 `<=` 로 바꾸는 뮤테이션이
    /// 여기서 걸린다.
    #[test]
    fn free_vram_exactly_equal_to_the_requirement_passes() {
        let result = check_gpu_requirements_against(
            &require(1, 20 * GIB, &["GPU-aaaa"]),
            &two_healthy_gpus(),
        );
        assert!(
            result.is_ok(),
            "여유 VRAM 이 요구와 정확히 같은데 거부했다: {:?}",
            result.err()
        );
    }

    // ---------------------------------------------------------------
    // negative — 각각 **실패했다** 가 아니라 **왜 실패했는지**를 단언한다.
    //
    // ★ 이 저장소에서 `!ok` 만 보는 테스트가 다섯 번 사고를 냈다. 이유를
    //   안 재면 "VRAM 부족" 을 "UUID 없음" 으로 잘못 보고하는 회귀가
    //   테스트를 그대로 통과한다.
    // ---------------------------------------------------------------

    /// (1) NVML 자체 실패 — **"GPU 가 없다" 가 아니라 "모른다" 다.**
    #[test]
    fn nvml_failure_is_reported_as_unknown_not_as_a_shortage() {
        let rejection = check_gpu_requirements_with(&require(1, GIB, &["GPU-aaaa"]), || {
            Err(NvmlError::LibraryUnavailable {
                detail: "nvml.dll: 없음".into(),
            })
        })
        .expect_err("NVML 을 못 열었는데 통과했다");

        match &rejection {
            GpuPreflightRejection::ObservationUnavailable { source } => {
                assert!(
                    matches!(source, NvmlError::LibraryUnavailable { .. }),
                    "원래 NVML 오류가 보존되지 않았다: {source}"
                );
            }
            other => panic!("NVML 실패를 다른 이유로 보고했다: {other}"),
        }
        assert!(
            rejection.is_unknown(),
            "NVML 실패를 '확인된 어긋남' 으로 분류했다 — 노드가 부적격으로 낙인찍힌다"
        );
        let text = rejection.to_string();
        assert!(
            text.contains("PREFLIGHT_OBSERVATION_UNAVAILABLE"),
            "이유를 식별할 접두사가 없다: {text}"
        );
        assert!(
            text.contains("NVML_UNAVAILABLE"),
            "원인 NVML 오류가 메시지에 남지 않았다: {text}"
        );
    }

    /// NVML 호출 실패(라이브러리는 열렸지만 호출이 실패)도 마찬가지로 "모름" 이다.
    #[test]
    fn a_failed_nvml_call_is_also_unknown() {
        let rejection = check_gpu_requirements_with(&require(1, GIB, &["GPU-aaaa"]), || {
            Err(NvmlError::CallFailed {
                call: "nvmlDeviceGetMemoryInfo_v2".into(),
                code: 15,
                meaning: "GPU_IS_LOST",
            })
        })
        .expect_err("NVML 호출이 실패했는데 통과했다");
        assert!(
            rejection.is_unknown(),
            "NVML 호출 실패를 '확인된 어긋남' 으로 분류했다"
        );
        assert!(
            rejection.to_string().contains("GPU_IS_LOST"),
            "원인이 메시지에 남지 않았다: {rejection}"
        );
    }

    /// **어긋남은 `is_unknown()` 이 아니다.** 위 두 테스트만 있으면
    /// `is_unknown()` 을 `true` 상수로 만드는 뮤테이션이 안 걸린다.
    #[test]
    fn a_confirmed_mismatch_is_not_unknown() {
        let rejection = check_gpu_requirements_against(
            &require(2, GIB, &["GPU-aaaa", "GPU-bbbb"]),
            &snapshot(vec![gpu("GPU-aaaa", 0, 24 * GIB, 20 * GIB)]),
        )
        .expect_err("한 장뿐인데 두 장 요구가 통과했다");
        assert!(
            !rejection.is_unknown(),
            "확인해서 알아낸 어긋남을 '모름' 으로 분류했다 — 진짜 모름과 구분이 사라진다"
        );
    }

    /// (2) GPU 개수 부족.
    #[test]
    fn fewer_gpus_than_required_is_rejected_as_a_count_shortage() {
        let rejection = check_gpu_requirements_against(
            &require(2, GIB, &["GPU-aaaa", "GPU-bbbb"]),
            &snapshot(vec![gpu("GPU-aaaa", 0, 24 * GIB, 20 * GIB)]),
        )
        .expect_err("장치가 한 장뿐인데 두 장 요구가 통과했다");

        match &rejection {
            GpuPreflightRejection::InsufficientGpuCount { present, required } => {
                assert_eq!(*present, 1, "본 장치 수를 잘못 셌다");
                assert_eq!(*required, 2, "요구 장치 수를 잘못 옮겼다");
            }
            other => panic!("개수 부족을 다른 이유로 보고했다: {other}"),
        }
    }

    /// (3) VRAM 부족 — 어느 GPU 가 · 얼마인데 · 얼마를 요구했는지까지 단언한다.
    #[test]
    fn insufficient_free_vram_is_rejected_with_the_actual_numbers() {
        let rejection = check_gpu_requirements_against(
            &require(1, 16 * GIB, &["GPU-aaaa"]),
            &snapshot(vec![
                gpu("GPU-aaaa", 0, 24 * GIB, 4 * GIB),
                gpu("GPU-bbbb", 1, 24 * GIB, 24 * GIB),
            ]),
        )
        .expect_err("고른 GPU 의 여유가 4GiB 뿐인데 16GiB 요구가 통과했다");

        match &rejection {
            GpuPreflightRejection::InsufficientFreeVram {
                uuid,
                free_bytes,
                required_bytes,
            } => {
                assert_eq!(uuid, "GPU-aaaa", "다른 GPU 를 지목했다");
                assert_eq!(*free_bytes, 4 * GIB, "관측된 여유 VRAM 을 잘못 옮겼다");
                assert_eq!(*required_bytes, 16 * GIB, "요구 VRAM 을 잘못 옮겼다");
            }
            other => panic!("VRAM 부족을 다른 이유로 보고했다: {other}"),
        }
    }

    /// ★ **total 이 아니라 free 로 잰다.** total 로 재는 뮤테이션은 위
    ///   테스트로도 걸리지만, 이 테스트가 그 의도를 명시한다 — 여유가
    ///   0인데 total 은 요구를 훌쩍 넘는 GPU 다.
    #[test]
    fn a_full_gpu_is_rejected_even_though_its_total_vram_is_large() {
        let rejection = check_gpu_requirements_against(
            &require(1, 8 * GIB, &["GPU-aaaa"]),
            &snapshot(vec![gpu("GPU-aaaa", 0, 80 * GIB, 0)]),
        )
        .expect_err("80GiB 짜리지만 여유가 0인 GPU 가 통과했다 — total 로 쟀다는 뜻이다");

        match &rejection {
            GpuPreflightRejection::InsufficientFreeVram { free_bytes, .. } => {
                assert_eq!(*free_bytes, 0, "여유 VRAM 을 잘못 옮겼다");
            }
            other => panic!("total 로 재고 있다 — 보고된 이유: {other}"),
        }
    }

    /// (4) 식별자 불일치 — 개수는 충분한데 고른 UUID 가 실제 장치에 없다.
    #[test]
    fn a_selected_uuid_that_is_not_present_is_rejected_as_an_identifier_mismatch() {
        let rejection =
            check_gpu_requirements_against(&require(1, GIB, &["GPU-cccc"]), &two_healthy_gpus())
                .expect_err("이 기계에 없는 UUID 를 골랐는데 통과했다");

        match &rejection {
            GpuPreflightRejection::SelectedGpuAbsent { uuid } => {
                assert_eq!(uuid, "GPU-cccc", "없는 UUID 를 잘못 지목했다");
            }
            other => panic!("식별자 불일치를 다른 이유로 보고했다: {other}"),
        }
    }

    /// ★ 식별자 불일치가 VRAM 부족보다 **먼저** 나와야 한다. 순서가
    ///   뒤집히면 "UUID 가 틀렸다" 를 "VRAM 이 모자라다" 로 보고한다.
    #[test]
    fn a_missing_identifier_is_reported_before_a_vram_shortage() {
        let rejection = check_gpu_requirements_against(
            // GPU-aaaa 는 여유가 모자라고, GPU-cccc 는 아예 없다.
            &require(2, 16 * GIB, &["GPU-aaaa", "GPU-cccc"]),
            &snapshot(vec![
                gpu("GPU-aaaa", 0, 24 * GIB, GIB),
                gpu("GPU-bbbb", 1, 24 * GIB, 20 * GIB),
            ]),
        )
        .expect_err("둘 다 어긋났는데 통과했다");

        assert!(
            matches!(
                &rejection,
                GpuPreflightRejection::SelectedGpuAbsent { uuid } if uuid == "GPU-cccc"
            ),
            "어느 장치인지 모르는 상태에서 VRAM 을 먼저 보고했다: {rejection}"
        );
    }

    /// 같은 GPU 를 두 번 골라 개수를 채우는 것 — 한 장이 두 장으로 세어진다.
    #[test]
    fn selecting_the_same_gpu_twice_does_not_satisfy_a_two_gpu_requirement() {
        let rejection = check_gpu_requirements_against(
            &require(2, GIB, &["GPU-aaaa", "GPU-aaaa"]),
            &two_healthy_gpus(),
        )
        .expect_err("같은 GPU 를 두 번 골랐는데 두 장 요구가 통과했다");

        match &rejection {
            GpuPreflightRejection::DuplicateSelectedUuid { uuid } => {
                assert_eq!(uuid, "GPU-aaaa", "중복된 UUID 를 잘못 지목했다");
            }
            other => panic!("선택 중복을 다른 이유로 보고했다: {other}"),
        }
    }

    /// 요구 개수와 고른 목록의 길이가 어긋나면 어느 쪽도 믿지 않는다.
    #[test]
    fn a_selection_shorter_than_the_required_count_is_rejected() {
        let rejection =
            check_gpu_requirements_against(&require(2, GIB, &["GPU-aaaa"]), &two_healthy_gpus())
                .expect_err("두 장 요구에 한 장만 골랐는데 통과했다");

        match &rejection {
            GpuPreflightRejection::SelectionCountMismatch { selected, required } => {
                assert_eq!(*selected, 1, "고른 개수를 잘못 셌다");
                assert_eq!(*required, 2, "요구 개수를 잘못 옮겼다");
            }
            other => panic!("개수 불일치를 다른 이유로 보고했다: {other}"),
        }
    }

    /// 반대 방향(고른 것이 더 많은 경우)도 같다 — 확인되지 않은 GPU 가
    /// 실행에 딸려 들어간다.
    #[test]
    fn a_selection_longer_than_the_required_count_is_rejected() {
        let rejection = check_gpu_requirements_against(
            &require(1, GIB, &["GPU-aaaa", "GPU-bbbb"]),
            &two_healthy_gpus(),
        )
        .expect_err("한 장 요구에 두 장을 골랐는데 통과했다");
        assert!(
            matches!(
                &rejection,
                GpuPreflightRejection::SelectionCountMismatch {
                    selected: 2,
                    required: 1
                }
            ),
            "개수 불일치를 다른 이유로 보고했다: {rejection}"
        );
    }

    /// 0장 요구는 "확인할 것 없음 = 통과" 가 아니다.
    #[test]
    fn a_zero_gpu_requirement_is_rejected_rather_than_trivially_passing() {
        let rejection = check_gpu_requirements_against(&require(0, GIB, &[]), &two_healthy_gpus())
            .expect_err("0장 요구가 조용히 통과했다 — 확인 안 한 것이 확인된 것으로 둔갑한다");
        assert!(
            matches!(&rejection, GpuPreflightRejection::RequiredCountIsZero),
            "0장 요구를 다른 이유로 보고했다: {rejection}"
        );
    }

    #[test]
    fn an_empty_selected_uuid_is_rejected() {
        let rejection =
            check_gpu_requirements_against(&require(1, GIB, &["   "]), &two_healthy_gpus())
                .expect_err("빈 UUID 가 통과했다");
        assert!(
            matches!(&rejection, GpuPreflightRejection::EmptySelectedUuid),
            "빈 UUID 를 다른 이유로 보고했다: {rejection}"
        );
    }

    /// 앞뒤 공백을 조용히 잘라 받아주면 같은 장치가 두 이름을 갖는다.
    #[test]
    fn a_selected_uuid_with_surrounding_whitespace_is_rejected_not_trimmed() {
        let rejection =
            check_gpu_requirements_against(&require(1, GIB, &[" GPU-aaaa"]), &two_healthy_gpus())
                .expect_err("공백이 붙은 UUID 가 조용히 잘려 통과했다");

        match &rejection {
            GpuPreflightRejection::NonCanonicalSelectedUuid { uuid } => {
                assert_eq!(uuid, " GPU-aaaa", "문제의 UUID 를 잘못 지목했다");
            }
            other => panic!("비정규 UUID 를 다른 이유로 보고했다: {other}"),
        }
    }

    /// MIG 가 켜진 장치는 여유 VRAM 이 넉넉해 보여도 거부한다.
    /// `gpu_scope_candidate()` 가 `Partitioned` 를 항상 거부하는 것과 같은 이유다.
    #[test]
    fn a_mig_enabled_gpu_is_rejected_even_with_plenty_of_free_vram() {
        let mut partitioned = gpu("GPU-aaaa", 0, 80 * GIB, 80 * GIB);
        partitioned.mig_enabled = Some(true);
        let rejection = check_gpu_requirements_against(
            &require(1, GIB, &["GPU-aaaa"]),
            &snapshot(vec![partitioned]),
        )
        .expect_err("MIG 가 켜진 장치가 통과했다");

        match &rejection {
            GpuPreflightRejection::SelectedGpuPartitioned { uuid } => {
                assert_eq!(uuid, "GPU-aaaa", "다른 GPU 를 지목했다");
            }
            other => panic!("MIG 를 다른 이유로 보고했다: {other}"),
        }
    }

    /// `None`(MIG 미지원)과 `Some(false)`(꺼짐)는 분할이 아니다 —
    /// 여기서 거부하면 소비자 GPU 가 전부 막힌다.
    #[test]
    fn mig_unsupported_and_mig_off_are_not_partitioned() {
        for state in [None, Some(false)] {
            let mut candidate = gpu("GPU-aaaa", 0, 24 * GIB, 20 * GIB);
            candidate.mig_enabled = state;
            let result = check_gpu_requirements_against(
                &require(1, GIB, &["GPU-aaaa"]),
                &snapshot(vec![candidate]),
            );
            assert!(
                result.is_ok(),
                "mig_enabled={state:?} 를 분할됨으로 봤다: {:?}",
                result.err()
            );
        }
    }

    /// 드라이버가 자기모순인 값을 주면 판정하지 않는다.
    #[test]
    fn an_incoherent_vram_observation_is_rejected_instead_of_being_believed() {
        let incoherent = NvmlGpu {
            used_vram_bytes: 20 * GIB,
            free_vram_bytes: 20 * GIB,
            total_vram_bytes: 24 * GIB,
            ..gpu("GPU-aaaa", 0, 24 * GIB, 20 * GIB)
        };
        let rejection = check_gpu_requirements_against(
            &require(1, GIB, &["GPU-aaaa"]),
            &snapshot(vec![incoherent]),
        )
        .expect_err("used+free 가 total 을 넘는 관측을 그대로 믿었다");

        match &rejection {
            GpuPreflightRejection::IncoherentVramObservation {
                uuid,
                used,
                free,
                total,
            } => {
                assert_eq!(uuid, "GPU-aaaa");
                assert_eq!((*used, *free, *total), (20 * GIB, 20 * GIB, 24 * GIB));
            }
            other => panic!("자기모순 관측을 다른 이유로 보고했다: {other}"),
        }
    }

    /// NVML 이 같은 UUID 를 두 번 보고하면 조용히 하나로 합치지 않는다.
    #[test]
    fn a_duplicate_uuid_in_the_observation_is_rejected() {
        let rejection = check_gpu_requirements_against(
            &require(2, GIB, &["GPU-aaaa", "GPU-bbbb"]),
            &snapshot(vec![
                gpu("GPU-aaaa", 0, 24 * GIB, 20 * GIB),
                gpu("GPU-aaaa", 1, 24 * GIB, 20 * GIB),
            ]),
        )
        .expect_err("NVML 이 중복 UUID 를 보고했는데 통과했다");

        match &rejection {
            GpuPreflightRejection::DuplicateObservedUuid { uuid } => {
                assert_eq!(uuid, "GPU-aaaa");
            }
            other => panic!("관측 중복을 다른 이유로 보고했다: {other}"),
        }
    }

    /// 판정이 요구 목록의 **순서**에 의존하지 않는다.
    #[test]
    fn the_verdict_does_not_depend_on_the_order_of_the_selection() {
        let observed = two_healthy_gpus();
        let forward = check_gpu_requirements_against(
            &require(2, 16 * GIB, &["GPU-aaaa", "GPU-bbbb"]),
            &observed,
        )
        .expect("정방향 순서가 거부됐다");
        let reverse = check_gpu_requirements_against(
            &require(2, 16 * GIB, &["GPU-bbbb", "GPU-aaaa"]),
            &observed,
        )
        .expect("역방향 순서가 거부됐다");
        assert_eq!(
            forward, reverse,
            "요구 목록의 순서가 결과를 바꿨다 — 같은 요구가 실행마다 다르게 보인다"
        );
    }

    /// 순수 커널은 GPU 를 만지지 않으므로, GPU 없는 기계에서도 위 판정이
    /// 전부 돈다. 여기서는 **실제 NVML 을 읽는 진입점**이 GPU 없는
    /// 기계에서 무엇을 하는지만 고정한다.
    ///
    /// ★ 개발 기계에는 NVIDIA GPU 가 없다(`CLAUDE.md` 환경 주의사항).
    ///   그러니 "실제 GPU 를 확인했다" 를 여기서 증명할 수 없고, 증명하는
    ///   척해서도 안 된다(§4). 고정하는 계약은 하나다 —
    ///   **NVML 이 없을 때 조용히 통과하지 않는다.**
    #[test]
    fn the_live_entry_point_never_passes_when_nvml_cannot_be_read() {
        let requirements = require(1, GIB, &["GPU-does-not-exist-on-this-machine"]);
        match check_gpu_requirements_now(&requirements) {
            Ok(ok) => panic!("존재하지 않는 UUID 요구가 통과했다: {:?}", ok.checked),
            Err(rejection) => {
                let text = rejection.to_string();
                assert!(
                    text.starts_with("PREFLIGHT_"),
                    "이유를 식별할 접두사가 없다: {text}"
                );
                // NVML 이 없는 기계면 '모름', NVML 이 있는 기계(예: x600)면
                // 이 가짜 UUID 는 없으므로 '식별자 불일치' 다. 둘 다 계약을
                // 만족한다 — 통과만 아니면 된다.
                assert!(
                    rejection.is_unknown()
                        || matches!(
                            &rejection,
                            GpuPreflightRejection::SelectedGpuAbsent { .. }
                                | GpuPreflightRejection::InsufficientGpuCount { .. }
                        ),
                    "예상 밖의 이유로 거부했다: {rejection}"
                );
            }
        }
    }
}
