//! 대체 통지 우편함 — Coordinator 쪽 영속(제안 `docs/contracts/proposals/2026-10-02_2207_대체_통지_우편함.md` v3 §3).
//!
//! ```text
//! 쓰는 때     장애 이어받기가 옛 Lease 를 폐기하는 **같은 트랜잭션**에서 **서명한 통지 바이트까지** 쓴다(`record_within`)
//!             — 폐기는 됐는데 서명된 통지가 없는 상태가 없다
//! 읽는 때     그 노드 앞으로 와서 받아들여진 답이 없는 것 전부(`unacked_for_node`) — 배달 · 새 실행 관문(다음 조각)
//! 지우지 않는다 추가 전용(감사) — 답이 온 행은 배달에서 빠질 뿐이다
//! ```
//! ★ 같은 notice_id 를 다시 쓰려 하면 **서명 대상 내용이 같을 때만** 멱등이다(Ed25519 서명은 결정적이라 같은 키 · 같은 내용이면 바이트도 같다).
//!   내용이 다르면 오류 — 같은 시도 · 노드 · fence 의 폐기를 두 가지로 말하지 않는다.

use gputeer_crypto::{sign, SigningKey};
use gputeer_protocol::mailbox_rules::{supersede_notice_id, validate_supersede_notice};
use gputeer_protocol::pb;
use gputeer_protocol::signing::signing_input;
use prost::Message;
use rusqlite::{Connection, OptionalExtension};

use crate::lease_store::encode_u64;

/// 통지에 서명하는 Coordinator — 서명자 신원(`--coordinator-id`)과 Coordinator 와 **같은** 키.
pub struct NoticeSigner {
    pub coordinator_id: String,
    pub key: SigningKey,
}

/// 폐기된 옛 시도의 신원과 그때 한 일 — 서명 전.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupersededAttempt {
    pub job_id: String,
    pub attempt_id: String,
    pub node_id: String,
    pub fence_epoch: u64,
    pub lease_id: String,
    pub cause: pb::SupersedeCause,
    pub job_disposition: pb::SupersedeJobDisposition,
    pub decided_at_unix_ms: u64,
}

/// 통지를 만들어 서명한다. notice_id 는 (attempt, node, fence) 로 계산한다 · 발행 시각 = 결정 시각.
pub fn sign_notice(signer: &NoticeSigner, superseded: &SupersededAttempt) -> pb::SupersedeNotice {
    let mut notice = pb::SupersedeNotice {
        schema_version: 1,
        notice_id: supersede_notice_id(
            &superseded.attempt_id,
            &superseded.node_id,
            superseded.fence_epoch,
        ),
        job_id: superseded.job_id.clone(),
        attempt_id: superseded.attempt_id.clone(),
        node_id: superseded.node_id.clone(),
        fence_epoch: superseded.fence_epoch,
        lease_id: superseded.lease_id.clone(),
        cause: superseded.cause as i32,
        job_disposition: superseded.job_disposition as i32,
        decided_at_unix_ms: superseded.decided_at_unix_ms,
        coordinator_id: signer.coordinator_id.clone(),
        issued_at_unix_ms: superseded.decided_at_unix_ms,
        coordinator_signature: Vec::new(),
    };
    notice.coordinator_signature = sign(&signer.key, &notice).to_vec();
    notice
}

/// 표가 없으면 만든다. 시각은 다른 표와 같이 8바이트 BE BLOB 이다.
pub(crate) fn ensure_table(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS coordinator_supersede_notices (
                notice_id TEXT PRIMARY KEY,
                node_id TEXT NOT NULL,
                job_id TEXT NOT NULL,
                attempt_id TEXT NOT NULL,
                fence_epoch BLOB NOT NULL CHECK(length(fence_epoch) = 8),
                lease_id TEXT NOT NULL,
                cause INTEGER NOT NULL,
                job_disposition INTEGER NOT NULL,
                decided_at_unix_ms BLOB NOT NULL CHECK(length(decided_at_unix_ms) = 8),
                signed_bytes BLOB NOT NULL,
                acked_at_unix_ms BLOB CHECK(acked_at_unix_ms IS NULL OR length(acked_at_unix_ms) = 8),
                ack_action INTEGER,
                ack_bytes BLOB
            );
            CREATE INDEX IF NOT EXISTS coordinator_supersede_notices_by_node
                ON coordinator_supersede_notices(node_id, acked_at_unix_ms);",
        )
        .map_err(|e| e.to_string())
}

/// 서명된 통지를 **호출자의 트랜잭션 안에서** 쓴다. 새로 썼으면 `true`, 같은 내용이 이미 있으면 `false`.
///   ★ 쓰기 전에 조합 규칙(notice_id 재계산 · enum · 신원)을 본다 — 잘못된 통지를 영속하지 않는다.
pub(crate) fn record_within(
    connection: &Connection,
    notice: &pb::SupersedeNotice,
) -> Result<bool, String> {
    ensure_table(connection)?;
    validate_supersede_notice(notice, &notice.node_id)
        .map_err(|e| format!("SUPERSEDE_NOTICE_INVALID — {e}"))?;
    let existing: Option<Vec<u8>> = connection
        .query_row(
            "SELECT signed_bytes FROM coordinator_supersede_notices WHERE notice_id = ?1",
            rusqlite::params![notice.notice_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(bytes) = existing {
        let stored = pb::SupersedeNotice::decode(bytes.as_slice()).map_err(|e| {
            format!(
                "SUPERSEDE_NOTICE_CORRUPT — 저장된 통지 {} 를 읽지 못했다: {e}",
                notice.notice_id
            )
        })?;
        if signing_input(&stored) == signing_input(notice) {
            return Ok(false);
        }
        return Err(format!(
            "SUPERSEDE_NOTICE_CONFLICT — 같은 notice_id {} 에 다른 내용을 쓰려 했다",
            notice.notice_id
        ));
    }
    connection
        .execute(
            "INSERT INTO coordinator_supersede_notices
                (notice_id, node_id, job_id, attempt_id, fence_epoch, lease_id, cause, job_disposition,
                 decided_at_unix_ms, signed_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                notice.notice_id,
                notice.node_id,
                notice.job_id,
                notice.attempt_id,
                encode_u64(notice.fence_epoch),
                notice.lease_id,
                notice.cause,
                notice.job_disposition,
                encode_u64(notice.decided_at_unix_ms),
                notice.encode_to_vec(),
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// 그 노드 앞으로 와서 받아들여진 답이 없는 통지 **전부** — 결정 시각 오래된 순(같으면 notice_id 순). 서명 포함 그대로 돌려준다.
pub fn unacked_for_node(
    connection: &Connection,
    node_id: &str,
) -> Result<Vec<pb::SupersedeNotice>, String> {
    ensure_table(connection)?;
    let mut statement = connection
        .prepare(
            "SELECT notice_id, signed_bytes FROM coordinator_supersede_notices
             WHERE node_id = ?1 AND acked_at_unix_ms IS NULL
             ORDER BY decided_at_unix_ms ASC, notice_id ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map(rusqlite::params![node_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut notices = Vec::new();
    for row in rows {
        let (notice_id, bytes) = row.map_err(|e| e.to_string())?;
        let notice = pb::SupersedeNotice::decode(bytes.as_slice()).map_err(|e| {
            format!("SUPERSEDE_NOTICE_CORRUPT — 저장된 통지 {notice_id} 를 읽지 못했다: {e}")
        })?;
        // 행의 키와 바이트 속 신원이 어긋나면 손상이다 — 다른 노드 앞 통지를 배달하지 않는다.
        if notice.notice_id != notice_id || notice.node_id != node_id {
            return Err(format!(
                "SUPERSEDE_NOTICE_CORRUPT — 행 {notice_id} 의 바이트가 다른 통지다"
            ));
        }
        notices.push(notice);
    }
    Ok(notices)
}

/// 시험용 서명자(고정 키).
#[cfg(test)]
pub(crate) fn test_signer() -> NoticeSigner {
    NoticeSigner {
        coordinator_id: "coordinator-test".into(),
        key: SigningKey::from_bytes(&[11u8; 32]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gputeer_crypto::{Ed25519Verifier, InMemoryKeyring};
    use gputeer_protocol::verify;

    fn signer() -> NoticeSigner {
        NoticeSigner {
            coordinator_id: "coordinator-1".into(),
            key: SigningKey::from_bytes(&[3u8; 32]),
        }
    }

    fn superseded(attempt_id: &str, decided_at: u64) -> SupersededAttempt {
        SupersededAttempt {
            job_id: "job-1".into(),
            attempt_id: attempt_id.into(),
            node_id: "node-1".into(),
            fence_epoch: 7,
            lease_id: "lease-1".into(),
            cause: pb::SupersedeCause::NodeLost,
            job_disposition: pb::SupersedeJobDisposition::Requeued,
            decided_at_unix_ms: decided_at,
        }
    }

    #[test]
    fn a_signed_notice_verifies_with_the_coordinator_key_and_passes_the_rules() {
        let signer = signer();
        let notice = sign_notice(&signer, &superseded("att-1", 1_000));
        validate_supersede_notice(&notice, "node-1").expect("조합 규칙");
        let mut keyring = InMemoryKeyring::new();
        keyring.insert("coordinator-1".to_string(), signer.key.verifying_key());
        let verifier = Ed25519Verifier::new(&keyring);
        // Evidence 라 며칠 뒤 시각으로도 검증된다
        verify(
            &notice,
            1,
            &verifier,
            1_000 + 7 * 24 * 3_600_000,
            &mut gputeer_protocol::signing::NoReplayCheck,
        )
        .expect("Coordinator 키로 검증돼야 한다");
        let mut other = InMemoryKeyring::new();
        other.insert(
            "coordinator-1".to_string(),
            SigningKey::from_bytes(&[4u8; 32]).verifying_key(),
        );
        assert!(
            verify(
                &notice,
                1,
                &Ed25519Verifier::new(&other),
                1_000,
                &mut gputeer_protocol::signing::NoReplayCheck,
            )
            .is_err(),
            "다른 키로 검증됐다"
        );
    }

    #[test]
    fn the_store_keeps_signed_bytes_is_idempotent_and_refuses_a_different_content() {
        let connection = Connection::open_in_memory().unwrap();
        let signer = signer();
        let first = sign_notice(&signer, &superseded("att-1", 2_000));
        assert_eq!(record_within(&connection, &first), Ok(true));
        assert_eq!(
            record_within(&connection, &first),
            Ok(false),
            "같은 내용은 멱등"
        );
        let conflicting = sign_notice(
            &signer,
            &SupersededAttempt {
                job_disposition: pb::SupersedeJobDisposition::Failed,
                ..superseded("att-1", 2_000)
            },
        );
        assert_eq!(conflicting.notice_id, first.notice_id);
        let error = record_within(&connection, &conflicting).expect_err("다른 내용을 받았다");
        assert!(error.contains("SUPERSEDE_NOTICE_CONFLICT"), "{error}");
        // 조합 규칙을 어긴 통지는 쓰지 않는다
        let mut broken = sign_notice(&signer, &superseded("att-2", 2_000));
        broken.notice_id = "not-the-id".into();
        let error = record_within(&connection, &broken).expect_err("잘못된 ID 를 받았다");
        assert!(error.contains("SUPERSEDE_NOTICE_INVALID"), "{error}");

        let second = sign_notice(&signer, &superseded("att-3", 1_500));
        assert_eq!(record_within(&connection, &second), Ok(true));
        let other_node = sign_notice(
            &signer,
            &SupersededAttempt {
                node_id: "node-2".into(),
                ..superseded("att-4", 1_000)
            },
        );
        assert_eq!(record_within(&connection, &other_node), Ok(true));
        // 그 노드 것만 · 결정 시각 오래된 순 · 서명 포함 그대로
        let unacked = unacked_for_node(&connection, "node-1").unwrap();
        assert_eq!(unacked, vec![second, first]);
        assert_eq!(
            unacked_for_node(&connection, "node-2").unwrap(),
            vec![other_node]
        );
        assert!(unacked_for_node(&connection, "node-3").unwrap().is_empty());
    }
}
