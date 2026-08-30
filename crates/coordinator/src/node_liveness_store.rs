//! 검증된 `NodeHeartbeat` 를 노드별 최신 생존 사실로 영속화한다.
//!
//! # 규범이 지목한 공백을 채운다
//!
//! `ADR-033` §7 이 이렇게 적어 뒀다.
//!
//! > 지금 저장소에는 노드가 살아 있는지 판정할 재료가 없다.
//! > `proto/control.proto:508` 에 `NodeRecord.last_heartbeat_unix_ms`
//! > 필드는 있지만 **그 값을 채우는 메시지도 관측자도 없다.**
//!
//! 메시지는 이제 있고(`NodeHeartbeat`), 관측자도 있다(Coordinator 의
//! heartbeat 수신부). 없던 것은 그 관측을 **재시작 너머로 남기는 곳**
//! 이다. 이 모듈이 그 자리다.
//!
//! # 왜 이력이 아니라 "노드별 최신" 인가
//!
//! ★ `CoordinatorReplicaAckStore`(`DoD-53`)는 관측마다 행을 남긴다 —
//!   거기서는 누가 언제 무엇을 봤는지가 전부 증거이기 때문이다.
//!
//! heartbeat 는 다르다. 판정에 쓰이는 것은 **마지막 소식 하나**이고
//! (`classify_node_liveness` 가 정확히 그렇게 고른다), 노드가 살아 있는
//! 동안 몇 초마다 하나씩 쌓이면 저장소가 무한히 자란다. 보존 정책을
//! 정하지 않은 채 무한 이력을 만드는 것은 남의 PC 를 채우는 일이다.
//!
//! 그래서 노드당 한 행을 유지한다. 지나간 heartbeat 를 보고 싶으면
//! 그건 감사 로그의 일이지 이 저장소의 일이 아니다.
//!
//! # 뒤로 가지 않는다
//!
//! ★ 더 오래된 heartbeat 로 **덮어쓰지 않는다.** 재전송·경로 지연으로
//!   옛 관측이 늦게 도착할 수 있는데, 그걸로 최신 값을 밀어내면
//!   살아 있는 노드가 갑자기 조용해 보인다. `issued_at_unix_ms` 가
//!   같거나 더 낮으면 저장하지 않고 기존 행을 그대로 돌려준다.
//!
//! # 이 모듈이 하지 않는 것
//!
//! ```text
//! 생존 판정        classify_node_liveness() 가 한다. 여기는 사실만 남긴다
//! 사망 선언        ADR-033 §7 — "연락 안 됨" 은 "죽었다" 가 아니다
//! 재배정           §8 의 여섯 조건이 필요하다
//! 서명 재검증      load 는 raw 를 돌려준다. 쓰기 전에 그때의 권위 있는
//!                  key directory 로 다시 검증해야 한다(DoD-50~53 과 같은 계약)
//! 이웃 신고 취합   §7 의 신고 메시지는 아직 없다
//! ```

use std::path::Path;

use gputeer_protocol::{canonical::blake3_256, pb, signing::Verified};
use prost::Message;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

/// 저장된 생존 사실.
///
/// ★ `Verified<NodeHeartbeat>` 가 아니다. 호출부는 이 값을 판정에 쓰기
///   전에 **그때의 권위 있는 key directory 로 다시 검증해야 한다** —
///   `DoD-50`~`DoD-53` 이 세운 것과 같은 계약이다. 서명이 저장 시점에
///   유효했다는 것이 지금도 유효하다는 뜻은 아니다(키가 revoke 됐을 수
///   있다).
#[derive(Debug, Clone, PartialEq)]
pub struct StoredNodeLiveness {
    pub heartbeat: pb::NodeHeartbeat,
    pub heartbeat_hash: [u8; 32],
    /// 저장 시점에 서명을 검증한 신원.
    pub signer_id_at_observation: String,
    pub node_id: String,
    pub device_id: String,
    pub last_heartbeat_unix_ms: u64,
    pub fence_epoch: u64,
    pub running_attempts: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObserveResult {
    pub stored: StoredNodeLiveness,
    /// `false` 면 더 오래되거나 같은 관측이라 저장하지 않고 기존 행을
    /// 돌려줬다는 뜻이다.
    pub advanced: bool,
}

/// 저장된 행이 규범을 벗어난 방식.
///
/// ★ 조용히 고치지 않는다. 손상된 사실 위에서 생존을 판정하면 그
///   판정이 틀렸다는 것을 아무도 모른다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivenessCorruption {
    EmptyBody,
    UndecodableBody,
    HashEncoding,
    HashMismatch,
    NodeIdMismatch,
    DeviceIdMismatch,
    IssuedAtMismatch,
    FenceEpochMismatch,
    RunningAttemptsMismatch,
}

#[derive(Debug)]
pub enum NodeLivenessStoreError {
    /// 저장소 자체가 고장났다. fail-closed 해야 한다.
    Storage { detail: String },
    /// heartbeat 가 구조적으로 쓸 수 없다.
    InvalidHeartbeat { detail: String },
    /// 서명자와 `device_id` 가 다르다.
    ///
    /// ★ `NodeHeartbeat::signer_id()` 가 `device_id` 이므로 정상
    ///   경로에서는 항상 같다. 그래도 확인한다 — 나중에 `signer_id()`
    ///   가 바뀌면 이 검사가 그것을 알려준다.
    SignerIsNotTheDevice { signer: String, device: String },
    /// 저장된 행이 손상됐다.
    Corrupt { kind: LivenessCorruption },
}

impl std::fmt::Display for NodeLivenessStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage { detail } => write!(f, "LIVENESS_STORAGE: {detail}"),
            Self::InvalidHeartbeat { detail } => write!(f, "LIVENESS_INVALID_HEARTBEAT: {detail}"),
            Self::SignerIsNotTheDevice { signer, device } => write!(
                f,
                "LIVENESS_SIGNER_MISMATCH: 서명자 {signer} 와 device_id {device} 가 다르다"
            ),
            Self::Corrupt { kind } => write!(f, "LIVENESS_CORRUPT: {kind:?}"),
        }
    }
}

impl std::error::Error for NodeLivenessStoreError {}

fn storage(error: impl std::fmt::Display) -> NodeLivenessStoreError {
    NodeLivenessStoreError::Storage {
        detail: error.to_string(),
    }
}

/// 노드별 최신 생존 사실을 담는 SQLite 저장소.
pub struct CoordinatorNodeLivenessStore {
    connection: Connection,
}

impl CoordinatorNodeLivenessStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, NodeLivenessStoreError> {
        let connection = Connection::open(path).map_err(storage)?;
        connection.busy_timeout(BUSY_TIMEOUT).map_err(storage)?;
        connection
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS coordinator_node_liveness (
                    node_id TEXT PRIMARY KEY,
                    device_id TEXT NOT NULL,
                    last_heartbeat_unix_ms INTEGER NOT NULL,
                    fence_epoch INTEGER NOT NULL,
                    running_attempts INTEGER NOT NULL,
                    signer_id_at_observation TEXT NOT NULL,
                    heartbeat_body BLOB NOT NULL,
                    heartbeat_hash BLOB NOT NULL
                );
                ",
            )
            .map_err(storage)?;
        Ok(Self { connection })
    }

    /// 검증된 heartbeat 를 관측으로 남긴다.
    ///
    /// # `&Verified<..>` 만 받는다
    ///
    /// ★ raw `pb::NodeHeartbeat` 은 이 경계를 넘지 못한다. 검증 안 된
    ///   heartbeat 를 저장하면, 그걸 근거로 "이 노드는 살아 있다" 고
    ///   판정하게 되고 — 아무나 남의 노드를 살아 있게 만들 수 있다.
    ///
    /// # 순서
    ///
    /// ```text
    /// 1  Verified::get() 뒤에만 필드를 읽는다
    /// 2  구조 검사·서명자 대조
    /// 3  body encode + hash 계산       (잠금 밖 — 오래 잡지 않는다)
    /// 4  BEGIN IMMEDIATE
    /// 5  기존 행과 시각 비교           (같은 transaction 안 — TOCTOU 없음)
    /// 6  더 최신일 때만 UPSERT
    /// ```
    pub fn observe(
        &mut self,
        verified: &Verified<pb::NodeHeartbeat>,
    ) -> Result<ObserveResult, NodeLivenessStoreError> {
        let heartbeat = verified.get();

        if heartbeat.node_id.trim().is_empty() {
            return Err(NodeLivenessStoreError::InvalidHeartbeat {
                detail: "node_id 가 비었다".into(),
            });
        }
        if heartbeat.device_id.trim().is_empty() {
            return Err(NodeLivenessStoreError::InvalidHeartbeat {
                detail: "device_id 가 비었다".into(),
            });
        }
        if heartbeat.issued_at_unix_ms == 0 {
            return Err(NodeLivenessStoreError::InvalidHeartbeat {
                detail: "issued_at_unix_ms 가 0 이다 — 시각 없는 관측은 판정에 쓸 수 없다".into(),
            });
        }
        let signer = verified.signer_id().to_string();
        if signer != heartbeat.device_id {
            return Err(NodeLivenessStoreError::SignerIsNotTheDevice {
                signer,
                device: heartbeat.device_id.clone(),
            });
        }

        // 서명을 포함한 완전한 body 를 남긴다 — 나중에 재검증하려면
        // 서명이 있어야 한다.
        let body = heartbeat.encode_to_vec();
        let hash = blake3_256(&body);

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;

        let existing: Option<(String, i64, i64, i64, String, Vec<u8>, Vec<u8>)> = transaction
            .query_row(
                "SELECT device_id, last_heartbeat_unix_ms, fence_epoch, running_attempts, \
                 signer_id_at_observation, heartbeat_body, heartbeat_hash \
                 FROM coordinator_node_liveness WHERE node_id = ?1",
                [&heartbeat.node_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .optional()
            .map_err(storage)?;

        if let Some(row) = &existing {
            let stored_at = u64::try_from(row.1).map_err(|_| NodeLivenessStoreError::Corrupt {
                kind: LivenessCorruption::IssuedAtMismatch,
            })?;
            // ★ 같거나 더 오래된 관측은 저장하지 않는다. 늦게 도착한 옛
            //   heartbeat 로 최신 값을 밀어내면 살아 있는 노드가 갑자기
            //   조용해 보인다.
            if heartbeat.issued_at_unix_ms <= stored_at {
                let stored = decode_row(&heartbeat.node_id, row)?;
                drop(transaction);
                return Ok(ObserveResult {
                    stored,
                    advanced: false,
                });
            }
        }

        transaction
            .execute(
                "INSERT INTO coordinator_node_liveness \
                 (node_id, device_id, last_heartbeat_unix_ms, fence_epoch, running_attempts, \
                  signer_id_at_observation, heartbeat_body, heartbeat_hash) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
                 ON CONFLICT(node_id) DO UPDATE SET \
                  device_id = excluded.device_id, \
                  last_heartbeat_unix_ms = excluded.last_heartbeat_unix_ms, \
                  fence_epoch = excluded.fence_epoch, \
                  running_attempts = excluded.running_attempts, \
                  signer_id_at_observation = excluded.signer_id_at_observation, \
                  heartbeat_body = excluded.heartbeat_body, \
                  heartbeat_hash = excluded.heartbeat_hash",
                rusqlite::params![
                    &heartbeat.node_id,
                    &heartbeat.device_id,
                    i64::try_from(heartbeat.issued_at_unix_ms).map_err(|_| {
                        NodeLivenessStoreError::InvalidHeartbeat {
                            detail: "issued_at_unix_ms 가 i64 범위를 넘는다".into(),
                        }
                    })?,
                    i64::try_from(heartbeat.fence_epoch).map_err(|_| {
                        NodeLivenessStoreError::InvalidHeartbeat {
                            detail: "fence_epoch 이 i64 범위를 넘는다".into(),
                        }
                    })?,
                    i64::from(heartbeat.running_attempts),
                    &heartbeat.device_id,
                    &body,
                    &hash[..],
                ],
            )
            .map_err(storage)?;
        transaction.commit().map_err(storage)?;

        Ok(ObserveResult {
            stored: StoredNodeLiveness {
                heartbeat: heartbeat.clone(),
                heartbeat_hash: hash,
                signer_id_at_observation: heartbeat.device_id.clone(),
                node_id: heartbeat.node_id.clone(),
                device_id: heartbeat.device_id.clone(),
                last_heartbeat_unix_ms: heartbeat.issued_at_unix_ms,
                fence_epoch: heartbeat.fence_epoch,
                running_attempts: heartbeat.running_attempts,
            },
            advanced: true,
        })
    }

    /// 저장된 모든 노드의 최신 생존 사실. `node_id` 오름차순.
    ///
    /// ★ 정렬을 SQL 이 아니라 여기서 보장한다고 쓰지 않는다 —
    ///   `ORDER BY` 로 결정적이다. `classify_node_liveness` 가 어차피
    ///   순서에 무관하지만, 결정적 출력은 진단을 쉽게 한다.
    pub fn load_all(&self) -> Result<Vec<StoredNodeLiveness>, NodeLivenessStoreError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT node_id, device_id, last_heartbeat_unix_ms, fence_epoch, \
                 running_attempts, signer_id_at_observation, heartbeat_body, heartbeat_hash \
                 FROM coordinator_node_liveness ORDER BY node_id ASC",
            )
            .map_err(storage)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    (
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, Vec<u8>>(6)?,
                        row.get::<_, Vec<u8>>(7)?,
                    ),
                ))
            })
            .map_err(storage)?;

        let mut out = Vec::new();
        for row in rows {
            let (node_id, rest) = row.map_err(storage)?;
            out.push(decode_row(&node_id, &rest)?);
        }
        Ok(out)
    }
}

/// 저장된 행을 되읽으며 **전부 다시 대조한다.**
///
/// ★ 읽을 때 검사하지 않으면 손상이 조용히 판정으로 흘러간다.
///   `DoD-52`·`DoD-53` 이 세운 것과 같은 규칙이다.
type Row = (String, i64, i64, i64, String, Vec<u8>, Vec<u8>);

fn decode_row(node_id: &str, row: &Row) -> Result<StoredNodeLiveness, NodeLivenessStoreError> {
    let (device_id, issued_at, fence_epoch, running_attempts, signer, body, hash_bytes) = row;

    if body.is_empty() {
        return Err(NodeLivenessStoreError::Corrupt {
            kind: LivenessCorruption::EmptyBody,
        });
    }
    let heartbeat = pb::NodeHeartbeat::decode(body.as_slice()).map_err(|_| {
        NodeLivenessStoreError::Corrupt {
            kind: LivenessCorruption::UndecodableBody,
        }
    })?;
    let hash: [u8; 32] =
        hash_bytes
            .as_slice()
            .try_into()
            .map_err(|_| NodeLivenessStoreError::Corrupt {
                kind: LivenessCorruption::HashEncoding,
            })?;
    if blake3_256(body) != hash {
        return Err(NodeLivenessStoreError::Corrupt {
            kind: LivenessCorruption::HashMismatch,
        });
    }
    if heartbeat.node_id != node_id {
        return Err(NodeLivenessStoreError::Corrupt {
            kind: LivenessCorruption::NodeIdMismatch,
        });
    }
    if &heartbeat.device_id != device_id || &heartbeat.device_id != signer {
        return Err(NodeLivenessStoreError::Corrupt {
            kind: LivenessCorruption::DeviceIdMismatch,
        });
    }
    let stored_at = u64::try_from(*issued_at).map_err(|_| NodeLivenessStoreError::Corrupt {
        kind: LivenessCorruption::IssuedAtMismatch,
    })?;
    if heartbeat.issued_at_unix_ms != stored_at {
        return Err(NodeLivenessStoreError::Corrupt {
            kind: LivenessCorruption::IssuedAtMismatch,
        });
    }
    let stored_epoch = u64::try_from(*fence_epoch).map_err(|_| NodeLivenessStoreError::Corrupt {
        kind: LivenessCorruption::FenceEpochMismatch,
    })?;
    if heartbeat.fence_epoch != stored_epoch {
        return Err(NodeLivenessStoreError::Corrupt {
            kind: LivenessCorruption::FenceEpochMismatch,
        });
    }
    let stored_attempts =
        u32::try_from(*running_attempts).map_err(|_| NodeLivenessStoreError::Corrupt {
            kind: LivenessCorruption::RunningAttemptsMismatch,
        })?;
    if heartbeat.running_attempts != stored_attempts {
        return Err(NodeLivenessStoreError::Corrupt {
            kind: LivenessCorruption::RunningAttemptsMismatch,
        });
    }

    Ok(StoredNodeLiveness {
        heartbeat_hash: hash,
        signer_id_at_observation: signer.clone(),
        node_id: node_id.to_string(),
        device_id: device_id.clone(),
        last_heartbeat_unix_ms: stored_at,
        fence_epoch: stored_epoch,
        running_attempts: stored_attempts,
        heartbeat,
    })
}
