use std::collections::{BTreeMap, BTreeSet};

/// `proto/common.proto`의 GPU allocation mode를 순수 kernel 입력으로 표현한다.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum GpuAllocationMode {
    Exclusive,
    Shared,
    Partitioned,
}

/// provenance 검증 결과는 이 kernel이 만들지 않고 caller가 입력한다.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProvenanceGate {
    Verified,
    Unverified,
}

/// scheduler가 소비할 수 있는 available VRAM과 아직 그 의미가 입증되지 않은 파생값.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AvailableVramObservation {
    Authoritative {
        bytes: u64,
    },
    /// `total - reserved`를 available로 해석하는 규범은 아직 없다.
    DerivedFromTotalAndReserved {
        total_bytes: u64,
        reserved_bytes: u64,
    },
}

/// 한 GPU에 대해 caller가 해소해 전달한 고정 관측값.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeGpuObservation {
    /// 이 첫 조각에서는 NVML UUID라고 주장하지 않는 opaque GPU ID다.
    pub gpu_id: String,
    pub healthy: Option<bool>,
    pub available_vram: Option<AvailableVramObservation>,
    pub model: Option<String>,
    pub driver_version: Option<u32>,
    pub compute_capability: Option<String>,
    /// runtime enforcement가 아니라 caller가 해소한 allocation 가능 mode다.
    pub allocation_modes: Option<BTreeSet<GpuAllocationMode>>,
}

/// 같은 관측 시각/revision에 고정된 GPU inventory 입력.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpuObservationSnapshot {
    pub observed_at_unix_ms: Option<u64>,
    pub inventory_revision: Option<u64>,
    pub gpus: Option<Vec<ScopeGpuObservation>>,
}

/// proto 기본값을 적용하기 전의 명시적 GPU 요구사항.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobGpuRequirements {
    pub minimum_vram_bytes_per_gpu: Option<u64>,
    pub minimum_gpu_count: Option<u32>,
    pub minimum_driver_version: Option<u32>,
    /// 호환성 규칙이 생기기 전까지 Some 값은 typed error로 닫는다.
    pub cuda_runtime_version: Option<String>,
    /// 빈 목록은 제약 없음이다.
    pub allowed_compute_capabilities: Vec<String>,
    pub allocation_mode: Option<GpuAllocationMode>,
    /// 빈 목록은 제약 없음이다.
    pub allowed_gpu_models: Vec<String>,
}

/// ResourceScope에 나중에 복사할 수 있지만 아직 wire 권한은 아닌 명시적 입력.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeResourceInput {
    pub cpu_cores: Option<u32>,
    pub ram_bytes: Option<u64>,
    pub workspace_bytes: Option<u64>,
    /// `Some(vec![])`은 쓰기 권한 없음, `None`은 누락이다.
    pub writable_prefixes: Option<Vec<String>>,
}

/// 고정 관측과 요구사항으로 계산한 assignment candidate.
///
/// 이 타입은 `ResourceScope`, Grant, Lease, reservation 또는 runtime enforcement가 아니다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeCandidate {
    pub observed_at_unix_ms: u64,
    pub inventory_revision: u64,
    pub selected_gpu_ids: Vec<String>,
    pub allocation_mode: GpuAllocationMode,
    pub cpu_cores: u32,
    pub ram_bytes: u64,
    pub workspace_bytes: u64,
    pub writable_prefixes: Vec<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ScopeMissingFact {
    ObservedAt,
    InventoryRevision,
    GpuInventory,
    JobMinimumVram,
    JobMinimumGpuCount,
    JobAllocationMode,
    CpuCores,
    RamBytes,
    WorkspaceBytes,
    WritablePrefixes,
    GpuHealth { gpu_id: String },
    GpuModel { gpu_id: String },
    GpuDriverVersion { gpu_id: String },
    GpuComputeCapability { gpu_id: String },
    GpuAllocationModes { gpu_id: String },
    AuthoritativeAvailableVram { gpu_id: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScopeError {
    UnverifiedProvenance,
    MissingFact(ScopeMissingFact),
    MinimumGpuCountMustBePositive,
    EmptyGpuId,
    NonCanonicalGpuId {
        gpu_id: String,
    },
    DuplicateGpuId {
        gpu_id: String,
    },
    EmptyWritablePrefix,
    NonCanonicalWritablePrefix {
        prefix: String,
    },
    DuplicateWritablePrefix {
        prefix: String,
    },
    NonAuthoritativeAvailableVram {
        gpu_id: String,
    },
    /// runtime 요구와 driver/capability의 호환 규칙은 이 첫 조각에 없다.
    CudaRuntimeCompatibilityUnresolved,
    InsufficientMatchingGpus {
        matching: u32,
        required: u32,
    },
    /// `[N/A]` 또는 caller의 mode claim만으로 MIG partition을 입증할 수 없다.
    PartitionedAllocationUnproven,
    /// 노드 단위 VRAM 예산을 강제할 수단이 입증되지 않았다.
    ///
    /// ★ 2026-09-09 추가. 이 자리가 **비어 있었다.** `Partitioned` 는 바로
    ///   위에서 거부하는데 `Shared` 는 아무 관문도 없어서, 노드가 Shared 를
    ///   광고하면 경고 없이 그대로 배치됐다.
    ///
    /// ★★ **이 거부는 규범을 옮긴 것이 아니다 — 규범보다 좁다.**
    ///   2026-09-10 재검수가 짚었다. 처음엔 "proto 조건을 구현했다" 고 적었다.
    ///   `proto/common.proto:172` 가 SHARED 를 여는 조건은 **둘**이다:
    ///   ```text
    ///   (가) 동일 소유자 Job 간
    ///   (나) Linux + MPS memory limit 확인 시
    ///   ```
    ///   이 커널은 **둘 다 판단할 입력이 없다.** 입력 구조체
    ///   (`ScopeGpuObservation`·`JobGpuRequirements`·`ScopeResourceInput`)에
    ///   같은 GPU 를 쓰는 다른 Job 의 제출자를 담을 칸도, MPS 설정 증거를
    ///   담을 칸도 없다. (`filter.rs` 의 `owner_member_id` 는 "노드 주인 대
    ///   제출자" 관계라 (가) 가 말하는 "같은 GPU 의 Job 끼리 주인이 같다"
    ///   와 다른 관계다.)
    ///   판단할 수 없는 것을 허용할 수 없으므로 **Shared 를 전부 거부한다.**
    ///   (가) 로 정당하게 열 수 있는 경우까지 막는 **잠정 제한**이다.
    ///   입력 칸이 생기면 (가) 부터 여는 것이 순서다 — (나) 는 아래 실측이
    ///   **x600 WSL2 에서** 만족시킬 수 없음을 보였고, (가) 는 **재지 않았다.**
    ///
    /// 실측이 (나) 에 대해 말해 주는 것(2026-09-08~09, x600):
    /// ```text
    /// MPS        WSL2 에서 **불가능**하다. 드라이버 번들에 MPS 바이너리가
    ///            없고 root 로도 compute mode 를 못 바꾼다
    /// 가로채기   유저스페이스 CUDA 심볼 가로채기로 한 프로세스의 동시
    ///            VRAM 사용량 상한은 실제로 걸렸다. 그러나 카운터가
    ///            **프로세스 로컬**이라 노드 단위 예산은 강제하지 못한다
    ///            — 프로세스를 여덟 개 띄우면 상한이 여덟 배가 된다
    /// ```
    /// 즉 **이번에 잰 환경(x600 WSL2)과 방식(MPS 확인 · 유저스페이스
    /// 가로채기)에서는** "이 노드의 VRAM 합이 용량을 넘지 않는다" 를 강제할
    /// 수단을 입증하지 못했다. proto 가 따로 적은 **네이티브 Linux + MPS**
    /// 환경은 재지 않았다(remote5090 대기) — 거기서 안 된다는 뜻이 아니다.
    /// ★ 2026-09-10 재검수 12 — 여기 "오늘 어느 플랫폼에서도 수단이 없다"
    ///   고 적었었다. 잰 것보다 넓게 말한 것이다.
    /// `CLAUDE.md` §0.4 — 강제할 수 없는 것을 보장으로 선언하지 않는다.
    ///
    /// 근거  `docs/evidence/_raw/WSL_GPU_MPS_실측_2026-09-08.txt`
    ///       `docs/evidence/_raw/VRAM_유저스페이스_가로채기_실측_2026-09-08.txt`
    SharedAllocationUnproven,
}

/// 고정 관측에서 deterministic `ScopeCandidate`를 계산한다.
///
/// I/O, 현재 시각, TTL, DB, network, 난수, 서명 검증 또는 상태 전이를 수행하지 않는다.
pub fn gpu_scope_candidate(
    snapshot: &GpuObservationSnapshot,
    requirements: &JobGpuRequirements,
    resources: &ScopeResourceInput,
    provenance: ProvenanceGate,
) -> Result<ScopeCandidate, ScopeError> {
    if provenance != ProvenanceGate::Verified {
        return Err(ScopeError::UnverifiedProvenance);
    }

    let observed_at_unix_ms = snapshot
        .observed_at_unix_ms
        .ok_or(ScopeError::MissingFact(ScopeMissingFact::ObservedAt))?;
    let inventory_revision = snapshot
        .inventory_revision
        .ok_or(ScopeError::MissingFact(ScopeMissingFact::InventoryRevision))?;
    let minimum_vram_bytes_per_gpu = requirements
        .minimum_vram_bytes_per_gpu
        .ok_or(ScopeError::MissingFact(ScopeMissingFact::JobMinimumVram))?;
    let minimum_gpu_count = requirements
        .minimum_gpu_count
        .ok_or(ScopeError::MissingFact(
            ScopeMissingFact::JobMinimumGpuCount,
        ))?;
    if minimum_gpu_count == 0 {
        return Err(ScopeError::MinimumGpuCountMustBePositive);
    }
    if requirements.cuda_runtime_version.is_some() {
        return Err(ScopeError::CudaRuntimeCompatibilityUnresolved);
    }
    let allocation_mode = requirements
        .allocation_mode
        .ok_or(ScopeError::MissingFact(ScopeMissingFact::JobAllocationMode))?;
    if allocation_mode == GpuAllocationMode::Partitioned {
        return Err(ScopeError::PartitionedAllocationUnproven);
    }
    if allocation_mode == GpuAllocationMode::Shared {
        return Err(ScopeError::SharedAllocationUnproven);
    }

    let cpu_cores = resources
        .cpu_cores
        .ok_or(ScopeError::MissingFact(ScopeMissingFact::CpuCores))?;
    let ram_bytes = resources
        .ram_bytes
        .ok_or(ScopeError::MissingFact(ScopeMissingFact::RamBytes))?;
    let workspace_bytes = resources
        .workspace_bytes
        .ok_or(ScopeError::MissingFact(ScopeMissingFact::WorkspaceBytes))?;
    let writable_prefixes = canonical_prefixes(
        resources
            .writable_prefixes
            .as_ref()
            .ok_or(ScopeError::MissingFact(ScopeMissingFact::WritablePrefixes))?,
    )?;
    let gpus = snapshot
        .gpus
        .as_ref()
        .ok_or(ScopeError::MissingFact(ScopeMissingFact::GpuInventory))?;
    validate_gpu_ids(gpus)?;

    let allowed_models = requirements
        .allowed_gpu_models
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let allowed_compute_capabilities = requirements
        .allowed_compute_capabilities
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut canonical_gpus = gpus.iter().collect::<Vec<_>>();
    canonical_gpus.sort_by(|left, right| left.gpu_id.cmp(&right.gpu_id));

    let mut matching = Vec::new();
    for gpu in canonical_gpus {
        match gpu.healthy {
            None => {
                return Err(ScopeError::MissingFact(ScopeMissingFact::GpuHealth {
                    gpu_id: gpu.gpu_id.clone(),
                }));
            }
            Some(false) => continue,
            Some(true) => {}
        }
        if !allowed_models.is_empty() {
            let model = gpu.model.as_ref().ok_or_else(|| {
                ScopeError::MissingFact(ScopeMissingFact::GpuModel {
                    gpu_id: gpu.gpu_id.clone(),
                })
            })?;
            if !allowed_models.contains(model.as_str()) {
                continue;
            }
        }
        if let Some(required_driver) = requirements.minimum_driver_version {
            let driver = gpu.driver_version.ok_or_else(|| {
                ScopeError::MissingFact(ScopeMissingFact::GpuDriverVersion {
                    gpu_id: gpu.gpu_id.clone(),
                })
            })?;
            if driver < required_driver {
                continue;
            }
        }
        if !allowed_compute_capabilities.is_empty() {
            let capability = gpu.compute_capability.as_ref().ok_or_else(|| {
                ScopeError::MissingFact(ScopeMissingFact::GpuComputeCapability {
                    gpu_id: gpu.gpu_id.clone(),
                })
            })?;
            if !allowed_compute_capabilities.contains(capability.as_str()) {
                continue;
            }
        }
        let allocation_modes = gpu.allocation_modes.as_ref().ok_or_else(|| {
            ScopeError::MissingFact(ScopeMissingFact::GpuAllocationModes {
                gpu_id: gpu.gpu_id.clone(),
            })
        })?;
        if !allocation_modes.contains(&allocation_mode) {
            continue;
        }
        let available_vram = match gpu.available_vram.as_ref() {
            None => {
                return Err(ScopeError::MissingFact(
                    ScopeMissingFact::AuthoritativeAvailableVram {
                        gpu_id: gpu.gpu_id.clone(),
                    },
                ));
            }
            Some(AvailableVramObservation::Authoritative { bytes }) => *bytes,
            Some(AvailableVramObservation::DerivedFromTotalAndReserved { .. }) => {
                return Err(ScopeError::NonAuthoritativeAvailableVram {
                    gpu_id: gpu.gpu_id.clone(),
                });
            }
        };
        if available_vram >= minimum_vram_bytes_per_gpu {
            matching.push((available_vram, gpu.gpu_id.as_str()));
        }
    }

    matching.sort_unstable();
    let required = usize::try_from(minimum_gpu_count).unwrap_or(usize::MAX);
    if matching.len() < required {
        return Err(ScopeError::InsufficientMatchingGpus {
            matching: u32::try_from(matching.len()).unwrap_or(u32::MAX),
            required: minimum_gpu_count,
        });
    }
    let mut selected_gpu_ids = matching[..required]
        .iter()
        .map(|(_, gpu_id)| (*gpu_id).to_owned())
        .collect::<Vec<_>>();
    selected_gpu_ids.sort_unstable();

    Ok(ScopeCandidate {
        observed_at_unix_ms,
        inventory_revision,
        selected_gpu_ids,
        allocation_mode,
        cpu_cores,
        ram_bytes,
        workspace_bytes,
        writable_prefixes,
    })
}

fn validate_gpu_ids(gpus: &[ScopeGpuObservation]) -> Result<(), ScopeError> {
    if gpus.iter().any(|gpu| gpu.gpu_id.trim().is_empty()) {
        return Err(ScopeError::EmptyGpuId);
    }
    if let Some(gpu_id) = gpus
        .iter()
        .filter(|gpu| gpu.gpu_id.trim() != gpu.gpu_id)
        .map(|gpu| gpu.gpu_id.as_str())
        .min()
    {
        return Err(ScopeError::NonCanonicalGpuId {
            gpu_id: gpu_id.to_owned(),
        });
    }
    let mut counts = BTreeMap::<&str, usize>::new();
    for gpu in gpus {
        *counts.entry(gpu.gpu_id.as_str()).or_default() += 1;
    }
    if let Some((gpu_id, _)) = counts.into_iter().find(|(_, count)| *count > 1) {
        return Err(ScopeError::DuplicateGpuId {
            gpu_id: gpu_id.to_owned(),
        });
    }
    Ok(())
}

fn canonical_prefixes(prefixes: &[String]) -> Result<Vec<String>, ScopeError> {
    if prefixes.iter().any(|prefix| prefix.trim().is_empty()) {
        return Err(ScopeError::EmptyWritablePrefix);
    }
    if let Some(prefix) = prefixes
        .iter()
        .filter(|prefix| prefix.trim() != prefix.as_str())
        .min()
    {
        return Err(ScopeError::NonCanonicalWritablePrefix {
            prefix: prefix.clone(),
        });
    }
    let canonical = prefixes.iter().cloned().collect::<BTreeSet<_>>();
    if canonical.len() != prefixes.len() {
        let mut counts = BTreeMap::<&str, usize>::new();
        for prefix in prefixes {
            *counts.entry(prefix.as_str()).or_default() += 1;
        }
        let prefix = counts
            .into_iter()
            .find(|(_, count)| *count > 1)
            .expect("a shorter set requires a duplicate")
            .0;
        return Err(ScopeError::DuplicateWritablePrefix {
            prefix: prefix.to_owned(),
        });
    }
    Ok(canonical.into_iter().collect())
}
