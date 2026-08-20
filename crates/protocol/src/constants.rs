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

/// 현재 프로토콜 스키마 버전.
pub const SCHEMA_VERSION: u32 = 2;

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
