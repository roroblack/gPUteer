//! 노드가 Hello 에 실어 보낸 GPU 관측(결함 301 · `signing.md` §6.6)을 검사하고 등록된 선언과 대조한다.
//!
//! ★ 관측은 노드 자기보고다(`WORKER_REPORTED`). 여기서 하는 일은 **선언이 지금도 사실인가** 의 증거를 모으는 것뿐이다 —
//!   선언(운영자가 `import-inventory` 로 넣은 것)을 관측으로 고치지 않는다. 둘을 섞으면 어느 값이 누구 말인지 잃는다.
//!
//! 세 단계다:
//!
//! ```text
//! check_hello_observation_shape   구조 · 형식. 어기면 Hello 자체를 거부한다(모든 lane)
//! observation_time_usable         시각. 어기면 확인에 쓰지 않을 뿐 Hello 는 받는다(풀 FRESH)
//! match_declaration               대조. 맞을 때만 확인 기록을 남긴다(`inventory_store::record_gpu_attestation`)
//! ```
//!
//! 설계: `docs/contracts/proposals/2026-09-25_1623_풀_신호와_노드_관측_서명.md` "적용 설계".

use gputeer_protocol::constants::{
    AGENT_SESSION_HELLO_GPU_OBSERVATION_MIN_SCHEMA_VERSION, GPU_OBSERVATION_MAX_AGE_MS,
    GPU_OBSERVATION_MAX_GPUS, MODE_MULTI_AGENT_GRANT,
};
use gputeer_protocol::pb;

use crate::inventory_store::GpuInventory;

/// 구조 규칙(`signing.md` §6.6 MUST). 관측이 없으면 통과한다.
///
/// 거부 사유는 `HELLO_REJECTED:` 로 시작한다 — 호출자가 그대로 Protocol 오류로 쓴다.
pub fn check_hello_observation_shape(hello: &pb::AgentSessionHello) -> Result<(), String> {
    let Some(observation) = hello.gpu_observation.as_ref() else {
        return Ok(());
    };
    if hello.schema_version < AGENT_SESSION_HELLO_GPU_OBSERVATION_MIN_SCHEMA_VERSION {
        return Err(format!(
            "HELLO_REJECTED: schema v{} Hello 에 GPU 관측(8)이 있다 — v2 에서만 쓴다",
            hello.schema_version
        ));
    }
    if hello.mode != MODE_MULTI_AGENT_GRANT {
        return Err(format!(
            "HELLO_REJECTED: FRESH 가 아닌 Hello(mode {})에 GPU 관측(8)이 있다 — 일을 받으러 오는 연결에서만 싣는다",
            hello.mode
        ));
    }
    let gpus = &observation.gpus;
    if gpus.is_empty() || gpus.len() > GPU_OBSERVATION_MAX_GPUS {
        return Err(format!(
            "HELLO_REJECTED: GPU 관측이 {} 개다 — 1~{GPU_OBSERVATION_MAX_GPUS} 개여야 한다",
            gpus.len()
        ));
    }
    for gpu in gpus {
        if gpu.uuid.is_empty() || gpu.model.is_empty() || gpu.total_vram_bytes == 0 {
            return Err(format!(
                "HELLO_REJECTED: GPU 관측에 빈 값이 있다(uuid {:?} · 이름 {:?} · 총 VRAM {})",
                gpu.uuid, gpu.model, gpu.total_vram_bytes
            ));
        }
    }
    // 오름차순 · 중복 없음 — 같은 관측이 한 가지 바이트로만 서명되게 한다.
    if gpus.windows(2).any(|pair| pair[0].uuid >= pair[1].uuid) {
        return Err(
            "HELLO_REJECTED: GPU 관측이 uuid 오름차순이 아니거나 같은 uuid 가 두 번 있다"
                .to_string(),
        );
    }
    Ok(())
}

/// 시각 규칙 — 관측이 Hello 서명보다 늦지 않고, 60초 넘게 이르지 않아야 확인에 쓴다.
///
/// ★ Hello 의 시각 자체는 서명 검증이 이미 Coordinator 시계와 대조했다. 여기서는 관측이 그 Hello 와 같은 때의 것인지만 본다.
pub fn observation_time_usable(
    hello: &pb::AgentSessionHello,
    observation: &pb::NodeGpuObservation,
) -> Result<(), String> {
    if observation.observed_at_unix_ms > hello.issued_at_unix_ms {
        return Err(format!(
            "관측 시각({})이 Hello 시각({})보다 늦다",
            observation.observed_at_unix_ms, hello.issued_at_unix_ms
        ));
    }
    let age = hello.issued_at_unix_ms - observation.observed_at_unix_ms;
    if age > GPU_OBSERVATION_MAX_AGE_MS {
        return Err(format!(
            "관측이 Hello 보다 {age}ms 이르다(상한 {GPU_OBSERVATION_MAX_AGE_MS}ms)"
        ));
    }
    Ok(())
}

/// 선언 gpu_id 가 NVML UUID 모양인가 — `GPU-` 또는 `MIG-` 뒤에 8-4-4-4-12 자리 16진수. 설치 자동화의 `<노드>-gpu-<번호>` 는 아니다.
///
/// ★ 2026-09-25 (결함 411 · 재검수 97) — 처음엔 접두어만 봐서, 노드 이름이 `GPU` 인 가입 파일의 `GPU-gpu-0` 을 UUID 로 오인했다.
/// ★ 옛 MIG 형식(`MIG-GPU-<uuid>/<gi>/<ci>`)은 UUID 로 보지 않는다 — 모델 · VRAM 으로 짝짓는다(대체 짝을 막지 못한다).
fn declared_as_uuid(gpu_id: &str) -> bool {
    let Some(rest) = gpu_id
        .strip_prefix("GPU-")
        .or_else(|| gpu_id.strip_prefix("MIG-"))
    else {
        return false;
    };
    let groups: Vec<&str> = rest.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.chars().all(|c| c.is_ascii_hexdigit()))
}

/// 선언의 GPU 마다 **서로 다른** 관측 GPU 하나씩을 짝지을 수 있는가.
///
/// 짝이 되는 조건:
///
/// ```text
/// 선언 model 이 있으면              관측 이름과 같다
/// 선언 available_vram_bytes 가 있으면  관측 총 VRAM 이하다
/// 선언 gpu_id 가 UUID 모양(GPU-/MIG-)이면  그 UUID 의 관측 GPU 하고만 짝짓는다 — 관측에 없으면 짝이 없다
/// ```
///
/// ★ UUID 로만 짝짓지 않는 이유 — 설치 자동화의 가입 파일은 `gpu_id` 를 `<노드>-gpu-<번호>` 로 적는다. 선언의 `gpu_id` 는
///   스케줄러 식별자이지 NVML UUID 가 아니다(`scheduler/src/scope.rs` 가 같은 말을 한다).
/// ★ 관측이 선언보다 많아도 된다(소유자가 GPU 하나를 풀에 내놓지 않을 수 있다). 선언이 비었으면 증거가 아니다 — 호출자가 거른다.
///
/// 이분 매칭(증가 경로)이다 — 탐욕으로 고르면 같은 모델 · 다른 VRAM 조합에서 맞는 선언을 틀렸다고 할 수 있다.
pub fn match_declaration(
    declared: &[GpuInventory],
    observed: &[pb::ObservedGpu],
) -> Result<(), String> {
    let fits = |d: &GpuInventory, o: &pb::ObservedGpu| -> bool {
        // ★ 2026-09-25 (결함 409 · 재검수 96) — UUID 로 선언했으면 **그 UUID 하고만** 짝짓는다. 전에는 "관측에 그 UUID 가 있으면" 만
        //   묶어서, 선언한 GPU 가 빠지면 같은 모델의 다른 GPU 가 대신 짝이 돼 옛 선언이 계속 신선했다.
        (!declared_as_uuid(&d.gpu_id) || o.uuid == d.gpu_id)
            && d.model.as_deref().is_none_or(|model| model == o.model)
            && d.available_vram_bytes
                .is_none_or(|vram| vram <= o.total_vram_bytes)
    };
    // owner[j] = 관측 j 와 짝지은 선언 번호
    let mut owner: Vec<Option<usize>> = vec![None; observed.len()];
    fn augment(
        i: usize,
        declared: &[GpuInventory],
        observed: &[pb::ObservedGpu],
        fits: &dyn Fn(&GpuInventory, &pb::ObservedGpu) -> bool,
        seen: &mut [bool],
        owner: &mut [Option<usize>],
    ) -> bool {
        for j in 0..observed.len() {
            if seen[j] || !fits(&declared[i], &observed[j]) {
                continue;
            }
            seen[j] = true;
            let free = match owner[j] {
                None => true,
                Some(k) => augment(k, declared, observed, fits, seen, owner),
            };
            if free {
                owner[j] = Some(i);
                return true;
            }
        }
        false
    }
    for (i, gpu) in declared.iter().enumerate() {
        let mut seen = vec![false; observed.len()];
        if !augment(i, declared, observed, &fits, &mut seen, &mut owner) {
            return Err(format!(
                "선언 GPU {:?}(model {:?} · VRAM {:?})와 짝지을 관측 GPU 가 없다 — 관측: {}",
                gpu.gpu_id,
                gpu.model,
                gpu.available_vram_bytes,
                observed
                    .iter()
                    .map(|o| format!("{}:{}:{}", o.uuid, o.model, o.total_vram_bytes))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL: &str = "NVIDIA GeForce RTX 4070 SUPER";
    const TOTAL: u64 = 12_878_610_432;
    const OLD: &str = "GPU-0a1b2c3d-0000-4000-8000-00000000000a";
    const NEW: &str = "GPU-0a1b2c3d-0000-4000-8000-00000000000b";

    fn declared(id: &str, model: Option<&str>, vram: Option<u64>) -> GpuInventory {
        GpuInventory {
            gpu_id: id.into(),
            model: model.map(str::to_string),
            healthy: Some(true),
            available_vram_bytes: vram,
        }
    }

    fn observed(uuid: &str, model: &str, total: u64) -> pb::ObservedGpu {
        pb::ObservedGpu {
            uuid: uuid.into(),
            model: model.into(),
            total_vram_bytes: total,
        }
    }

    fn hello(schema: u32, mode: i32, gpus: Vec<pb::ObservedGpu>) -> pb::AgentSessionHello {
        pb::AgentSessionHello {
            schema_version: schema,
            mode,
            issued_at_unix_ms: 1_000_000,
            gpu_observation: Some(pb::NodeGpuObservation {
                observed_at_unix_ms: 999_000,
                gpus,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn install_style_declaration_matches_by_model_and_vram() {
        // 가입 파일 모양 — gpu_id 는 번호, VRAM 은 nvidia-smi MiB 를 바이트로(총 VRAM 이하)
        let d = [declared(
            "node-a-gpu-0",
            Some(MODEL),
            Some(12_282 * 1_048_576),
        )];
        let o = [observed("GPU-1", MODEL, TOTAL)];
        assert_eq!(match_declaration(&d, &o), Ok(()));
    }

    #[test]
    fn missing_gpu_model_mismatch_and_too_much_vram_do_not_match() {
        let o = [observed("GPU-1", MODEL, TOTAL)];
        // GPU 두 장 선언 · 한 장 관측 — GPU 가 빠졌다
        let two = [
            declared("n-gpu-0", Some(MODEL), None),
            declared("n-gpu-1", Some(MODEL), None),
        ];
        assert!(match_declaration(&two, &o).is_err());
        // 다른 모델
        assert!(match_declaration(&[declared("n-gpu-0", Some("RTX 3060"), None)], &o).is_err());
        // 선언 VRAM 이 관측 총 VRAM 보다 크다
        assert!(
            match_declaration(&[declared("n-gpu-0", Some(MODEL), Some(TOTAL + 1))], &o).is_err()
        );
    }

    #[test]
    fn a_uuid_declaration_binds_only_to_that_gpu() {
        let o = [observed(OLD, MODEL, TOTAL), observed(NEW, MODEL, TOTAL)];
        assert_eq!(
            match_declaration(&[declared(NEW, Some(MODEL), None)], &o),
            Ok(())
        );
        // 선언 UUID 의 GPU 가 다른 모델이면 — 같은 모델의 다른 GPU 로 대신 짝짓지 않는다
        let o2 = [
            observed(OLD, MODEL, TOTAL),
            observed(NEW, "RTX 3060", TOTAL),
        ];
        assert!(match_declaration(&[declared(NEW, Some(MODEL), None)], &o2).is_err());
    }

    /// ★ 결함 409 — UUID 로 선언한 GPU 가 빠지고 같은 모델 · 용량의 다른 GPU 가 꽂혔다. 대신 짝짓지 않는다.
    #[test]
    fn a_uuid_declared_gpu_that_disappeared_is_not_replaced_by_a_look_alike() {
        let d = [declared(OLD, Some(MODEL), Some(TOTAL))];
        assert!(match_declaration(&d, &[observed(NEW, MODEL, TOTAL)]).is_err());
        assert_eq!(
            match_declaration(&d, &[observed(OLD, MODEL, TOTAL)]),
            Ok(())
        );
    }

    /// ★ 결함 411 — 노드 이름이 `GPU` 인 번호형 선언(`GPU-gpu-0`)은 UUID 가 아니다. 모델 · VRAM 으로 짝짓는다.
    #[test]
    fn a_numbered_declaration_that_starts_with_gpu_is_not_a_uuid() {
        let d = [declared("GPU-gpu-0", Some(MODEL), Some(TOTAL))];
        assert_eq!(
            match_declaration(&d, &[observed(NEW, MODEL, TOTAL)]),
            Ok(())
        );
        assert!(declared_as_uuid(OLD));
        assert!(declared_as_uuid("MIG-0a1b2c3d-0000-4000-8000-00000000000a"));
        for not_uuid in [
            "GPU-gpu-0",
            "GPU-",
            "node-gpu-0",
            "GPU-0a1b2c3d-0000-4000-8000-00000000000",
            "MIG-GPU-x/1/0",
        ] {
            assert!(!declared_as_uuid(not_uuid), "{not_uuid}");
        }
    }

    /// 탐욕으로 고르면 틀리는 조합 — 첫 선언(VRAM 조건 없음)이 큰 GPU 를 먼저 잡으면 둘째(큰 VRAM 필요)가 남는 것이 없다.
    #[test]
    fn matching_is_not_greedy() {
        let d = [
            declared("n-gpu-0", Some(MODEL), None),
            declared("n-gpu-1", Some(MODEL), Some(TOTAL)),
        ];
        let o = [
            observed("GPU-1", MODEL, TOTAL),
            observed("GPU-2", MODEL, 8_000_000_000),
        ];
        assert_eq!(match_declaration(&d, &o), Ok(()));
    }

    #[test]
    fn extra_observed_gpus_are_allowed() {
        let o = [
            observed("GPU-1", MODEL, TOTAL),
            observed("GPU-2", MODEL, TOTAL),
        ];
        assert_eq!(
            match_declaration(&[declared("n-gpu-0", Some(MODEL), None)], &o),
            Ok(())
        );
    }

    #[test]
    fn shape_rules() {
        let one = || vec![observed("GPU-1", MODEL, TOTAL)];
        assert_eq!(
            check_hello_observation_shape(&hello(2, MODE_MULTI_AGENT_GRANT, one())),
            Ok(())
        );
        // v1 에 관측
        assert!(check_hello_observation_shape(&hello(1, MODE_MULTI_AGENT_GRANT, one())).is_err());
        // FRESH 가 아닌 Hello 에 관측
        assert!(check_hello_observation_shape(&hello(
            2,
            gputeer_protocol::constants::MODE_RENEW,
            one()
        ))
        .is_err());
        // 빈 목록 · 빈 값 · VRAM 0 · 순서 · 중복
        assert!(check_hello_observation_shape(&hello(2, MODE_MULTI_AGENT_GRANT, vec![])).is_err());
        assert!(check_hello_observation_shape(&hello(
            2,
            MODE_MULTI_AGENT_GRANT,
            vec![observed("GPU-1", "", TOTAL)]
        ))
        .is_err());
        assert!(check_hello_observation_shape(&hello(
            2,
            MODE_MULTI_AGENT_GRANT,
            vec![observed("GPU-1", MODEL, 0)]
        ))
        .is_err());
        assert!(check_hello_observation_shape(&hello(
            2,
            MODE_MULTI_AGENT_GRANT,
            vec![
                observed("GPU-2", MODEL, TOTAL),
                observed("GPU-1", MODEL, TOTAL)
            ]
        ))
        .is_err());
        assert!(check_hello_observation_shape(&hello(
            2,
            MODE_MULTI_AGENT_GRANT,
            vec![
                observed("GPU-1", MODEL, TOTAL),
                observed("GPU-1", MODEL, TOTAL)
            ]
        ))
        .is_err());
        let too_many = (0..=GPU_OBSERVATION_MAX_GPUS)
            .map(|i| observed(&format!("GPU-{i:03}"), MODEL, TOTAL))
            .collect();
        assert!(
            check_hello_observation_shape(&hello(2, MODE_MULTI_AGENT_GRANT, too_many)).is_err()
        );
        // 관측 없는 v1 · v2 는 통과
        let mut bare = hello(1, MODE_MULTI_AGENT_GRANT, vec![]);
        bare.gpu_observation = None;
        assert_eq!(check_hello_observation_shape(&bare), Ok(()));
    }

    #[test]
    fn time_rules() {
        let h = hello(
            2,
            MODE_MULTI_AGENT_GRANT,
            vec![observed("GPU-1", MODEL, TOTAL)],
        );
        let at = |t: u64| pb::NodeGpuObservation {
            observed_at_unix_ms: t,
            gpus: vec![],
        };
        assert_eq!(observation_time_usable(&h, &at(1_000_000)), Ok(()));
        assert_eq!(
            observation_time_usable(&h, &at(1_000_000 - GPU_OBSERVATION_MAX_AGE_MS)),
            Ok(())
        );
        assert!(observation_time_usable(&h, &at(1_000_001)).is_err());
        assert!(
            observation_time_usable(&h, &at(1_000_000 - GPU_OBSERVATION_MAX_AGE_MS - 1)).is_err()
        );
    }
}
