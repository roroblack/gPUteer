//! `gputeer import-inventory` — **거부 경로와 원자성이 이 조각의 값어치다.**
//!
//! 정상 반입이 되는 것보다, 문서 한 항목이 틀렸을 때 앞 항목이 남지
//! 않는지가 중요하다. "거부했다" 만 보면 절반쯤 반입해 놓고 거부라고
//! 보고하는 구현도 통과한다.
//!
//! ★ 그리고 이 조각은 **끝까지** 간다 — 반입한 것이 실제로 scheduler 가
//!   고를 수 있는 후보가 되는지까지 확인한다. 저장했다는 것과 쓸 수 있다는
//!   것은 다르다.

use std::path::{Path, PathBuf};
use std::process::Command;

use gputeer_coordinator::inventory_store::CoordinatorInventoryStore;

fn cli_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join(if cfg!(windows) {
        "gputeer.exe"
    } else {
        "gputeer"
    })
}

const OBSERVED_AT: u64 = 1_700_000_000_000;

/// 실제 Ed25519 공개키의 hex — 지어낸 32바이트를 쓰면 파서가 거부한다.
fn public_key_hex(seed: u8) -> String {
    gputeer_crypto::SigningKey::from_bytes(&[seed; 32])
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 완전한 노드 한 벌. `tweak` 으로 한 군데만 망가뜨려 반례를 만든다.
fn agent_json(node: &str, seed: u8, revision: u64) -> String {
    format!(
        r#"{{
      "registry": {{
        "node_id": "{node}",
        "device_id": "device-{seed}",
        "owner_member_id": "owner-{seed}",
        "verifying_key_hex": "{key}",
        "node_state": "ONLINE",
        "risk_state": "NORMAL",
        "security_tier": "S2",
        "isolation_class": "CONTAINED",
        "key_protection": "K1"
      }},
      "inventory": {{
        "inventory_revision": {revision},
        "observed_at_unix_ms": {OBSERVED_AT},
        "gpus": [
          {{
            "gpu_id": "{node}-gpu-0",
            "model": "RTX 4070 SUPER",
            "healthy": true,
            "available_vram_bytes": 12884901888
          }}
        ],
        "available_cpu_cores": 16,
        "available_ram_bytes": 34359738368,
        "available_workspace_bytes": 107374182400,
        "allowed_workload_classes": ["TRAINING", "EVALUATION"],
        "third_party_workloads_opt_in": true
      }}
    }}"#,
        key = public_key_hex(seed)
    )
}

fn document(agents: &[String]) -> String {
    format!(
        "{{\n  \"schema_version\": 1,\n  \"agents\": [{}]\n}}",
        agents.join(",")
    )
}

fn write_doc(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("문서 쓰기");
    path
}

struct Run {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn import(doc: &Path, db: &Path) -> Run {
    let out = Command::new(cli_bin())
        .args([
            "import-inventory",
            "--inventory",
            doc.to_str().expect("경로"),
            "--inventory-db",
            db.to_str().expect("경로"),
        ])
        .output()
        .expect("import-inventory 실행");
    Run {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// DB 파일 전체의 상태. 없으면 `None`.
fn db_snapshot(db: &Path) -> Option<Vec<u8>> {
    if !db.exists() {
        return None;
    }
    Some(std::fs::read(db).expect("db 읽기"))
}

/// 반입된 노드 이름들 — 새 store 로 파일을 **다시 열어** 읽는다.
fn nodes_in(db: &Path) -> Vec<String> {
    if !db.exists() {
        return Vec::new();
    }
    let mut store = CoordinatorInventoryStore::open(db).expect("inventory store 열기");
    store
        .pool_snapshot(OBSERVED_AT + 1_000)
        .expect("pool_snapshot")
        .candidates
        .iter()
        .map(|c| c.node_id.clone())
        .collect()
}

// ------------------------------------------------------------------ 정상 경로

/// ★ 이 조각이 실제로 무엇을 만드는가 — 저장이 아니라 **선택 가능한 후보**다.
#[test]
fn an_imported_node_becomes_a_candidate_the_scheduler_actually_selects() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let doc = write_doc(
        dir.path(),
        "bootstrap.json",
        &document(&[agent_json("node-a", 1, 1)]),
    );
    let db = dir.path().join("inventory.sqlite3");

    let run = import(&doc, &db);
    assert!(run.ok, "정상 반입이 실패했다: {}", run.stderr);
    assert!(
        run.stdout.contains("entries=1") && run.stdout.contains("registered=1"),
        "무엇을 했는지 안 말한다: {}",
        run.stdout
    );

    // 다른 프로세스처럼 파일을 다시 연다 — 넣은 것이 실제로 남았는가.
    let mut store = CoordinatorInventoryStore::open(&db).expect("store 열기");
    let pool = store.pool_snapshot(OBSERVED_AT + 1_000).expect("snapshot");

    // ★ 여기서 멈추지 않는다. scheduler 가 이 후보를 **고르는가.**
    // ★ 실제 정의를 읽고 맞춘 것이다 — 처음에는 필드 이름을 지어냈고
    //   컴파일이 거부했다(`CLAUDE.md` §1 "지어내지 않는다").
    let requirements = gputeer_scheduler::JobRequirements {
        submitter_member_id: Some("owner-1".into()),
        workload_class: Some(gputeer_scheduler::WorkloadClass::Training),
        side_effect_class: Some(gputeer_scheduler::SideEffectClass::Pure),
        sensitivity: Some(gputeer_scheduler::Sensitivity::Internal),
        minimum_security_tier: Some(gputeer_scheduler::SecurityTier::S2),
        minimum_isolation_class: Some(gputeer_scheduler::IsolationClass::Contained),
        minimum_key_protection: Some(gputeer_scheduler::KeyProtection::K1),
        minimum_gpu_count: Some(1),
        minimum_vram_bytes_per_gpu: Some(8 * 1024 * 1024 * 1024),
        allowed_gpu_models: Vec::new(),
        cpu_cores: Some(4),
        ram_bytes: Some(8 * 1024 * 1024 * 1024),
        workspace_bytes: Some(1024 * 1024 * 1024),
    };
    let report = gputeer_scheduler::evaluate_eligibility(
        &pool,
        &requirements,
        &gputeer_scheduler::Policy {
            maximum_snapshot_age_ms: 60_000,
            silent_after_ms: None,
        },
    );
    match report.resolution {
        gputeer_scheduler::EligibilityResolution::SingleEligible { ref node_id } => {
            assert_eq!(node_id, "node-a", "엉뚱한 노드를 골랐다");
        }
        ref other => panic!(
            "반입한 노드를 고르지 못했다: {other:?}, 거부 사유={:?}",
            report.rejected
        ),
    }
}

/// 여러 노드가 한 문서로 들어간다.
#[test]
fn several_nodes_import_together() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let doc = write_doc(
        dir.path(),
        "bootstrap.json",
        &document(&[agent_json("node-a", 1, 1), agent_json("node-b", 2, 1)]),
    );
    let db = dir.path().join("inventory.sqlite3");

    let run = import(&doc, &db);
    assert!(run.ok, "정상 반입이 실패했다: {}", run.stderr);
    assert_eq!(nodes_in(&db), vec!["node-a", "node-b"], "후보가 안 생겼다");
}

/// 같은 문서를 다시 반입하면 아무것도 새로 만들지 않는다.
#[test]
fn importing_the_same_document_twice_creates_nothing_new() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let doc = write_doc(
        dir.path(),
        "bootstrap.json",
        &document(&[agent_json("node-a", 1, 1)]),
    );
    let db = dir.path().join("inventory.sqlite3");

    assert!(import(&doc, &db).ok);
    let second = import(&doc, &db);
    assert!(second.ok, "재반입이 실패했다: {}", second.stderr);
    assert!(
        second.stdout.contains("registered=0") && second.stdout.contains("inventories_updated=0"),
        "재반입이 새로 만들었다고 보고한다: {}",
        second.stdout
    );
    assert_eq!(nodes_in(&db), vec!["node-a"], "행이 늘었다");
}

// ------------------------------------------------------------------ 거부 경로

/// ★★ **이 조각의 핵심 주장.** 뒤쪽 항목이 틀리면 앞쪽이 남지 않는다.
///
/// 저장소의 단건 API 두 개를 CLI 에서 이어 붙였다면 이 테스트가 실패한다 —
/// 그 둘은 각자 커밋하므로 앞 노드는 이미 들어가 있다.
#[test]
fn a_bad_entry_leaves_nothing_from_the_earlier_entries() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    // 두 번째 노드가 첫 번째의 device_id 를 다시 쓴다.
    let mut clashing = agent_json("node-b", 2, 1);
    clashing = clashing.replace("\"device-2\"", "\"device-1\"");
    let doc = write_doc(
        dir.path(),
        "bootstrap.json",
        &document(&[agent_json("node-a", 1, 1), clashing]),
    );
    let db = dir.path().join("inventory.sqlite3");
    let before = db_snapshot(&db);

    let run = import(&doc, &db);
    assert!(!run.ok, "device_id 충돌을 받아들였다: {}", run.stdout);
    assert!(
        run.stderr.contains("device_id"),
        "무엇이 충돌했는지 안 말한다: {}",
        run.stderr
    );
    assert!(
        run.stderr.contains("node-b"),
        "어느 노드가 문제인지 안 말한다: {}",
        run.stderr
    );

    // ★ 앞 노드가 남았는가 — 이 조각의 핵심 주장이다.
    //
    //   `nodes_in()` 은 `pool_snapshot()` 을 거치므로 registry 만 남은
    //   경우도 잡는다(inventory 없는 registry 도 후보로 나오되 fact 가
    //   전부 `None` 이다). 단건 조회로 한 번 더 확인한다.
    assert!(
        nodes_in(&db).is_empty(),
        "거부했는데 앞 노드가 남았다: {:?}",
        nodes_in(&db)
    );
    {
        let store = CoordinatorInventoryStore::open(&db).expect("store 열기");
        assert!(
            store.get_agent("node-a").expect("조회").is_none(),
            "registry 가 남았다"
        );
        assert!(
            store.get_inventory("node-a").expect("조회").is_none(),
            "inventory 가 남았다"
        );
    }

    // ★ **바이트 비교는 여기서 하지 않는다.** 처음에는
    //   `db_snapshot(&db) == before || nodes_in(&db).is_empty()` 라고 썼는데,
    //   둘째 항이 바로 위 단언으로 이미 참이라 **`||` 가 바이트 비교를
    //   통째로 무의미하게 만들었다**(독립 검수 2라운드). 그리고 그 비교는
    //   애초에 참이 될 수도 없다 — CLI 가 `CoordinatorInventoryStore::open()`
    //   을 부르는 순간 **스키마가 만들어지므로** 거부해도 파일은 생긴다.
    //
    //   그러니 여기서 잴 수 있는 정직한 주장은 "사용자 행이 하나도 없다"
    //   뿐이다. **채워진 DB 의 바이트 불변**은 별도 테스트가 잰다
    //   (`a_rejection_leaves_an_already_populated_db_untouched`).
    let _ = before;
    assert!(
        db.exists(),
        "거부 뒤 스키마 파일이 없다 — 이 주석의 전제가 바뀌었다면 같이 고쳐라"
    );
}

/// 모르는 필드를 조용히 버리지 않는다.
///
/// ★ 버리면 오타 낸 fact 가 **누락된 fact** 가 되고, scheduler 가 그
///   노드를 조용히 떨어뜨린다 — 문서에는 적혀 있는데.
#[test]
fn an_unknown_field_is_refused_instead_of_silently_dropped() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let typo = agent_json("node-a", 1, 1).replace("\"security_tier\"", "\"secuirty_tier\"");
    let doc = write_doc(dir.path(), "bootstrap.json", &document(&[typo]));
    let db = dir.path().join("inventory.sqlite3");

    let run = import(&doc, &db);
    assert!(!run.ok, "오타 난 필드를 받아들였다: {}", run.stdout);
    assert!(
        run.stderr.contains("secuirty_tier"),
        "어느 필드가 문제인지 안 말한다: {}",
        run.stderr
    );
    assert!(nodes_in(&db).is_empty(), "거부했는데 노드가 남았다");
}

/// 빠진 필드도 거부한다 — 운영자 선언에 "관측 안 됨" 은 없다.
#[test]
fn a_missing_fact_is_refused_rather_than_stored_as_unknown() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let without = agent_json("node-a", 1, 1).replace("\"key_protection\": \"K1\"\n      ", "");
    // 앞 필드의 쉼표가 남아 문법이 깨지지 않도록 정리한다.
    let without = without.replace(
        "\"isolation_class\": \"CONTAINED\",",
        "\"isolation_class\": \"CONTAINED\"",
    );
    let doc = write_doc(dir.path(), "bootstrap.json", &document(&[without]));
    let db = dir.path().join("inventory.sqlite3");

    let run = import(&doc, &db);
    assert!(!run.ok, "빠진 fact 를 받아들였다: {}", run.stdout);
    assert!(
        run.stderr.contains("key_protection"),
        "무엇이 빠졌는지 안 말한다: {}",
        run.stderr
    );
    assert!(nodes_in(&db).is_empty(), "거부했는데 노드가 남았다");
}

/// 모르는 enum 이름은 아는 이름을 알려 주며 거부한다.
#[test]
fn an_unknown_enum_value_is_refused_with_the_known_ones() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let bad = agent_json("node-a", 1, 1).replace("\"ONLINE\"", "\"ONLIEN\"");
    let doc = write_doc(dir.path(), "bootstrap.json", &document(&[bad]));
    let db = dir.path().join("inventory.sqlite3");

    let run = import(&doc, &db);
    assert!(!run.ok, "오타 난 상태를 받아들였다: {}", run.stdout);
    assert!(
        run.stderr.contains("ONLIEN") && run.stderr.contains("ONLINE"),
        "아는 값을 안 알려 준다: {}",
        run.stderr
    );
}

/// 32바이트지만 Ed25519 공개키가 아닌 값을 거부한다.
///
/// ★ 저장소는 이걸 통과시킨다. 여기서 좁히지 않으면 그 노드가 보낸
///   무엇도 영영 검증되지 않는데 등록만 성공한다.
#[test]
fn a_key_that_is_not_on_the_curve_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    // 실행해서 "Cannot decompress Edwards point" 를 실제로 확인한 값이다.
    let bad = agent_json("node-a", 1, 1).replace(
        &public_key_hex(1),
        "11111111111111111111111111111111111111111111111111111111111111bb",
    );
    let doc = write_doc(dir.path(), "bootstrap.json", &document(&[bad]));
    let db = dir.path().join("inventory.sqlite3");

    let run = import(&doc, &db);
    assert!(!run.ok, "곡선 밖 키를 받아들였다: {}", run.stdout);
    assert!(
        run.stderr.contains("Ed25519 공개키가 아니다"),
        "거부 사유가 다르다: {}",
        run.stderr
    );
    assert!(nodes_in(&db).is_empty(), "거부했는데 노드가 남았다");
}

/// 같은 문서에 같은 노드가 두 번 나오면 거부한다 — 정확히 같아도.
#[test]
fn the_same_node_twice_in_one_document_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let doc = write_doc(
        dir.path(),
        "bootstrap.json",
        &document(&[agent_json("node-a", 1, 1), agent_json("node-a", 1, 1)]),
    );
    let db = dir.path().join("inventory.sqlite3");

    let run = import(&doc, &db);
    assert!(!run.ok, "같은 노드를 두 번 받아들였다: {}", run.stdout);
    assert!(
        run.stderr.contains("두 번 나온다"),
        "이유를 안 말한다: {}",
        run.stderr
    );
    assert!(nodes_in(&db).is_empty(), "거부했는데 노드가 남았다");
}

/// 모르는 schema_version 을 통과시키지 않는다.
#[test]
fn an_unsupported_schema_version_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let body = document(&[agent_json("node-a", 1, 1)])
        .replace("\"schema_version\": 1", "\"schema_version\": 2");
    let doc = write_doc(dir.path(), "bootstrap.json", &body);
    let db = dir.path().join("inventory.sqlite3");

    let run = import(&doc, &db);
    assert!(!run.ok, "모르는 판을 받아들였다: {}", run.stdout);
    assert!(
        run.stderr.contains("schema_version"),
        "이유를 안 말한다: {}",
        run.stderr
    );
    assert!(nodes_in(&db).is_empty(), "거부했는데 노드가 남았다");
}

/// 빈 문서를 성공으로 보고하지 않는다.
///
/// ★ 아무것도 안 한 것을 "반입했다" 고 찍으면 운영자는 노드가 들어간
///   줄 안다. 조용한 무시는 실패보다 나쁘다.
#[test]
fn an_empty_document_is_refused_rather_than_reported_as_success() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let doc = write_doc(dir.path(), "bootstrap.json", &document(&[]));
    let db = dir.path().join("inventory.sqlite3");

    let run = import(&doc, &db);
    assert!(!run.ok, "빈 문서를 성공으로 보고했다: {}", run.stdout);
    assert!(
        run.stderr.contains("agents 가 비어 있다"),
        "이유를 안 말한다: {}",
        run.stderr
    );
}

/// 관측 시각이 미래면 거부한다 — 관용은 0 이다.
///
/// ★ scheduler 도 결국 거부한다(`observed > evaluated` -> `SnapshotNotFresh`).
///   그런데 그때는 "후보가 없다" 만 보인다 — 시계가 틀렸는지 단위를 잘못
///   썼는지 운영자가 알 길이 없다. 반입 시점에 말해 주는 것이 값어치다.
#[test]
fn an_observation_from_the_future_is_refused_at_import_time() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let far_future = OBSERVED_AT + 100 * 365 * 24 * 3_600_000;
    let future = agent_json("node-a", 1, 1).replace(
        &format!("\"observed_at_unix_ms\": {OBSERVED_AT}"),
        &format!("\"observed_at_unix_ms\": {far_future}"),
    );
    let doc = write_doc(dir.path(), "bootstrap.json", &document(&[future]));
    let db = dir.path().join("inventory.sqlite3");

    let run = import(&doc, &db);
    assert!(!run.ok, "미래 관측을 받아들였다: {}", run.stdout);
    assert!(
        run.stderr.contains("미래다"),
        "이유를 안 말한다: {}",
        run.stderr
    );
    assert!(nodes_in(&db).is_empty(), "거부했는데 노드가 남았다");

    // 대조 — 과거 관측은 통과해야 한다. 이게 없으면 "전부 거부" 로도 통과한다.
    let ok_doc = write_doc(
        dir.path(),
        "ok.json",
        &document(&[agent_json("node-a", 1, 1)]),
    );
    assert!(import(&ok_doc, &db).ok, "과거 관측을 거부했다");
}

/// 비영속 DB 는 거부한다 — `:memory:` 와 빈 경로 둘 다.
///
/// 넣은 것이 프로세스와 함께 사라지면 넣은 것이 아닌데 로그에는 성공으로
/// 찍힌다. 판정은 저장소에 물어본다(같은 판정을 두 곳에 두지 않는다).
#[test]
fn a_non_durable_inventory_db_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let doc = write_doc(
        dir.path(),
        "bootstrap.json",
        &document(&[agent_json("node-a", 1, 1)]),
    );
    for label in [":memory:", ""] {
        let run = import(&doc, Path::new(label));
        assert!(!run.ok, "{label:?} 를 받아들였다: {}", run.stdout);
        assert!(
            run.stderr.contains("영속이 아니다"),
            "{label:?}: 거부 사유가 무엇인지 말하지 않는다: {}",
            run.stderr
        );
    }
}

/// 이미 노드가 든 DB 에 나쁜 문서를 들이밀어도 그 노드가 그대로 있다.
///
/// ★ `import-manifest` 2라운드 검수가 짚은 것 — 거부 테스트가 전부
///   "DB 가 애초에 없음" 이면 무변경 검사가 사실상 아무것도 안 잰다.
#[test]
fn a_rejection_leaves_an_already_populated_db_untouched() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let db = dir.path().join("inventory.sqlite3");
    let good = write_doc(
        dir.path(),
        "good.json",
        &document(&[agent_json("node-a", 1, 1)]),
    );
    assert!(import(&good, &db).ok, "정상 반입이 실패했다");
    let before = db_snapshot(&db).expect("채운 DB");
    assert!(!before.is_empty(), "채운 DB 가 비어 있다");

    // 새 노드가 기존 노드의 device_id 를 다시 쓴다.
    let clashing = agent_json("node-b", 2, 1).replace("\"device-2\"", "\"device-1\"");
    let bad = write_doc(dir.path(), "bad.json", &document(&[clashing]));
    let run = import(&bad, &db);
    assert!(!run.ok, "충돌을 받아들였다: {}", run.stdout);

    let after = db_snapshot(&db).expect("여전히 있어야 한다");
    assert!(
        before == after,
        "거부가 채워진 DB 를 바꿨다({} -> {} 바이트)",
        before.len(),
        after.len()
    );
    assert_eq!(nodes_in(&db), vec!["node-a"], "기존 노드가 바뀌었다");
}
