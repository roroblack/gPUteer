//! `check_gpu_requirements_now()` 를 **실물 NVML** 에 대고 한 번 잰다.
//!
//! # 왜 이 프로브가 필요한가 (2026-09-06)
//!
//! `preflight.rs` 의 테스트 32건은 전부 **합성 관측값**으로 돈다. 순수
//! 판정 로직은 그것으로 충분히 재지만, **`check_gpu_requirements_now()` 가
//! 진짜 NVML 을 제대로 읽는지는 아무것도 증명하지 않는다.** 그 함수만
//! 통째로 망가져 있어도 32건이 전부 초록이다.
//!
//! 개발 기계에는 NVIDIA GPU 가 없어서(Intel Iris Xe) 여기서는 못 잰다.
//! GPU 가 있는 기계(x600, RTX 4070 SUPER)에서 이것을 돌려 확인한다.
//!
//! # 무엇을 재는가
//!
//! ```text
//! 1  실제 관측이 되는가          NvmlSnapshot 이 장치를 실제로 읽는가
//! 2  맞는 요구는 통과하는가       지금 있는 UUID·지금 있는 여유 VRAM
//! 3  틀린 요구는 거부하는가       없는 UUID · 과한 VRAM · 과한 개수
//! ```
//!
//! ★ **3번이 없으면 2번은 아무 뜻이 없다.** "항상 통과" 하는 구현도
//!   2번을 통과하기 때문이다.
//!
//! ```text
//! preflight_probe
//! ```

#[cfg(windows)]
fn main() {
    real_main()
}

#[cfg(target_os = "linux")]
fn main() {
    real_main()
}

#[cfg(not(any(windows, target_os = "linux")))]
fn main() {
    eprintln!("이 프로브는 Windows/Linux 전용이다");
    std::process::exit(2);
}

#[allow(dead_code)]
fn real_main() {
    use gputeer_runtime_nvml::preflight::{check_gpu_requirements_now, GpuRequirements};

    // ── 1. 실제로 관측되는가 ─────────────────────────────────────────
    let snapshot = match gputeer_runtime_nvml::observe() {
        Ok(s) => s,
        Err(e) => {
            // ★ 이것도 결과다. "NVML 을 못 읽었다" 와 "GPU 가 모자란다" 는
            //   다른 사실이고, 그 구분이 이 계층의 설계 요점이다.
            println!("PREFLIGHT_PROBE stage=observe ok=false detail={e:?}");
            std::process::exit(1);
        }
    };
    println!(
        "PREFLIGHT_PROBE stage=observe ok=true gpu_count={}",
        snapshot.gpus.len()
    );
    for gpu in &snapshot.gpus {
        println!(
            "  GPU index={} uuid={} total={}B free={}B mig={:?}",
            gpu.index, gpu.uuid, gpu.total_vram_bytes, gpu.free_vram_bytes, gpu.mig_enabled
        );
    }
    let Some(first) = snapshot.gpus.first() else {
        println!("PREFLIGHT_PROBE stage=observe ok=false detail=장치가 하나도 없다");
        std::process::exit(1);
    };

    // ── 2. 지금 사실에 맞는 요구는 통과하는가 ────────────────────────
    //
    // ★ 여유 VRAM 을 **지금 값의 절반**으로 잡는다. 지금 값 그대로 쓰면
    //   프로브가 도는 사이 다른 프로세스가 조금만 잡아도 흔들린다.
    let half_free = first.free_vram_bytes / 2;
    let ok_req = GpuRequirements {
        required_gpu_count: 1,
        minimum_free_vram_bytes_per_gpu: half_free,
        selected_gpu_uuids: vec![first.uuid.clone()],
    };
    match check_gpu_requirements_now(&ok_req) {
        Ok(_) => println!("PREFLIGHT_PROBE stage=accept ok=true need_free={half_free}B"),
        Err(rej) => println!("PREFLIGHT_PROBE stage=accept ok=false rejection={rej:?}"),
    }

    // ── 3. 틀린 요구는 거부하는가 (대조군 셋) ────────────────────────
    //
    // ★★ 이게 없으면 위 2번은 "항상 통과" 하는 구현과 구분되지 않는다.
    let absent = GpuRequirements {
        required_gpu_count: 1,
        minimum_free_vram_bytes_per_gpu: 0,
        selected_gpu_uuids: vec!["GPU-00000000-0000-0000-0000-000000000000".to_string()],
    };
    match check_gpu_requirements_now(&absent) {
        Ok(_) => println!("PREFLIGHT_PROBE stage=reject_absent ok=false detail=없는 UUID 를 통과시켰다"),
        Err(rej) => println!("PREFLIGHT_PROBE stage=reject_absent ok=true rejection={rej:?}"),
    }

    let too_much = GpuRequirements {
        required_gpu_count: 1,
        // 실제 총량보다 크게 — 절대 만족될 수 없다.
        minimum_free_vram_bytes_per_gpu: first.total_vram_bytes.saturating_mul(4),
        selected_gpu_uuids: vec![first.uuid.clone()],
    };
    match check_gpu_requirements_now(&too_much) {
        Ok(_) => println!("PREFLIGHT_PROBE stage=reject_vram ok=false detail=과한 VRAM 요구를 통과시켰다"),
        Err(rej) => println!("PREFLIGHT_PROBE stage=reject_vram ok=true rejection={rej:?}"),
    }

    let too_many = GpuRequirements {
        required_gpu_count: (snapshot.gpus.len() as u32).saturating_add(1),
        minimum_free_vram_bytes_per_gpu: 0,
        selected_gpu_uuids: snapshot
            .gpus
            .iter()
            .map(|g| g.uuid.clone())
            .chain(std::iter::once(
                "GPU-11111111-1111-1111-1111-111111111111".to_string(),
            ))
            .collect(),
    };
    match check_gpu_requirements_now(&too_many) {
        Ok(_) => println!("PREFLIGHT_PROBE stage=reject_count ok=false detail=있는 것보다 많은 개수를 통과시켰다"),
        Err(rej) => println!("PREFLIGHT_PROBE stage=reject_count ok=true rejection={rej:?}"),
    }

    println!("PREFLIGHT_PROBE done");
}
