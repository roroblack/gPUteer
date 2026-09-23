//! 프로토콜 상수 — **단일 출처** (`RULE.md` §3.1).
//!
//! 같은 숫자가 코드 두 곳에 나타나면 그 자체가 결함이다.
//! 이 파일은 공용 파일이며 통합 책임자만 수정한다
//! (`docs/contracts/01_스트림_소유권.md` §3).
//!
//! 값의 근거는 기준선 계획서다. 바꾸려면 변경 제안 절차를 따른다.

/// 단수명 메시지의 시각 허용 오차. 기준선 §25.3.
///
/// **장수명 문서(JobManifest)에는 적용하지 않는다.** 큐 대기가 정상이기 때문이다.
pub const CLOCK_SKEW_TOLERANCE_MS: u64 = 60_000;

/// ExecutionGrant 기본 수명. 기준선 §15.4.
pub const GRANT_TTL_MS: u64 = 60_000;

/// 이웃 신고(`NeighborUnreachableReport`)의 수명.
///
/// ★ **규범이 정한 값이 아니다.** `ADR-033` §7 은 신고 TTL 을 정하지
///   않았다. `GRANT_TTL_MS` 를 그대로 쓰면 "ExecutionGrant 기본 수명"
///   (기준선 §15.4)이 신고에도 적용되는 것처럼 보이는데, 그럴 근거가
///   없다(2026-08-30 독립 검수 지적).
///
///   그래서 **별도 상수로 분리**하고 값의 근거를 여기 적는다 — 관측은
///   짧게 살아야 하고(오래된 신고가 지금 상태로 오인되면 안 된다), 이
///   저장소의 다른 ShortLived 메시지와 같은 시간 규모를 쓰는 것이
///   가장 덜 놀랍다. 풀 정책이 이 값을 정하게 되면 여기가 아니라
///   정책에서 와야 한다.
pub const NEIGHBOR_REPORT_TTL_MS: u64 = 60_000;

/// JobManifest 기본 수명. 기준선 §15.2.
pub const MANIFEST_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

/// JobManifest 최대 수명.
pub const MANIFEST_MAX_TTL_MS: u64 = 30 * 24 * 60 * 60 * 1_000;

/// Lease 기본 수명. 기준선 §20.2.
pub const LEASE_DURATION_MS: u64 = 10 * 60 * 1_000;

/// lease 만료 후 재배치까지의 유예. 네트워크 순단으로 인한 중복 실행 억제.
pub const LEASE_GRACE_MS: u64 = 60_000;

/// lease_id 하나로 누적 가능한 최대 시간.
pub const LEASE_MAX_TOTAL_DURATION_S: u32 = 24 * 60 * 60;

/// CAS 청크 크기. 기준선 §41.1 / signing.md §6.3.
pub const CHUNK_SIZE_BYTES: usize = 4 * 1024 * 1024;

/// VRAM fragmentation 계수 (ppm). 1_200_000 = 1.20배. 기준선 §10.5.
///
/// **부동소수점을 쓰지 않는다** (signing.md §3.1-g).
pub const FRAGMENTATION_FACTOR_PPM: u64 = 1_200_000;

/// nonce 바이트 길이. signing.md §10.
pub const NONCE_LEN: usize = 16;

/// replay 캐시 상한. 초과 시 축출이 아니라 **거부**한다 (signing.md §10).
pub const REPLAY_CACHE_MAX_ENTRIES: usize = 100_000;

/// 이 빌드가 아는 **가장 높은** 메시지 schema_version. `verify()` 는 호출자의 상한을 이 값으로 자른다.
///
/// ★ 2026-09-23 — 2 -> 3. `ExecutionGrant` v3(재개 지점, `docs/contracts/proposals/2026-09-23_1908_Grant_v3_재개_지점.md`)
///   를 넣으면서 올렸다. 올리지 않으면 v3 를 읽으려는 검증이 **패닉**한다(debug) — 장애 이어받기 실측에서 Coordinator 가
///   바로 그렇게 죽었다. 메시지마다의 상한은 따로 있다(`EXECUTION_GRANT_MAX_SCHEMA_VERSION` 등) — 이 값을 올린다고
///   다른 메시지가 v3 를 받게 되지 않는다.
pub const SCHEMA_VERSION: u32 = 3;

/// ★ 단수명 메시지의 **TTL 상한** (독립 검수 2026-08-17).
///
/// # 왜 상한이 필요한가
///
/// replay nonce 의 보존 시한은 `expires_at + skew` 다.
/// `expires_at` 이 아주 먼 미래인 유효 서명이 들어오면
/// 그 nonce 는 **영원히 GC 되지 않는다.**
/// 그런 nonce 를 캐시 상한만큼 만들면 다른 모든 검증이 거부된다.
///
/// # 값의 근거
///
/// 단수명 메시지 중 가장 긴 것은 Lease 다:
/// `LEASE_DURATION_MS`(10분) + `LEASE_GRACE_MS`(1분) = 11분.
/// 여기에 여유를 둬 **15분**으로 잡았다.
///
/// ★ 이것은 측정값이 아니라 **정책값**이다.
///   실제 Lease 갱신 주기를 측정하면 조정해야 할 수 있다.
pub const MAX_SHORTLIVED_TTL_MS: u64 = 15 * 60 * 1_000;

/// ★ ingress 프레임 하나의 최대 크기 (2026-08-17, 코덱스 설계 논의).
///
/// # 왜 상한이 필요한가
///
/// 길이 접두사만 읽고 그만큼 버퍼를 할당하면, 공격자가 거대한 길이 값을
/// 보내는 것만으로 메모리를 소진시킬 수 있다. 상한이 없으면 프레이밍
/// 자체가 DoS 통로다.
///
/// # 값의 근거
///
/// 8 MiB. `CHUNK_SIZE_BYTES`(4MiB)의 2배 — 체크포인트 청크 하나에
/// 도메인 헤더와 서명 오버헤드를 더해도 넉넉하다.
/// 제어 메시지(Grant · Lease 등)는 이보다 훨씬 작다.
///
/// ★ 이것은 측정값이 아니라 정책값이다. 실제 최대 청크 크기를 재면
///   조정해야 할 수 있다.
pub const MAX_INGRESS_FRAME_BYTES: u32 = 8 * 1024 * 1024;

/// `AgentSessionHello.mode` — 다중 Agent Grant lane.
///
/// # 왜 여기 있는가
///
/// ★ 2026-08-30 독립 검수 지적. 이 값이 `crates/agent` 안에만 있어서
///   **Coordinator 는 mode 를 아예 안 봤다.** 등록된 Agent 가 Resume
///   모드(`2`)로 서명한 Hello 를 보내도 다중 Agent Grant 를 받았다 —
///   서명은 유효하므로 아무것도 걸러내지 못한다.
///
/// 보내는 쪽만 상수를 알고 받는 쪽은 모르면, 그 필드는 있으나 마나다.
/// `CLAUDE.md` §3 이 "프로토콜 상수는 한 곳에만 둔다" 고 정한 것과
/// 같은 이유로 여기 둔다.
pub const MODE_MULTI_AGENT_GRANT: i32 = 1;

/// `AgentSessionHello.mode` — Resume lane(`DoD-36`).
///
/// 다중 Agent lane 과 **반드시 달라야 한다.** 같으면 한쪽 lane 용으로
/// 서명된 Hello 를 다른 lane 이 자기 것으로 받아들인다.
pub const MODE_RESUME: i32 = 2;

/// `AgentSessionHello.mode` — 새 연결로 Lease 갱신만 한다(B+E 계약 단계 1).
pub const MODE_RENEW: i32 = 3;

/// `AgentSessionHello.mode` — 새 연결로 종료 보고만 한다(B+E 계약 단계 1).
pub const MODE_REPORT: i32 = 4;

/// `AttemptReport` 가 쓰는 가장 높은 schema_version — 2 부터 종료 관측 · 확정 실패 단계 필드가 있다(B+E 계약 단계 1).
/// ★ 소비 경로(Coordinator 의 AttemptReport 읽기 등)는 이 값을 지원 버전으로 넘긴다 — 숫자를 경로마다 적지 않는다.
pub const ATTEMPT_REPORT_MAX_SCHEMA_VERSION: u32 = 2;

/// `ExecutionGrant` 를 받는 쪽이 읽는 최대 schema_version.
///
/// ★ 2026-09-23 (신뢰망 남은 일 F) — 3 은 재개 지점(`resume_from = 26`)을 실은 Grant 다. 재개 지점이 없으면
///   발급자는 계속 2 를 쓴다 — 재개가 필요 없는 작업은 구버전 Agent 도 받는다.
pub const EXECUTION_GRANT_MAX_SCHEMA_VERSION: u32 = 3;
