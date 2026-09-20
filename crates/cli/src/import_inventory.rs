//! `gputeer import-inventory` — 운영자가 만든 bootstrap 문서를 durable
//! inventory store 에 **반입**한다.
//!
//! # 왜 "검증" 이라고 부르지 않는가
//!
//! `gputeer import-manifest` 는 "제출자 서명 + 운영자 keyring" 이라는
//! 진짜 신뢰 경계가 있었다. **여기엔 없다.**
//!
//! ```text
//! AgentRegistry · AgentInventory   proto 아님. coordinator 의 Rust 구조체다
//! 서명 domain 목록                 inventory/bootstrap domain 이 없다
//! NodeHeartbeat                    서명 대상이지만 짧은 수명 자기보고다 —
//!                                  inventory 전체나 등록 승인을 서명하지 않는다
//! ```
//!
//! 그래서 신뢰 경계가 암호가 아니라 **이 명령을 실행할 권한과 그 파일의
//! OS 권한**이다. 이 명령이 정직하게 주장할 수 있는 것은 이것뿐이다:
//!
//! > 운영자가 준 서명 없는 bootstrap 문서를 지원 스키마로 해석하고,
//! > 필드 정합성·충돌·revision·영속성을 검사한 뒤 원자적으로 반입했다.
//!
//! ★ 증명하지 **못하는** 것 — 이 Agent 가 실제로 그 키를 가졌는지, 보고된
//!   GPU 가 실재하는지, 그 노드가 지금 살아 있는지, owner membership 이
//!   유효한지. 그래서 출력에도 문서에도 **"verified"·"검증됨" 을 쓰지
//!   않는다.**

use std::collections::{BTreeMap, BTreeSet};

use gputeer_coordinator::inventory_store::{
    AgentBootstrap, AgentInventory, AgentRegistry, CoordinatorInventoryStore, GpuInventory,
};
use gputeer_scheduler::{
    IsolationClass, KeyProtection, NodeState, RiskState, SecurityTier, WorkloadClass,
};
use serde::Deserialize;

/// 이 빌드가 아는 bootstrap 문서 판.
const SUPPORTED_SCHEMA_VERSION: u32 = 1;

/// `gputeer import-inventory` 진입점.
pub fn run(args: &[String]) -> Result<String, String> {
    let flags = parse_flags(args)?;
    let inventory_path = require(&flags, "--inventory")?;
    let db_path = require(&flags, "--inventory-db")?;

    let text = std::fs::read_to_string(inventory_path)
        .map_err(|e| format!("bootstrap 문서를 읽지 못했다({inventory_path}): {e}"))?;
    let document: BootstrapDocument = serde_json::from_str(&text)
        .map_err(|e| format!("BOOTSTRAP_REJECTED: 문서 해석 실패: {e}"))?;

    if document.schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(format!(
            "BOOTSTRAP_REJECTED: 이 빌드가 모르는 schema_version({}) — 아는 것은 {SUPPORTED_SCHEMA_VERSION} 이다",
            document.schema_version
        ));
    }
    if document.agents.is_empty() {
        return Err(
            "BOOTSTRAP_REJECTED: agents 가 비어 있다 — 아무것도 반입하지 않는 문서를 성공으로 보고하지 않는다"
                .into(),
        );
    }

    // ★ **파일 전체를 먼저 검사한다.** 한 항목이라도 틀리면 DB 를 열기도
    //   전에 끝난다 — 저장소가 원자적이어도, 열어서 잠그는 것 자체가
    //   다른 작업을 기다리게 만든다.
    // 시각은 여기서 **한 번만** 읽는다 — 항목마다 읽으면 같은 문서 안에서
    // 기준이 달라져, 같은 값이 앞 항목에선 통과하고 뒤 항목에선 거부된다.
    let now_unix_ms = now_unix_ms();
    let mut entries = Vec::with_capacity(document.agents.len());
    let mut seen_nodes = BTreeSet::new();
    for (index, agent) in document.agents.iter().enumerate() {
        let entry = agent
            .to_bootstrap(now_unix_ms)
            .map_err(|e| format!("BOOTSTRAP_REJECTED: agents[{index}]: {e}"))?;
        // ★ 한 파일 안의 중복 노드는 **정확히 같아도** 거부한다 — 어느
        //   쪽이 정본인지 문서가 말하지 않는다. 저장소의 멱등 규칙은
        //   "같은 문서를 다시 실행" 을 위한 것이지 "한 문서에 두 번 쓰기"
        //   가 아니다.
        if !seen_nodes.insert(entry.registry.node_id.clone()) {
            return Err(format!(
                "BOOTSTRAP_REJECTED: agents[{index}]: 같은 문서에 node_id 가 두 번 나온다({})",
                entry.registry.node_id
            ));
        }
        entries.push(entry);
    }

    let mut store = CoordinatorInventoryStore::open(db_path)
        .map_err(|e| format!("inventory store 를 열지 못했다({db_path}): {e}"))?;

    // 영속성 판정을 여기서 다시 만들지 않는다 — 저장소가 이미 안다.
    // (`import-manifest` 에서 같은 판정을 두 곳에 뒀다가 한쪽만 낡았다.)
    if !store.is_durable() {
        return Err(format!(
            "--inventory-db 가 영속이 아니다({db_path:?}) — 반입한 inventory 가 프로세스와 함께 사라지는데 로그에는 저장한 것처럼 찍힌다"
        ));
    }

    let result = store
        .import_bootstrap(&entries)
        .map_err(|e| format!("BOOTSTRAP_REJECTED: {e}"))?;

    Ok(format!(
        "IMPORTED entries={} registered={} inventories_updated={}",
        result.entries, result.registered, result.inventories_updated
    ))
}

// ---------------------------------------------------------------- 문서 형식

/// ★ `deny_unknown_fields` 가 이 파서의 핵심이다. 모르는 필드를 조용히
///   버리면, 운영자가 오타 낸 `secuirty_tier` 가 **누락된 fact** 가 되어
///   scheduler 가 그 노드를 조용히 떨어뜨린다 — 문서에는 적혀 있는데.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapDocument {
    schema_version: u32,
    agents: Vec<AgentDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentDocument {
    registry: RegistryDocument,
    inventory: InventoryDocument,
}

/// ★ 전부 필수다 — `Option` 이 하나도 없다. 저장소는 `None`(관측 안 됨)
///   을 의도적으로 허용하지만 그건 heartbeat 처럼 **부분 관측이 정상인**
///   경로를 위한 계약이다. 운영자 bootstrap 은 관측이 아니라 **선언**
///   이므로 빠진 값은 관측 공백이 아니라 문서 오류다.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryDocument {
    node_id: String,
    device_id: String,
    owner_member_id: String,
    verifying_key_hex: String,
    node_state: String,
    risk_state: String,
    security_tier: String,
    isolation_class: String,
    key_protection: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InventoryDocument {
    inventory_revision: u64,
    observed_at_unix_ms: u64,
    gpus: Vec<GpuDocument>,
    available_cpu_cores: u32,
    available_ram_bytes: u64,
    available_workspace_bytes: u64,
    allowed_workload_classes: Vec<String>,
    third_party_workloads_opt_in: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GpuDocument {
    gpu_id: String,
    model: String,
    healthy: bool,
    available_vram_bytes: u64,
}

impl AgentDocument {
    fn to_bootstrap(&self, now_unix_ms: u64) -> Result<AgentBootstrap, String> {
        let registry = AgentRegistry {
            node_id: self.registry.node_id.clone(),
            device_id: self.registry.device_id.clone(),
            owner_member_id: self.registry.owner_member_id.clone(),
            verifying_key: parse_verifying_key(&self.registry.verifying_key_hex)?,
            node_state: Some(parse_node_state(&self.registry.node_state)?),
            risk_state: Some(parse_risk_state(&self.registry.risk_state)?),
            security_tier: Some(parse_security_tier(&self.registry.security_tier)?),
            isolation_class: Some(parse_isolation_class(&self.registry.isolation_class)?),
            key_protection: Some(parse_key_protection(&self.registry.key_protection)?),
        };

        // ★ 관측 시각이 미래면 거부한다. **관용은 0** 이다 — scheduler 가
        //   이미 그 규칙을 쓴다(`filter.rs`: `observed > evaluated` 면
        //   `SnapshotNotFresh`). 여기서 다른 관용값을 정하면 없는 규범을
        //   지어내는 것이고, 두 계층의 판정이 어긋난다.
        //
        //   ★ scheduler 도 결국 거부하는데 왜 여기서 또 보는가 — **언제
        //     알려 주느냐**가 다르다. 여기서 안 막으면 운영자는 반입이
        //     성공했다고 듣고, 한참 뒤에 "후보가 없다" 만 본다. 시계가
        //     틀렸는지 단위를 잘못 썼는지는 그때 알 수 없다.
        if self.inventory.observed_at_unix_ms > now_unix_ms {
            return Err(format!(
                "observed_at_unix_ms 가 미래다({} > 지금 {now_unix_ms}) — 시계가 어긋났거나 단위가 틀렸다",
                self.inventory.observed_at_unix_ms
            ));
        }

        let mut workloads = BTreeSet::new();
        for name in &self.inventory.allowed_workload_classes {
            if !workloads.insert(parse_workload_class(name)?) {
                return Err(format!("allowed_workload_classes 에 {name} 이 두 번 있다"));
            }
        }

        let inventory = AgentInventory {
            // registry 와 같은 값을 쓴다 — 문서가 두 번 말하게 하면 서로
            // 어긋날 수 있고, 그 어긋남은 저장소가 아니라 **문서**의
            // 결함인데 저장소 오류로 나타난다.
            node_id: self.registry.node_id.clone(),
            inventory_revision: self.inventory.inventory_revision,
            observed_at_unix_ms: self.inventory.observed_at_unix_ms,
            gpus: Some(
                self.inventory
                    .gpus
                    .iter()
                    .map(|gpu| GpuInventory {
                        gpu_id: gpu.gpu_id.clone(),
                        model: Some(gpu.model.clone()),
                        healthy: Some(gpu.healthy),
                        available_vram_bytes: Some(gpu.available_vram_bytes),
                    })
                    .collect(),
            ),
            available_cpu_cores: Some(self.inventory.available_cpu_cores),
            available_ram_bytes: Some(self.inventory.available_ram_bytes),
            available_workspace_bytes: Some(self.inventory.available_workspace_bytes),
            allowed_workload_classes: Some(workloads),
            third_party_workloads_opt_in: Some(self.inventory.third_party_workloads_opt_in),
        };
        Ok(AgentBootstrap {
            registry,
            inventory,
        })
    }
}

/// ★ 저장소는 **임의의 32바이트**를 받아들인다. 여기서 좁힌다 — 실제
///   Ed25519 공개키로 해석되지 않는 32바이트를 등록하면, 그 노드가 보낸
///   무엇도 영영 검증되지 않는데 등록은 성공한 것처럼 보인다.
fn parse_verifying_key(hex: &str) -> Result<Vec<u8>, String> {
    if hex.len() != 64 {
        return Err(format!(
            "verifying_key_hex 는 64자리 hex(32바이트)여야 한다(길이 {})",
            hex.len()
        ));
    }
    // ★ 결함 ⑬(2026-09-10) — 바이트로 자르지 않는다. `gputeer_crypto::hex`
    //   가 한 바이트씩 읽으므로 문자 경계를 가를 수 없다.
    let out = gputeer_crypto::hex::decode_fixed::<32>(hex)
        .map_err(|e| format!("verifying_key_hex 파싱 실패: {e}"))?;
    gputeer_crypto::VerifyingKey::from_bytes(&out)
        .map_err(|e| format!("verifying_key_hex 가 Ed25519 공개키가 아니다: {e}"))?;
    Ok(out.to_vec())
}

/// 모르는 이름을 조용히 넘기지 않는다 — 아는 이름을 함께 알려 준다.
fn parse_enum<T: Copy>(field: &str, value: &str, table: &[(&str, T)]) -> Result<T, String> {
    table
        .iter()
        .find(|(name, _)| *name == value)
        .map(|(_, parsed)| *parsed)
        .ok_or_else(|| {
            let known: Vec<&str> = table.iter().map(|(name, _)| *name).collect();
            format!("{field} 가 {value:?} 인데 아는 값은 {known:?} 다")
        })
}

fn parse_node_state(value: &str) -> Result<NodeState, String> {
    parse_enum(
        "node_state",
        value,
        &[
            ("DISCOVERED", NodeState::Discovered),
            ("ENROLLING", NodeState::Enrolling),
            ("ENROLL_REJECTED", NodeState::EnrollRejected),
            ("APPROVED", NodeState::Approved),
            ("ONLINE", NodeState::Online),
            ("SUSPECT", NodeState::Suspect),
            ("UNREACHABLE", NodeState::Unreachable),
            ("LOST", NodeState::Lost),
            ("RECOVERING", NodeState::Recovering),
            ("DRAINING", NodeState::Draining),
            ("OFFLINE", NodeState::Offline),
            ("QUARANTINED", NodeState::Quarantined),
            ("REVOKED", NodeState::Revoked),
            ("TERMINATED", NodeState::Terminated),
        ],
    )
}

fn parse_risk_state(value: &str) -> Result<RiskState, String> {
    parse_enum(
        "risk_state",
        value,
        &[
            ("NORMAL", RiskState::Normal),
            ("SUSPECT", RiskState::Suspect),
            ("QUARANTINED", RiskState::Quarantined),
            ("REVOKED", RiskState::Revoked),
        ],
    )
}

fn parse_security_tier(value: &str) -> Result<SecurityTier, String> {
    parse_enum(
        "security_tier",
        value,
        &[
            ("S0", SecurityTier::S0),
            ("S1", SecurityTier::S1),
            ("S2", SecurityTier::S2),
            ("S3", SecurityTier::S3),
            ("S4", SecurityTier::S4),
            ("S5", SecurityTier::S5),
        ],
    )
}

fn parse_isolation_class(value: &str) -> Result<IsolationClass, String> {
    parse_enum(
        "isolation_class",
        value,
        &[
            ("RESTRICTED", IsolationClass::Restricted),
            ("CONTAINED", IsolationClass::Contained),
            ("VIRTUALIZED", IsolationClass::Virtualized),
        ],
    )
}

fn parse_key_protection(value: &str) -> Result<KeyProtection, String> {
    parse_enum(
        "key_protection",
        value,
        &[
            ("K0", KeyProtection::K0),
            ("K1", KeyProtection::K1),
            ("K2", KeyProtection::K2),
        ],
    )
}

fn parse_workload_class(value: &str) -> Result<WorkloadClass, String> {
    parse_enum(
        "allowed_workload_classes",
        value,
        &[
            ("TRAINING", WorkloadClass::Training),
            ("INFERENCE", WorkloadClass::Inference),
            ("PREPROCESSING", WorkloadClass::Preprocessing),
            ("EVALUATION", WorkloadClass::Evaluation),
            ("RENDERING", WorkloadClass::Rendering),
            ("OTHER", WorkloadClass::Other),
        ],
    )
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as u64
}

fn parse_flags(args: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < args.len() {
        let key = &args[i];
        if !key.starts_with("--") {
            return Err(format!("알 수 없는 인자: {key}"));
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("{key} 에 값이 없다"))?;
        out.insert(key.clone(), value.clone());
        i += 2;
    }
    Ok(out)
}

fn require<'a>(flags: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    flags
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| format!("{key} 가 필요하다"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ **손으로 쓴 표는 낡는다.** 이 저장소에서 세 번 그랬다 —
    ///   `DoD-02`·`DoD-06` 의 domain 배열, `DoD-63` 의 canonical 벡터 배열.
    ///   전부 "새 변형이 생겼는데 배열은 그대로" 였고, 배열로 고쳤더니
    ///   또 낡았다.
    ///
    /// 여기서는 배열을 검사하지 않는다. **exhaustive `match` 를 쓴다** —
    /// scheduler 에 새 변형이 생기면 이 테스트가 **컴파일되지 않는다.**
    /// 통과/실패 이전에 빌드가 멈추므로 표를 안 고치고는 지나갈 수 없다.
    ///
    /// 왜 중요한가: 표에 빠진 변형은 운영자가 **쓸 수 없는 값**이 된다.
    /// 모델이 지원하는 상태를 문서에 적었는데 "아는 값은..." 이라고
    /// 거부당하고, 그게 파서 결함인지 오타인지 운영자는 알 수 없다.

    #[test]
    fn every_node_state_is_expressible_in_a_bootstrap_document() {
        // 이 match 가 exhaustive 다 — 변형이 늘면 컴파일이 깨진다.
        let name = |state: NodeState| match state {
            NodeState::Discovered => "DISCOVERED",
            NodeState::Enrolling => "ENROLLING",
            NodeState::EnrollRejected => "ENROLL_REJECTED",
            NodeState::Approved => "APPROVED",
            NodeState::Online => "ONLINE",
            NodeState::Suspect => "SUSPECT",
            NodeState::Unreachable => "UNREACHABLE",
            NodeState::Lost => "LOST",
            NodeState::Recovering => "RECOVERING",
            NodeState::Draining => "DRAINING",
            NodeState::Offline => "OFFLINE",
            NodeState::Quarantined => "QUARANTINED",
            NodeState::Revoked => "REVOKED",
            NodeState::Terminated => "TERMINATED",
        };
        for state in [
            NodeState::Discovered,
            NodeState::Enrolling,
            NodeState::EnrollRejected,
            NodeState::Approved,
            NodeState::Online,
            NodeState::Suspect,
            NodeState::Unreachable,
            NodeState::Lost,
            NodeState::Recovering,
            NodeState::Draining,
            NodeState::Offline,
            NodeState::Quarantined,
            NodeState::Revoked,
            NodeState::Terminated,
        ] {
            assert_eq!(parse_node_state(name(state)), Ok(state), "{:?}", state);
        }
    }

    #[test]
    fn every_risk_state_is_expressible_in_a_bootstrap_document() {
        let name = |state: RiskState| match state {
            RiskState::Normal => "NORMAL",
            RiskState::Suspect => "SUSPECT",
            RiskState::Quarantined => "QUARANTINED",
            RiskState::Revoked => "REVOKED",
        };
        for state in [
            RiskState::Normal,
            RiskState::Suspect,
            RiskState::Quarantined,
            RiskState::Revoked,
        ] {
            assert_eq!(parse_risk_state(name(state)), Ok(state), "{:?}", state);
        }
    }

    #[test]
    fn every_security_tier_is_expressible_in_a_bootstrap_document() {
        let name = |tier: SecurityTier| match tier {
            SecurityTier::S0 => "S0",
            SecurityTier::S1 => "S1",
            SecurityTier::S2 => "S2",
            SecurityTier::S3 => "S3",
            SecurityTier::S4 => "S4",
            SecurityTier::S5 => "S5",
        };
        for tier in [
            SecurityTier::S0,
            SecurityTier::S1,
            SecurityTier::S2,
            SecurityTier::S3,
            SecurityTier::S4,
            SecurityTier::S5,
        ] {
            assert_eq!(parse_security_tier(name(tier)), Ok(tier), "{:?}", tier);
        }
    }

    #[test]
    fn every_isolation_class_is_expressible_in_a_bootstrap_document() {
        let name = |class: IsolationClass| match class {
            IsolationClass::Restricted => "RESTRICTED",
            IsolationClass::Contained => "CONTAINED",
            IsolationClass::Virtualized => "VIRTUALIZED",
        };
        for class in [
            IsolationClass::Restricted,
            IsolationClass::Contained,
            IsolationClass::Virtualized,
        ] {
            assert_eq!(parse_isolation_class(name(class)), Ok(class), "{:?}", class);
        }
    }

    #[test]
    fn every_key_protection_is_expressible_in_a_bootstrap_document() {
        let name = |protection: KeyProtection| match protection {
            KeyProtection::K0 => "K0",
            KeyProtection::K1 => "K1",
            KeyProtection::K2 => "K2",
        };
        for protection in [KeyProtection::K0, KeyProtection::K1, KeyProtection::K2] {
            assert_eq!(
                parse_key_protection(name(protection)),
                Ok(protection),
                "{:?}",
                protection
            );
        }
    }

    #[test]
    fn every_workload_class_is_expressible_in_a_bootstrap_document() {
        let name = |class: WorkloadClass| match class {
            WorkloadClass::Training => "TRAINING",
            WorkloadClass::Inference => "INFERENCE",
            WorkloadClass::Preprocessing => "PREPROCESSING",
            WorkloadClass::Evaluation => "EVALUATION",
            WorkloadClass::Rendering => "RENDERING",
            WorkloadClass::Other => "OTHER",
        };
        for class in [
            WorkloadClass::Training,
            WorkloadClass::Inference,
            WorkloadClass::Preprocessing,
            WorkloadClass::Evaluation,
            WorkloadClass::Rendering,
            WorkloadClass::Other,
        ] {
            assert_eq!(parse_workload_class(name(class)), Ok(class), "{:?}", class);
        }
    }

    /// 모르는 이름은 **아는 이름을 알려 주며** 거부한다.
    #[test]
    fn an_unknown_enum_name_lists_what_is_known() {
        let error = parse_node_state("ONLIEN").expect_err("오타를 받아들였다");
        assert!(
            error.contains("ONLIEN"),
            "무엇이 틀렸는지 안 말한다: {error}"
        );
        assert!(error.contains("ONLINE"), "아는 값을 안 알려 준다: {error}");
    }

    /// 32바이트지만 Ed25519 공개키가 아닌 것을 거부한다.
    ///
    /// ★ 저장소는 이걸 통과시킨다(임의 32바이트 허용). 여기서 좁히지
    ///   않으면 그 노드가 보낸 무엇도 영영 검증되지 않는데 등록만 성공한다.
    #[test]
    fn a_32_byte_value_that_is_not_a_public_key_is_refused() {
        // ★ 처음에는 `ff` 32바이트가 곡선 밖일 거라고 **가정**했는데
        //   실제로는 복호화된다(테스트가 잡았다). 아래 값은 실행해서
        //   "Cannot decompress Edwards point" 를 실제로 확인한 것이다 —
        //   곡선 위에 없는 바이트열을 손으로 고르면 안 된다.
        let not_on_curve = "11111111111111111111111111111111111111111111111111111111111111bb";
        let error = parse_verifying_key(not_on_curve).expect_err("아무 32바이트나 받았다");
        assert!(
            error.contains("Ed25519 공개키가 아니다"),
            "거부 사유가 다르다: {error}"
        );
        // 대조 — 진짜 공개키는 통과해야 한다. 이게 없으면 "전부 거부" 로도 통과한다.
        let key = gputeer_crypto::SigningKey::from_bytes(&[9u8; 32]);
        let hex: String = key
            .verifying_key()
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert!(parse_verifying_key(&hex).is_ok(), "진짜 공개키를 거부했다");
    }
}
