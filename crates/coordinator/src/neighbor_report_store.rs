//! 검증된 이웃 신고를 **관측 사실로** 영속화한다 — 판정은 하지 않는다.
//!
//! # 규범이 지목한 자리
//!
//! `ADR-033` §7 이 층을 둘로 나눴다.
//!
//! ```text
//! 관측(신고)   같은 풀의 이웃이 "저 노드에 연락이 안 된다" 고 서명해 보고한다
//! 판정(결정)   Broker 가 그 보고를 **모아** 노드 상태를 정하고 재배정을 결정한다
//! ```
//!
//! 신고 메시지는 `DoD-63` 이 만들었고, 판정 관문은 `DoD-61`
//! (`crates/scheduler/src/reassignment.rs`)이 만들었다. 비어 있던 것은
//! 그 사이 — **"모아" 두는 곳**이다. 신고가 도착해도 남지 않으면 여러
//! 이웃의 관측을 함께 볼 수가 없다.
//!
//! 이 모듈은 그 자리를 채우되 **관측만 남긴다.** **정족수를 세지 않고**,
//! 생존 임계값을 적용하지 않고, "연락 두절" 이라고 결론짓지 않는다 —
//! §7 이 금지한 것이 정확히 그 승격이다.
//!
//! ★ "세지 않는다" 를 문자 그대로 읽으면 틀린다(독립 검수 1라운드 정정)
//!   — 아래 저장 상한을 강제하려고 행 수는 센다. 세지 않는 것은 **판정에
//!   쓰이는 수**, 즉 정족수와 생존 임계값이다.
//!
//! # ★ 이 저장소가 실제로 **닫는** 구멍 하나
//!
//! `DoD-63` 이 열린 채 남긴 것 중 하나가 여기서 닫힌다.
//!
//! > `coordinator_device_id` 는 canonical 에 들어가므로 변조는 막히지만
//! > "다른 Coordinator 로 보낸 신고를 재사용할 수 없다" 는 보장은 아니다 —
//! > 프레이밍 계층은 이 값을 자기 ID 와 대조하지 않는다. **소비자가
//! > 반드시 대조해야 한다.**
//!
//! 이 저장소가 그 **첫 소비자**다. 열 때 자기 Coordinator ID 를 받고,
//! 다른 곳으로 보낸 신고는 거부한다.
//!
//! # ★ 닫지 **못하는** 구멍 — 그리고 왜 그것이 여기서 더 위험한가
//!
//! `DoD-63` 의 다른 구멍은 그대로다 — 서명은 **장치**를 인증할 뿐,
//! 그 장치가 주장한 `reporter_node_id` 의 실제 소유자인지는 인증하지
//! 않는다.
//!
//! 그게 이 저장소에서 특히 중요한 이유는, `ADR-033` §8 조건 3 의 정족수가
//! **기계 수**로 세어지기 때문이다. 장치 하나가 서로 다른 기계 ID 를 N 개
//! 주장하면 행이 N 개 생긴다.
//!
//! 그래서 이 저장소는 **정족수를 세지 않는다.** 행을 돌려줄 뿐이고, 각
//! 행에 `reporter_device_id`(기록 당시 검증됐다고 저장된 장치 ID — 읽기
//! 시점에 재검증한 값이 아니다)를 함께 실어 보낸다 — 호출부가
//! 권위 있는 device→node 결합을 해소하기 전에는 그 행들을 정족수로
//! 세면 안 된다는 사실을 **값으로 보이게** 하기 위해서다.
//!
//! # 왜 이력이 아니라 (신고자, 대상)당 최신 한 행인가
//!
//! `CoordinatorNodeLivenessStore`(`DoD-60`)와 같은 이유다 — 노드가 연락이
//! 안 되는 동안 신고는 주기적으로 계속 온다. 전부 남기면 저장소가 무한히
//! 자라고, 그건 남의 PC 를 채우는 일이다(`CLAUDE.md` §0.5).
//!
//! 판정에 쓰이는 것은 각 이웃의 **마지막 관측**이므로 그것만 남긴다.
//! 행 수는 (신고자 수 × 대상 수)로 묶인다.
//!
//! ★ 지나간 신고 이력이 필요하면 그건 감사 로그(§4)의 일이지 이
//!   저장소의 일이 아니다.
//!
//! # 뒤로 가지 않는다
//!
//! 더 오래된 관측으로 덮어쓰지 않는다. 재전송·지연으로 옛 신고가 늦게
//! 도착할 수 있는데, 그걸로 최신 값을 밀어내면 이미 복구된 노드가 다시
//! 연락 두절로 보인다.
//!
//! # 이 모듈이 하지 않는 것
//!
//! ```text
//! 정족수 계산      세지 않는다. §8 조건 3 은 reassignment.rs 의 관문이 본다
//!                  (저장 상한용 행 수는 세지만 그건 판정이 아니다)
//! 생존 판정        하지 않는다. "연락 안 됨" 은 "죽었다" 가 아니다(§7)
//! 멤버십 해소      못 한다. 신고자가 정당한 이웃인지 확인할 수단이 없다
//! device→node 결합 못 한다. 위 참조
//! 서명 재검증      하지 않는다. load 가 돌려주는 것은 **서명을 재검증하지
//!                  않은 decoded row** 다(protobuf 디코드와 해시·인덱스
//!                  일치는 검사한다) — 판정에 쓰기 전에 그때의 권위 있는
//!                  key directory 로 다시 검증해야 한다(DoD-50~53 계약)
//! 시간 기반 만료    하지 않는다. TTL 보존 정책이 규범에 없다
//! 서명 재검증(읽기)  하지 않는다 — 아래 `StoredNeighborReport` 주의 참조
//! ```

use std::path::Path;

use gputeer_protocol::{canonical::blake3_256, pb, signing::Verified};
use prost::Message;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

/// 한 신고 **장치**가 이 저장소에 남길 수 있는 행 수 상한.
///
/// ★ **규범이 정한 값이 아니다.** `ADR-033` 은 신고 저장 상한을 정하지
///   않았다. 이 값도, 아래 축출 규칙도 `CLAUDE.md` §0.5("남의 PC 를
///   채우지 않는다")를 지키기 위한 **로컬 자원 방어**일 뿐이다.
///
/// 왜 필요한가 — 이 저장소는 신고자가 정당한 이웃인지, 지목된 노드가
/// 실재하는지 **둘 다 확인하지 못한다**(멤버십 해소 부재). 그래서 장치
/// 하나가 기계 ID 와 대상 ID 를 지어내며 행을 계속 만들 수 있다.
///
/// # ★ 이 상한이 실제로 묶는 것 — 그리고 묶지 **못하는** 것
///
/// ```text
/// 묶는다      이 API 로 만든 행에 한해, 장치 하나의 행 수 (<= 64)
/// 못 묶는다   저장소 전체 크기
/// ```
///
/// **서로 다른 장치 ID 를 계속 제시하면 각각 64행씩 늘어난다**(독립 검수
/// 1라운드 지적). 저장소 크기는 **이 DB 에 현재 행을 가진 서로 다른 장치
/// ID 수 × 64** 이고, **그 장치 수에는 이 코드 어디에도 상한이 없다**
/// (2라운드 정정 — 이 저장소는 key directory 를 받지도, 그 크기를 알지도
/// 못한다. 호출마다 다른 directory 로 검증됐거나 키가 교체돼도 여기서는
/// 구분되지 않는다).
///
/// 즉 이 상수는 **장치 하나가 혼자서** 저장소를 채우는 것만 막는다. 진짜
/// 경계는 여기가 아니라 멤버십에 있다.
///
/// ★ 그리고 이건 **이 API 를 통해 만들어진 상태**에만 성립한다(독립 검수
///   8라운드 정정). 외부 쓰기나 구버전 DB 로 한 장치 행이 이미 64 를 넘어
///   있으면 그 상태 자체는 막지 못하고, **다음 새 행 기록 때** 상한 안쪽으로
///   되돌린다 — 그때까지는 넘은 채로 남는다.
///
/// ★ "다음 기록" 이 아니라 "다음 **새 행** 기록" 이다(11라운드 정정) — 상한
///   검사는 새 행을 만들 때만 돈다. 기존 쌍의 갱신만 계속되면 초과 상태는
///   그대로 유지된다.
///
/// ★ 그러므로 이건 **증상 억제이지 해결이 아니다.** 진짜 해결은 멤버십
///   해소로 정당하지 않은 신고자를 아예 받지 않는 것이다.
const MAX_ROWS_PER_REPORTER_DEVICE: i64 = 64;

/// 저장된 이웃 신고 — **관측 사실**이다.
///
/// ★ `Verified<NeighborUnreachableReport>` 가 아니다. 판정에 쓰기 전에
///   그때의 권위 있는 key directory 로 다시 검증해야 한다.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredNeighborReport {
    /// 신고한 **기계**. `ADR-033` §8 조건 3 의 정족수 단위다.
    pub reporter_node_id: String,
    /// 그 신고에 서명했다고 **기록 시점에 검증돼 저장된** 장치.
    ///
    /// ★ **읽기 결과 자체는 미검증이다**(독립 검수 1라운드 정정) — 읽을
    ///   때는 해시와 인덱스 일치만 다시 보고 서명을 재검증하지 않는다.
    ///   "지금 유효한 서명자" 가 아니라 "그때 그렇게 저장된 값" 이다.
    ///
    /// ★ 그리고 "기록 시점에 검증됐다" 는 **이 API 로 쓴 행에만** 성립한다
    ///   (9라운드 정정). 이 모듈은 외부 쓰기·구버전 DB 로 들어온 행이 있을
    ///   수 있다고 스스로 인정하면서 읽기에서 서명을 재검증하지 않는다 —
    ///   그러므로 이 필드는 타입 수준 보장이 아니라 **그 행에 그렇게 적혀
    ///   있다**는 사실일 뿐이다.
    ///
    /// ★ 그리고 이 값과 `reporter_node_id` 의 결합은 **저장 시점에도
    ///   검증되지 않았다.** 장치 하나가 여러 기계 ID 를 주장할 수 있다 —
    ///   정족수를 세기 전에 호출부가 해소해야 한다.
    pub reporter_device_id: String,
    /// 연락이 안 된다고 지목된 노드.
    pub unreachable_node_id: String,
    pub observed_at_unix_ms: u64,
    pub report_hash: [u8; 32],
    /// 서명 포함 원본 몸통.
    pub report: pb::NeighborUnreachableReport,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RecordOutcome {
    /// 새로 남겼거나 더 새로운 관측으로 교체했다.
    Recorded {
        stored: StoredNeighborReport,
        /// ★ 저장 상한 때문에 밀려난 관측들 — 지우기 직전에 읽어 **검증한
        ///   행 전체**를, 오래된 순서로.
        ///
        /// 조용히 버리지 않는다(`CLAUDE.md` §3). ID 만 돌려주면 호출부는
        /// "무엇이" 사라졌는지 알 수 없다 — 지워진 뒤에는 다시 읽을 수도
        /// 없으므로 여기서 전부 넘긴다(독립 검수 3라운드 지적).
        ///
        /// ★ 보통은 0개 또는 1개다. 2개 이상은 이 API 로는 만들 수 없는
        ///   상태(외부 쓰기·구버전 DB 로 한 장치 행이 이미 상한을 넘은 경우)를
        ///   **복구할 때**만 나온다(5라운드 지적).
        ///
        /// ★ `Vec` 이라 비어 있을 때 힙을 잡지 않는다 — `RecordOutcome` 의
        ///   모든 값이 축출 여부와 무관하게 큰 필드를 이고 다니지 않게 한다.
        evicted: Vec<StoredNeighborReport>,
    },
    /// 이미 같거나 더 새로운 관측이 있어 아무것도 바꾸지 않았다.
    ///
    /// ★ 오류가 아니다 — 재전송·지연은 정상이다.
    NotNewer(StoredNeighborReport),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeighborReportCorruption {
    EmptyBody,
    UndecodableBody,
    HashEncoding,
    HashMismatch,
    ObservedAtEncoding,
    ReporterNodeMismatch,
    ReporterDeviceMismatch,
    UnreachableNodeMismatch,
}

#[derive(Debug, PartialEq)]
pub enum NeighborReportStoreError {
    InvalidInput(&'static str),
    /// ★ **이 Coordinator 앞으로 온 신고가 아니다.**
    ///
    /// `DoD-63` 이 "소비자가 반드시 대조해야 한다" 고 남긴 것을 여기서
    /// 대조한다. 대조하지 않으면 A 에게 보낸 신고를 B 가 자기 판정에
    /// 쓸 수 있다.
    AddressedToAnotherCoordinator {
        addressed: String,
        ours: String,
    },
    /// 서명자가 신고 안의 `reporter_device_id` 와 다르다.
    ///
    /// 이 메시지의 `signer_id()` 는 `reporter_device_id` 이므로 정상
    /// 경로에서는 일치한다. 다르면 검증 계층 밖에서 조작된 것이다.
    SignerIsNotTheReporterDevice {
        signer_id: String,
        reporter_device_id: String,
    },
    /// 이 기계에 대한 기존 행 **중 하나라도** 다른 장치의 서명으로 남아 있다.
    ///
    /// ★ 대상이 달라도 걸린다(독립 검수 9라운드 정정) — 정족수 단위가
    ///   기계이므로 기계→장치 결합은 대상과 무관하게 하나여야 한다.
    ///
    /// ★ 그 기계의 행을 **전부** 본다(10라운드 정정) — 한 건만 보면 이미
    ///   섞여 있는 상태를 통과시킨다.
    ///
    /// 어느 쪽이 그 기계의 진짜 장치인지 이 저장소는 모른다 — 추측하면
    /// 조작된 신고가 정당한 신고를 밀어낼 수 있다. fail closed.
    ConflictingReporterDevice {
        reporter_node_id: String,
        stored_device_id: String,
        incoming_device_id: String,
    },
    /// 상한이 찼는데 **들어온 신고가 밀려나야 할 관측들보다 새롭지 않다.**
    ///
    /// 상한에 걸리면 보통은 오래된 관측을 밀어내고 받는다 — 정당한 이웃이
    /// 새 노드를 영영 신고 못 하게 되면 그건 관측 계층의 기능 실패다(독립
    /// 검수 1라운드 지적). 그러나 **더 오래되거나 같은 관측으로 더 새로운
    /// 관측을 밀어내지는 않는다**(이 모듈의 "뒤로 가지 않는다" 와 같은
    /// `<=` 규칙). 그 경우만 거부한다.
    ///
    /// ★ 신고 내용이 틀렸다는 뜻이 **아니다.**
    ReporterDeviceQuotaExhausted {
        reporter_device_id: String,
        stored_rows: i64,
        limit: i64,
        /// ★ **실제 판단 기준**이다 — 밀려날 집합 중 **가장 새로운** 관측의
        ///   시각. 들어온 신고가 이 값보다 새로워야 받아들여진다.
        ///
        /// 정상 경로에서는 밀려날 것이 한 건이라 "가장 오래된 관측" 과 같다.
        /// 상한을 이미 넘은 DB 를 복구할 때는 여러 건이 밀려나므로 둘이
        /// 달라진다 — 그때 "가장 오래된 것" 을 보고하면 **왜 거부됐는지를
        /// 잘못 전한다**(독립 검수 6라운드 지적, `CLAUDE.md` §3).
        blocking_observed_at_unix_ms: u64,
    },
    Corrupt {
        reporter_node_id: String,
        unreachable_node_id: String,
        kind: NeighborReportCorruption,
    },
    Storage(String),
}

/// ★ 오류가 **사실을 정확히** 전하게 한다(`CLAUDE.md` §3).
///
/// 예컨대 손상을 "저장소 장애" 로 읽히게 쓰면 운영자가 디스크를 의심하며
/// 엉뚱한 곳을 본다 — 어느 행의 무엇이 어긋났는지까지 적는다.
impl std::fmt::Display for NeighborReportStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(field) => write!(f, "입력이 유효하지 않다: {field}"),
            Self::AddressedToAnotherCoordinator { addressed, ours } => write!(
                f,
                "이 Coordinator 앞으로 온 신고가 아니다 — 수신자 {addressed}, 우리 {ours}"
            ),
            Self::SignerIsNotTheReporterDevice {
                signer_id,
                reporter_device_id,
            } => write!(
                f,
                "서명자가 신고자 장치와 다르다 — 서명자 {signer_id}, 신고 안의 장치 {reporter_device_id}"
            ),
            Self::ConflictingReporterDevice {
                reporter_node_id,
                stored_device_id,
                incoming_device_id,
            } => write!(
                f,
                "기계 {reporter_node_id} 는 이미 장치 {stored_device_id} 에 묶여 있다 — 들어온 장치 {incoming_device_id}"
            ),
            Self::ReporterDeviceQuotaExhausted {
                reporter_device_id,
                stored_rows,
                limit,
                blocking_observed_at_unix_ms,
            } => write!(
                f,
                "장치 {reporter_device_id} 의 저장 상한({limit})이 찼고(현재 {stored_rows}행)                  들어온 신고가 밀려날 관측({blocking_observed_at_unix_ms})보다 새롭지 않다"
            ),
            // ★ 이 두 값은 **행 키(SQLite 인덱스 열)** 이지 디코드된 신고에서
            //   온 값이 아니다(독립 검수 1라운드 정정) — `ReporterNodeMismatch`
            //   같은 손상은 바로 그 둘이 어긋났다는 뜻이므로, 그냥 "신고자/대상"
            //   이라고 쓰면 어느 쪽 값인지 잘못 전한다.
            Self::Corrupt {
                reporter_node_id,
                unreachable_node_id,
                kind,
            } => write!(
                f,
                "저장된 행이 손상됐다 — 행 키(신고자 {reporter_node_id}, 대상 {unreachable_node_id}), 종류 {kind:?}"
            ),
            Self::Storage(message) => write!(f, "저장소 오류: {message}"),
        }
    }
}

impl std::error::Error for NeighborReportStoreError {}

pub struct CoordinatorNeighborReportStore {
    connection: Connection,
    /// 이 저장소를 소유한 Coordinator. 신고의 수신자와 대조한다.
    coordinator_device_id: String,
}

impl CoordinatorNeighborReportStore {
    /// 저장소를 연다.
    ///
    /// `coordinator_device_id` 는 **이 Coordinator 자신의 ID** 다. 다른
    /// 곳으로 보낸 신고를 거부하는 데 쓴다.
    pub fn open(
        path: impl AsRef<Path>,
        coordinator_device_id: &str,
    ) -> Result<Self, NeighborReportStoreError> {
        if coordinator_device_id.trim().is_empty() {
            return Err(NeighborReportStoreError::InvalidInput(
                "coordinator_device_id",
            ));
        }
        let connection = Connection::open(path).map_err(map_sql_error)?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(map_sql_error)?;
        connection
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS coordinator_neighbor_reports (
                    reporter_node_id TEXT NOT NULL,
                    unreachable_node_id TEXT NOT NULL,
                    reporter_device_id TEXT NOT NULL,
                    observed_at_unix_ms BLOB NOT NULL,
                    report_hash BLOB NOT NULL CHECK(length(report_hash) = 32),
                    report_body BLOB NOT NULL,
                    PRIMARY KEY(reporter_node_id, unreachable_node_id)
                );
                -- ★ 저장 상한을 강제하려면 새 행마다 이 장치의 행을 세고
                --    후보를 모은다. 인덱스가 없으면 그때마다 전체 테이블을
                --    훑는다 — 저장소 전체 크기에는 상한이 없으므로(위
                --    `MAX_ROWS_PER_REPORTER_DEVICE` 주석 참조) 그 비용이
                --    계속 커진다(독립 검수 5라운드 지적).
                CREATE INDEX IF NOT EXISTS coordinator_neighbor_reports_by_device
                    ON coordinator_neighbor_reports(reporter_device_id);
                -- ★ `reports_about()` 은 대상 노드로 조회하는데, 그건 정본 키의
                --    **두 번째** 열이라 단독 조회에 키를 못 쓴다 — 인덱스가
                --    없으면 전체 테이블을 훑는다(독립 검수 10라운드 지적).
                --    이 조회는 §8 판정 재료를 모으는 주 경로다.
                CREATE INDEX IF NOT EXISTS coordinator_neighbor_reports_by_target
                    ON coordinator_neighbor_reports(unreachable_node_id);
                "#,
            )
            .map_err(map_sql_error)?;
        Ok(Self {
            connection,
            coordinator_device_id: coordinator_device_id.to_string(),
        })
    }

    /// `:memory:` 는 durable 이 아니다 — 재시작을 넘지 못한다.
    pub fn is_durable(&self) -> bool {
        matches!(self.connection.path(), Some(path) if !path.is_empty() && path != ":memory:")
    }

    /// 한 노드에 대한 신고들을 **행 그대로** 돌려준다.
    ///
    /// ★ 세지 않는다. 정족수 판정은 `reassignment.rs` 의 관문이 하고,
    ///   그 전에 호출부가 device→node 결합과 멤버십을 해소해야 한다.
    ///
    /// 결정적 순서를 위해 `reporter_node_id` 오름차순으로 돌려준다.
    pub fn reports_about(
        &self,
        unreachable_node_id: &str,
    ) -> Result<Vec<StoredNeighborReport>, NeighborReportStoreError> {
        if unreachable_node_id.trim().is_empty() {
            return Err(NeighborReportStoreError::InvalidInput("unreachable_node_id"));
        }
        let mut statement = self
            .connection
            .prepare(
                "SELECT reporter_node_id, unreachable_node_id, reporter_device_id,
                        observed_at_unix_ms, report_hash, report_body
                 FROM coordinator_neighbor_reports
                 WHERE unreachable_node_id = ?1
                 ORDER BY reporter_node_id ASC",
            )
            .map_err(map_sql_error)?;
        let rows = statement
            .query_map(rusqlite::params![unreachable_node_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                ))
            })
            .map_err(map_sql_error)?;

        let mut out = Vec::new();
        for row in rows {
            out.push(decode_row(row.map_err(map_sql_error)?, &self.coordinator_device_id)?);
        }
        Ok(out)
    }

    /// 한 (신고자, 대상) 쌍의 현재 행.
    pub fn get_report(
        &self,
        reporter_node_id: &str,
        unreachable_node_id: &str,
    ) -> Result<Option<StoredNeighborReport>, NeighborReportStoreError> {
        fetch_report(
            &self.connection,
            reporter_node_id,
            unreachable_node_id,
            &self.coordinator_device_id,
        )
    }

    /// 검증된 신고를 관측 사실로 남긴다.
    ///
    /// ★ 순서를 정확히 적는다(독립 검수 1라운드 정정) — 입력 검증·서명자
    /// 대조·**수신자 대조는 `BEGIN IMMEDIATE` 를 잡기 전에** 한다. 이 셋은
    /// 전부 이 메시지와 이 저장소 자신의 상수만 보므로 DB 상태와 경쟁하지
    /// 않는다. DB 를 보는 것 — 기존 행 조회·소유 장치 대조·신선도 비교·
    /// 저장 상한·축출·쓰기 — 은 **전부 하나의 트랜잭션 안**이고, 중간에
    /// 오류로 빠져나가면 `Transaction` 이 drop 되며 rollback 된다.
    pub fn record_verified_report(
        &mut self,
        verified: &Verified<pb::NeighborUnreachableReport>,
    ) -> Result<RecordOutcome, NeighborReportStoreError> {
        // 어떤 필드도 Verified 관문을 지나기 전에 읽지 않는다.
        let report = verified.get();
        validate_input(report)?;

        let signer_id = verified.signer_id();
        if signer_id != report.reporter_device_id {
            return Err(NeighborReportStoreError::SignerIsNotTheReporterDevice {
                signer_id: signer_id.to_string(),
                reporter_device_id: report.reporter_device_id.clone(),
            });
        }

        // ★ 이 Coordinator 앞으로 온 신고인가 — `DoD-63` 이 남긴 구멍을
        //   여기서 닫는다.
        if report.coordinator_device_id != self.coordinator_device_id {
            return Err(NeighborReportStoreError::AddressedToAnotherCoordinator {
                addressed: report.coordinator_device_id.clone(),
                ours: self.coordinator_device_id.clone(),
            });
        }

        let report_body = report.encode_to_vec();
        let report_hash = blake3_256(&report_body);

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;

        // ★ **이 기계에 대한 모든 기존 행**을 검증해 읽고, 하나라도 다른
        //   장치의 것이면 거부한다.
        //
        //   `(기계, 대상)` 쌍이 같은 행에만 검사를 걸면, 같은 기계 ID 를
        //   **다른 대상으로** 신고해서 두 장치가 그 기계를 동시에 주장할 수
        //   있다(독립 검수 9라운드가 잡은 결함) — `ADR-033` §8 조건 3 의
        //   정족수 단위가 **기계**이므로 결합은 기계 단위로 지켜야 한다.
        //
        // ★ 그리고 `LIMIT 1` 로는 부족하다(10라운드 지적) — 구버전 구현이나
        //   외부 쓰기로 한 기계에 장치 A·B 행이 함께 있으면, 정렬상 첫 행과
        //   같은 장치의 신고가 다른 충돌 행을 남긴 채 통과한다. "어떤 행이든"
        //   을 주장하려면 전부 봐야 한다.
        //
        // ★ 또 **검증해서 읽어야 한다**(10라운드 지적) — 컬럼과 몸통이
        //   어긋난 손상 행을 검증 없이 읽으면, 정당한 장치의 재보고에서
        //   그 손상이 `ConflictingReporterDevice` 로 **가려진다**. 손상은
        //   손상으로 보고해야 한다(`CLAUDE.md` §3).
        //
        //   `reporter_node_id` 는 정본 키의 첫 성분이라 이 조회는 그 키를 탄다.
        let machine_keys: Vec<String> = {
            let mut statement = transaction
                .prepare(
                    "SELECT unreachable_node_id
                     FROM coordinator_neighbor_reports
                     WHERE reporter_node_id = ?1
                     ORDER BY unreachable_node_id ASC",
                )
                .map_err(map_sql_error)?;
            let rows = statement
                .query_map(rusqlite::params![report.reporter_node_id], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(map_sql_error)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(map_sql_error)?);
            }
            out
        };

        let mut existing = None;
        for target in &machine_keys {
            let row = fetch_report(
                &transaction,
                &report.reporter_node_id,
                target,
                &self.coordinator_device_id,
            )?
            .ok_or_else(|| {
                NeighborReportStoreError::Storage(
                    "이 기계의 기존 행을 다시 읽지 못했다".to_string(),
                )
            })?;
            if row.reporter_device_id != report.reporter_device_id {
                return Err(NeighborReportStoreError::ConflictingReporterDevice {
                    reporter_node_id: report.reporter_node_id.clone(),
                    stored_device_id: row.reporter_device_id,
                    incoming_device_id: report.reporter_device_id.clone(),
                });
            }
            if *target == report.unreachable_node_id {
                existing = Some(row);
            }
        }
        if let Some(existing) = existing.clone() {
            // ★ 뒤로 가지 않는다.
            if report.observed_at_unix_ms <= existing.observed_at_unix_ms {
                transaction.commit().map_err(map_sql_error)?;
                return Ok(RecordOutcome::NotNewer(existing));
            }
        }

        // ★ 새 행을 만들 때만 상한을 본다 — 기존 행의 갱신은 저장소를
        //   키우지 않으므로 막을 이유가 없다.
        let mut evicted = Vec::new();
        if existing.is_none() {
            let stored_rows: i64 = transaction
                .query_row(
                    "SELECT COUNT(*) FROM coordinator_neighbor_reports
                     WHERE reporter_device_id = ?1",
                    rusqlite::params![report.reporter_device_id],
                    |row| row.get(0),
                )
                .map_err(map_sql_error)?;
            if stored_rows >= MAX_ROWS_PER_REPORTER_DEVICE {
                // 이 장치의 오래된 관측부터 **상한 안쪽이 될 만큼** 찾는다.
                //
                // ★ 정상 경로에서는 한 행이다. 여러 행이 되는 것은 이 API 로는
                //   만들 수 없는 상태(외부 쓰기·구버전 DB)를 복구할 때뿐이다 —
                //   아래 `to_evict` 계산 참조(독립 검수 7라운드 정정: 여기서
                //   "한 행" 이라고만 쓰면 아래 설명과 모순된다).
                //
                // ★ 행의 정본 키는 `(reporter_node_id, unreachable_node_id)` 다
                //   — 대상 ID 만으로는 행이 하나로 정해지지 않는다. 한 장치가
                //   여러 기계 ID 를 주장할 수 있고(이 저장소는 그걸 막지
                //   못한다), 그러면 같은 대상에 대해 서로 다른 신고자 행이
                //   동시에 존재한다. 조회·삭제 키에서 `reporter_node_id` 를
                //   빼면 **여러 행이 한꺼번에 지워진다**(독립 검수 2라운드가
                //   실제로 잡은 결함).
                //
                // ★ **후보를 SQL 정렬로 고르지 않는다**(독립 검수 4라운드가
                //   잡은 결함). `observed_at_unix_ms` 는 고정폭 big-endian 이라
                //   정상 값이면 BLOB 사전순이 수치 순서와 같지만, 그 컬럼이
                //   **손상되면** 실제로 가장 오래된 행이 정렬에서 뒤로 숨는다.
                //   그러면 멀쩡한 행이 대신 밀려나고 손상된 행은 남는다 —
                //   "손상을 통과시키지 않는다" 는 이 모듈의 계약이 정확히
                //   여기서 깨진다.
                //
                //   그래서 이 장치의 행을 **전부 검증해 읽은 뒤** 그중에서
                //   오래된 것부터 고른다. 어느 한 행이라도 손상됐으면 축출하지
                //   않고 거부한다.
                //
                // ★ 비용에 대해 정확히 적는다(5라운드 정정) — 이 경로는 상한에
                //   닿았을 때만 돌고, 읽는 행 수는 **그 장치의 행 수**로
                //   묶인다. 저장소 전체 크기는 묶이지 않으므로
                //   `reporter_device_id` 인덱스가 없으면 조회가 전체 테이블을
                //   훑는다 — 그래서 `open()` 이 그 인덱스를 만든다.
                let keys: Vec<(String, String)> = {
                    let mut statement = transaction
                        .prepare(
                            "SELECT reporter_node_id, unreachable_node_id
                             FROM coordinator_neighbor_reports
                             WHERE reporter_device_id = ?1",
                        )
                        .map_err(map_sql_error)?;
                    let rows = statement
                        .query_map(rusqlite::params![report.reporter_device_id], |row| {
                            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                        })
                        .map_err(map_sql_error)?;
                    let mut out = Vec::new();
                    for row in rows {
                        out.push(row.map_err(map_sql_error)?);
                    }
                    out
                };

                let mut candidates = Vec::with_capacity(keys.len());
                for (candidate_reporter, candidate_target) in keys {
                    let row = fetch_report(
                        &transaction,
                        &candidate_reporter,
                        &candidate_target,
                        &self.coordinator_device_id,
                    )?
                    .ok_or_else(|| {
                        NeighborReportStoreError::Storage(
                            "축출 후보를 다시 읽지 못했다".to_string(),
                        )
                    })?;
                    candidates.push(row);
                }

                // 동점은 정본 키 두 성분으로 갈라 결정적으로 순서를 정한다.
                candidates.sort_by(|a, b| {
                    a.observed_at_unix_ms
                        .cmp(&b.observed_at_unix_ms)
                        .then_with(|| a.reporter_node_id.cmp(&b.reporter_node_id))
                        .then_with(|| a.unreachable_node_id.cmp(&b.unreachable_node_id))
                });

                // ★ 몇 개를 밀어내야 상한 **안쪽**이 되는가.
                //
                //   정상 경로에서는 정확히 1이다(`stored_rows == 64`). 2 이상은
                //   이 API 로는 만들 수 없는 상태 — 외부 쓰기나 구버전 DB 로
                //   한 장치 행이 이미 상한을 넘은 경우 — 를 **복구**할 때만
                //   나온다. 한 건만 밀어내면 그 DB 는 영영 상한을 넘은 채로
                //   남는다(독립 검수 5라운드 지적).
                let over = stored_rows - MAX_ROWS_PER_REPORTER_DEVICE + 1;
                let to_evict = usize::try_from(over).map_err(|_| {
                    NeighborReportStoreError::Storage(
                        "축출 개수 계산이 범위를 벗어났다".to_string(),
                    )
                })?;
                if to_evict == 0 || to_evict > candidates.len() {
                    // 셈과 조회가 어긋났다.
                    return Err(NeighborReportStoreError::Storage(format!(
                        "저장 상한 계산과 실제 행이 어긋났다                          (셈 {stored_rows}, 읽은 행 {}, 밀어낼 수 {to_evict})",
                        candidates.len()
                    )));
                }
                let doomed: Vec<StoredNeighborReport> =
                    candidates.into_iter().take(to_evict).collect();

                // ★ 더 오래되거나 **같은** 관측으로 기존 관측을 밀어내지
                //   않는다 — 이 모듈의 다른 신선도 경계와 같은 `<=` 규칙이다.
                //   밀려날 것 중 **가장 새로운 것**과 비교해야 한다.
                let newest_doomed = doomed
                    .last()
                    .expect("to_evict >= 1 이므로 비어 있지 않다")
                    .observed_at_unix_ms;
                if report.observed_at_unix_ms <= newest_doomed {
                    return Err(NeighborReportStoreError::ReporterDeviceQuotaExhausted {
                        reporter_device_id: report.reporter_device_id.clone(),
                        stored_rows,
                        limit: MAX_ROWS_PER_REPORTER_DEVICE,
                        // ★ 비교에 실제로 쓴 값을 그대로 보고한다.
                        blocking_observed_at_unix_ms: newest_doomed,
                    });
                }

                for victim in &doomed {
                    let deleted = transaction
                        .execute(
                            "DELETE FROM coordinator_neighbor_reports
                             WHERE reporter_node_id = ?1
                               AND unreachable_node_id = ?2
                               AND reporter_device_id = ?3",
                            rusqlite::params![
                                victim.reporter_node_id,
                                victim.unreachable_node_id,
                                report.reporter_device_id
                            ],
                        )
                        .map_err(map_sql_error)?;
                    // 정본 키로 지웠으므로 정확히 한 행이어야 한다.
                    if deleted != 1 {
                        return Err(NeighborReportStoreError::Storage(format!(
                            "축출이 {deleted} 행을 지웠다 — 정확히 1 이어야 한다"
                        )));
                    }
                }
                evicted = doomed;


            }
        }

        transaction
            .execute(
                "INSERT INTO coordinator_neighbor_reports(
                    reporter_node_id, unreachable_node_id, reporter_device_id,
                    observed_at_unix_ms, report_hash, report_body
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(reporter_node_id, unreachable_node_id) DO UPDATE SET
                    reporter_device_id = excluded.reporter_device_id,
                    observed_at_unix_ms = excluded.observed_at_unix_ms,
                    report_hash = excluded.report_hash,
                    report_body = excluded.report_body",
                rusqlite::params![
                    report.reporter_node_id,
                    report.unreachable_node_id,
                    report.reporter_device_id,
                    encode_u64(report.observed_at_unix_ms),
                    report_hash.as_slice(),
                    report_body,
                ],
            )
            .map_err(map_sql_error)?;
        transaction.commit().map_err(map_sql_error)?;

        Ok(RecordOutcome::Recorded {
            stored: StoredNeighborReport {
                reporter_node_id: report.reporter_node_id.clone(),
                reporter_device_id: report.reporter_device_id.clone(),
                unreachable_node_id: report.unreachable_node_id.clone(),
                observed_at_unix_ms: report.observed_at_unix_ms,
                report_hash,
                report: report.clone(),
            },
            evicted,
        })
    }
}

fn validate_input(
    report: &pb::NeighborUnreachableReport,
) -> Result<(), NeighborReportStoreError> {
    if report.reporter_node_id.trim().is_empty() {
        return Err(NeighborReportStoreError::InvalidInput("reporter_node_id"));
    }
    if report.reporter_device_id.trim().is_empty() {
        return Err(NeighborReportStoreError::InvalidInput("reporter_device_id"));
    }
    if report.unreachable_node_id.trim().is_empty() {
        return Err(NeighborReportStoreError::InvalidInput(
            "unreachable_node_id",
        ));
    }
    if report.coordinator_device_id.trim().is_empty() {
        return Err(NeighborReportStoreError::InvalidInput(
            "coordinator_device_id",
        ));
    }
    // ★ 자기 자신을 연락 두절이라고 신고하는 것은 관측이 아니다 — 신고자가
    //   살아서 신고를 보냈다는 사실과 모순된다.
    if report.reporter_node_id == report.unreachable_node_id {
        return Err(NeighborReportStoreError::InvalidInput(
            "reporter_node_id == unreachable_node_id",
        ));
    }
    Ok(())
}

fn fetch_report(
    connection: &Connection,
    reporter_node_id: &str,
    unreachable_node_id: &str,
    expected_coordinator_device_id: &str,
) -> Result<Option<StoredNeighborReport>, NeighborReportStoreError> {
    let raw = connection
        .query_row(
            "SELECT reporter_node_id, unreachable_node_id, reporter_device_id,
                    observed_at_unix_ms, report_hash, report_body
             FROM coordinator_neighbor_reports
             WHERE reporter_node_id = ?1 AND unreachable_node_id = ?2",
            rusqlite::params![reporter_node_id, unreachable_node_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                ))
            },
        )
        .optional()
        .map_err(map_sql_error)?;
    match raw {
        None => Ok(None),
        Some(row) => Ok(Some(decode_row(row, expected_coordinator_device_id)?)),
    }
}

/// 저장된 행을 읽으면서 **손상까지 다시 본다.**
///
/// 몸통에서 디코드한 값이 인덱스 컬럼과 어긋나면 조용히 넘기지 않는다 —
/// 넘기면 어느 쪽이 진실인지 모른 채 판정에 들어간다.
type RawRow = (String, String, String, Vec<u8>, Vec<u8>, Vec<u8>);

fn decode_row(
    row: RawRow,
    expected_coordinator_device_id: &str,
) -> Result<StoredNeighborReport, NeighborReportStoreError> {
    let (reporter_node_id, unreachable_node_id, reporter_device_id, observed_at, hash, body) = row;
    let corrupt = |kind| NeighborReportStoreError::Corrupt {
        reporter_node_id: reporter_node_id.clone(),
        unreachable_node_id: unreachable_node_id.clone(),
        kind,
    };

    if body.is_empty() {
        return Err(corrupt(NeighborReportCorruption::EmptyBody));
    }
    let report_hash: [u8; 32] = hash
        .try_into()
        .map_err(|_| corrupt(NeighborReportCorruption::HashEncoding))?;
    if blake3_256(&body) != report_hash {
        return Err(corrupt(NeighborReportCorruption::HashMismatch));
    }
    let report = pb::NeighborUnreachableReport::decode(body.as_slice())
        .map_err(|_| corrupt(NeighborReportCorruption::UndecodableBody))?;
    let observed_at_unix_ms =
        decode_u64(&observed_at).map_err(|_| corrupt(NeighborReportCorruption::ObservedAtEncoding))?;

    if report.reporter_node_id != reporter_node_id {
        return Err(corrupt(NeighborReportCorruption::ReporterNodeMismatch));
    }
    if report.reporter_device_id != reporter_device_id {
        return Err(corrupt(NeighborReportCorruption::ReporterDeviceMismatch));
    }
    if report.unreachable_node_id != unreachable_node_id {
        return Err(corrupt(NeighborReportCorruption::UnreachableNodeMismatch));
    }
    if report.observed_at_unix_ms != observed_at_unix_ms {
        return Err(corrupt(NeighborReportCorruption::ObservedAtEncoding));
    }

    // ★ **읽을 때도 수신자를 대조한다.** 쓰기 경로의 검사만으로는
    //   부족하다 — 이미 행이 들어 있는 파일을 다른 Coordinator 가 열면
    //   남에게 보낸 신고를 자기 것으로 읽게 된다(파일 복사·이관·설정
    //   실수). 검사 지점이 하나뿐이면 그 지점을 지나지 않는 경로가
    //   생기는 순간 방어가 사라진다.
    if report.coordinator_device_id != expected_coordinator_device_id {
        return Err(NeighborReportStoreError::AddressedToAnotherCoordinator {
            addressed: report.coordinator_device_id.clone(),
            ours: expected_coordinator_device_id.to_string(),
        });
    }

    Ok(StoredNeighborReport {
        reporter_node_id,
        reporter_device_id,
        unreachable_node_id,
        observed_at_unix_ms,
        report_hash,
        report,
    })
}

fn encode_u64(value: u64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn decode_u64(bytes: &[u8]) -> Result<u64, ()> {
    let bytes: [u8; 8] = bytes.try_into().map_err(|_| ())?;
    Ok(u64::from_be_bytes(bytes))
}

fn map_sql_error(error: rusqlite::Error) -> NeighborReportStoreError {
    NeighborReportStoreError::Storage(error.to_string())
}
