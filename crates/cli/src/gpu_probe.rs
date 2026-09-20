//! `gputeer gpu-probe` — 이 기계의 GPU 를 NVML 로 실제로 읽어 출력한다.
//!
//! # 왜 필요한가
//!
//! `crates/runtime-nvml` 의 단위 테스트는 **GPU 가 없는 기계에서도 도는**
//! 계약만 고정한다(NVML 부재를 빈 목록으로 뭉개지 않는다). 실제로 값을
//! 읽어오는지는 GPU 있는 기계에서 돌려봐야만 알 수 있고, 이 저장소의
//! 개발 기계에는 NVIDIA GPU 가 없다(`CLAUDE.md` 환경 주의사항) —
//! 그래서 x600 에서 사람이 직접 돌릴 수 있는 진입점이 필요하다.
//!
//! ★ **이 명령의 통과를 "GPU 배치가 동작한다" 로 세지 않는다.** 이건
//!   조회일 뿐이다. 배치·점유·격리는 각각 별도 계층이다.

use gputeer_runtime_nvml::{observe, NvmlError};

pub fn run(_args: &[String]) -> Result<String, String> {
    match observe() {
        Ok(snapshot) => {
            let mut out = String::new();
            out.push_str(&format!(
                "GPU_PROBE_OK driver_version={} cuda_driver_version={} devices={}\n",
                snapshot.driver_version,
                snapshot.cuda_driver_version,
                snapshot.gpus.len()
            ));
            for gpu in &snapshot.gpus {
                // ★ 모르는 값을 0 이나 빈 문자열로 채우지 않는다 —
                //   `(모름)` 으로 적어 사실과 구분한다(`CLAUDE.md` §1).
                out.push_str(&format!(
                    "GPU uuid={} index={} name={:?} total_vram_bytes={} free_vram_bytes={} \
                     used_vram_bytes={} compute_capability={} mig={}\n",
                    gpu.uuid,
                    gpu.index,
                    gpu.name,
                    gpu.total_vram_bytes,
                    gpu.free_vram_bytes,
                    gpu.used_vram_bytes,
                    gpu.compute_capability.as_deref().unwrap_or("(모름)"),
                    match gpu.mig_enabled {
                        Some(true) => "enabled(PARTITIONED)",
                        Some(false) => "disabled",
                        None => "(지원 안 함)",
                    }
                ));
            }
            if snapshot.gpus.is_empty() {
                // "0개" 는 확인된 사실이다. 아래 Err 경로의 "모름" 과 다르다.
                out.push_str(
                    "GPU_PROBE_NOTE NVML 은 정상인데 장치가 0개다 — 이건 확인된 사실이다\n",
                );
            }
            Ok(out)
        }
        Err(NvmlError::LibraryUnavailable { detail }) => {
            // ★ 이것을 성공으로 세지 않는다. GPU 가 없는 것이 아니라
            //   GPU 가 있는지 **모르는** 상태다.
            Err(format!(
                "GPU_PROBE_UNKNOWN NVML 을 열 수 없어 이 기계의 GPU 유무를 확인하지 못했다 \
                 (GPU 가 0개라는 뜻이 아니다) — {detail}"
            ))
        }
        Err(other) => Err(format!("GPU_PROBE_FAILED {other}")),
    }
}
