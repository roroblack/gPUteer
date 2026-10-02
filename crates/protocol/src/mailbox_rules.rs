//! 대체 통지 우편함(`SupersedeNotice` · `MailboxDelivery` · `MailboxAck` · `MailboxAckReceipt`)의 조합 규칙 — 서명 검증과 별개다. 둘 다 통과해야 쓴다.
//!
//! 제안 `docs/contracts/proposals/2026-10-02_2207_대체_통지_우편함.md` v3 §1:
//!
//! ```text
//! 통지     node_id = 받는 노드 · cause · job_disposition ≠ 0 · issued_at = decided_at · notice_id 가 다시 계산한 값과 같다
//! 배달     node_id = Hello 의 node_id · 안의 통지가 모두 그 node_id · notice_id 중복 없음 · session_nonce = 그 세션 Hello 의 nonce
//! 답       node_id = Hello 의 node_id · handled 의 notice_id 는 그 세션에서 실제로 배달한 것만 · 중복 없음 · notice_hash 가 배달한 그 통지와 같음 · action ≠ 0
//! 수신 확인 accepted ⊆ 답의 handled · session_nonce = 그 세션의 nonce
//! 모르는 enum 값은 거부(fail closed)
//! ```
//! ★ `Verified` 는 서명 통과이지 조합 규칙 통과가 아니다 — 받는 쪽이 이 함수를 **직접 불러야 한다**. 배달 안의 통지 서명은 겉 서명과 **따로** 검증한다
//!   (규칙 i 로 안의 서명 칸은 겉 서명에 묶이지 않는다).
//! ★ 2026-10-02 이 조각(계약층)에는 부르는 곳이 아직 없다 — Coordinator · Agent 쪽은 다음 조각이고, 그때의 의무다.

use crate::constants::SUPERSEDE_NOTICE_ID_TAG;
use crate::pb;
use crate::signing::signing_input;

/// 조합 규칙 위반.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailboxRuleError {
    /// 원인이 0 이거나 모르는 값이다.
    UnknownCause(i32),
    /// Job 처분이 0 이거나 모르는 값이다.
    UnknownDisposition(i32),
    /// 처리 결과가 0 이거나 모르는 값이다.
    UnknownAction(i32),
    /// 신원 칸(job · attempt · node · lease · notice_id)이 비었다.
    EmptyIdentity(&'static str),
    /// 다른 노드 앞이다.
    WrongNode { expected: String, got: String },
    /// 통지의 발행 시각이 결정 시각과 다르다.
    IssuedNotDecided,
    /// notice_id 가 신원으로 다시 계산한 값과 다르다.
    NoticeIdMismatch,
    /// 같은 notice_id 가 두 번 나온다.
    DuplicateNoticeId(String),
    /// 세션 nonce 가 그 세션 Hello 의 nonce 와 다르다.
    WrongSessionNonce,
    /// 그 세션에서 배달하지 않은 통지에 답했다.
    NotDelivered(String),
    /// 답의 통지 해시가 배달한 통지와 다르다.
    NoticeHashMismatch(String),
    /// 수신 확인이 답에 없는 통지를 받아들였다고 한다.
    NotHandled(String),
}

impl std::fmt::Display for MailboxRuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownCause(v) => write!(f, "폐기 원인을 모른다({v})"),
            Self::UnknownDisposition(v) => write!(f, "Job 처분을 모른다({v})"),
            Self::UnknownAction(v) => write!(f, "처리 결과를 모른다({v})"),
            Self::EmptyIdentity(field) => write!(f, "신원 칸이 비었다({field})"),
            Self::WrongNode { expected, got } => {
                write!(f, "다른 노드 앞이다(기대 {expected} · 받은 값 {got})")
            }
            Self::IssuedNotDecided => write!(f, "통지의 발행 시각이 결정 시각과 다르다"),
            Self::NoticeIdMismatch => write!(f, "notice_id 가 신원으로 계산한 값과 다르다"),
            Self::DuplicateNoticeId(id) => write!(f, "notice_id 가 두 번 나온다({id})"),
            Self::WrongSessionNonce => write!(f, "세션 nonce 가 그 세션 Hello 의 nonce 와 다르다"),
            Self::NotDelivered(id) => write!(f, "이 세션에서 배달하지 않은 통지에 답했다({id})"),
            Self::NoticeHashMismatch(id) => {
                write!(f, "답의 통지 해시가 배달한 통지와 다르다({id})")
            }
            Self::NotHandled(id) => write!(f, "답에 없는 통지를 받아들였다고 한다({id})"),
        }
    }
}

impl std::error::Error for MailboxRuleError {}

/// `notice_id` = hex(BLAKE3-256(tag ‖ u32 BE 길이 + attempt_id ‖ u32 BE 길이 + node_id ‖ u64 BE fence_epoch)). 같은 시도 · 노드 · fence 의 폐기는 같은 ID 다.
///   길이를 앞에 붙여 이어 붙임의 경계가 갈리지 않게 한다("ab"+"c" 와 "a"+"bc" 가 같은 ID 가 되지 않는다).
pub fn supersede_notice_id(attempt_id: &str, node_id: &str, fence_epoch: u64) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(SUPERSEDE_NOTICE_ID_TAG);
    hasher.update(&(attempt_id.len() as u32).to_be_bytes());
    hasher.update(attempt_id.as_bytes());
    hasher.update(&(node_id.len() as u32).to_be_bytes());
    hasher.update(node_id.as_bytes());
    hasher.update(&fence_epoch.to_be_bytes());
    hasher.finalize().to_hex().to_string()
}

/// 답이 싣는 통지 해시 — 그 통지의 BLAKE3-256(sig_input). 서명 칸은 sig_input 에 들지 않는다.
pub fn supersede_notice_hash(notice: &pb::SupersedeNotice) -> pb::Digest {
    pb::Digest {
        algo: pb::HashAlgorithm::Blake3256 as i32,
        value: blake3::hash(&signing_input(notice)).as_bytes().to_vec(),
    }
}

fn expect_node(expected: &str, got: &str) -> Result<(), MailboxRuleError> {
    if expected != got {
        return Err(MailboxRuleError::WrongNode {
            expected: expected.to_string(),
            got: got.to_string(),
        });
    }
    Ok(())
}

fn not_empty(value: &str, field: &'static str) -> Result<(), MailboxRuleError> {
    if value.is_empty() {
        return Err(MailboxRuleError::EmptyIdentity(field));
    }
    Ok(())
}

/// 통지 **하나**의 조합 규칙. `receiving_node_id` 는 받는 노드(Agent 자신 · Coordinator 가 배달하는 세션의 Hello node_id).
pub fn validate_supersede_notice(
    notice: &pb::SupersedeNotice,
    receiving_node_id: &str,
) -> Result<(), MailboxRuleError> {
    not_empty(&notice.notice_id, "notice_id")?;
    not_empty(&notice.job_id, "job_id")?;
    not_empty(&notice.attempt_id, "attempt_id")?;
    not_empty(&notice.node_id, "node_id")?;
    not_empty(&notice.lease_id, "lease_id")?;
    expect_node(receiving_node_id, &notice.node_id)?;
    match pb::SupersedeCause::try_from(notice.cause) {
        Ok(pb::SupersedeCause::Unspecified) | Err(_) => {
            return Err(MailboxRuleError::UnknownCause(notice.cause))
        }
        Ok(_) => {}
    }
    match pb::SupersedeJobDisposition::try_from(notice.job_disposition) {
        Ok(pb::SupersedeJobDisposition::Unspecified) | Err(_) => {
            return Err(MailboxRuleError::UnknownDisposition(notice.job_disposition))
        }
        Ok(_) => {}
    }
    if notice.issued_at_unix_ms != notice.decided_at_unix_ms {
        return Err(MailboxRuleError::IssuedNotDecided);
    }
    if notice.notice_id
        != supersede_notice_id(&notice.attempt_id, &notice.node_id, notice.fence_epoch)
    {
        return Err(MailboxRuleError::NoticeIdMismatch);
    }
    Ok(())
}

/// 배달의 조합 규칙 — 겉(노드 · 세션)과 안의 통지 **전부**. 안의 통지 서명은 여기서 보지 않는다(받는 쪽이 따로 검증한다).
pub fn validate_mailbox_delivery(
    delivery: &pb::MailboxDelivery,
    hello_node_id: &str,
    hello_nonce: &[u8],
) -> Result<(), MailboxRuleError> {
    expect_node(hello_node_id, &delivery.node_id)?;
    if delivery.session_nonce != hello_nonce {
        return Err(MailboxRuleError::WrongSessionNonce);
    }
    let mut seen = std::collections::BTreeSet::new();
    for notice in &delivery.notices {
        validate_supersede_notice(notice, hello_node_id)?;
        if !seen.insert(notice.notice_id.as_str()) {
            return Err(MailboxRuleError::DuplicateNoticeId(
                notice.notice_id.clone(),
            ));
        }
    }
    Ok(())
}

/// 답의 조합 규칙 — `delivered` 는 **그 세션에서 실제로 배달한** 통지들이다(Coordinator 가 보낸 배달의 notices).
pub fn validate_mailbox_ack(
    ack: &pb::MailboxAck,
    hello_node_id: &str,
    hello_nonce: &[u8],
    delivered: &[pb::SupersedeNotice],
) -> Result<(), MailboxRuleError> {
    expect_node(hello_node_id, &ack.node_id)?;
    if ack.session_nonce != hello_nonce {
        return Err(MailboxRuleError::WrongSessionNonce);
    }
    let mut seen = std::collections::BTreeSet::new();
    for handled in &ack.handled {
        match pb::MailboxAction::try_from(handled.action) {
            Ok(pb::MailboxAction::Unspecified) | Err(_) => {
                return Err(MailboxRuleError::UnknownAction(handled.action))
            }
            Ok(_) => {}
        }
        if !seen.insert(handled.notice_id.as_str()) {
            return Err(MailboxRuleError::DuplicateNoticeId(
                handled.notice_id.clone(),
            ));
        }
        let notice = delivered
            .iter()
            .find(|n| n.notice_id == handled.notice_id)
            .ok_or_else(|| MailboxRuleError::NotDelivered(handled.notice_id.clone()))?;
        if handled.notice_hash.as_ref() != Some(&supersede_notice_hash(notice)) {
            return Err(MailboxRuleError::NoticeHashMismatch(
                handled.notice_id.clone(),
            ));
        }
    }
    Ok(())
}

/// 수신 확인의 조합 규칙 — `ack` 는 그 세션에서 보낸 답이다.
pub fn validate_mailbox_ack_receipt(
    receipt: &pb::MailboxAckReceipt,
    hello_node_id: &str,
    hello_nonce: &[u8],
    ack: &pb::MailboxAck,
) -> Result<(), MailboxRuleError> {
    expect_node(hello_node_id, &receipt.node_id)?;
    if receipt.session_nonce != hello_nonce {
        return Err(MailboxRuleError::WrongSessionNonce);
    }
    let mut seen = std::collections::BTreeSet::new();
    for id in &receipt.accepted {
        if !seen.insert(id.as_str()) {
            return Err(MailboxRuleError::DuplicateNoticeId(id.clone()));
        }
        if !ack.handled.iter().any(|h| &h.notice_id == id) {
            return Err(MailboxRuleError::NotHandled(id.clone()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NODE: &str = "node-1";
    const NONCE: &[u8] = &[7u8; 16];

    fn notice(attempt: &str, fence: u64) -> pb::SupersedeNotice {
        pb::SupersedeNotice {
            schema_version: 1,
            notice_id: supersede_notice_id(attempt, NODE, fence),
            job_id: "job-1".into(),
            attempt_id: attempt.into(),
            node_id: NODE.into(),
            fence_epoch: fence,
            lease_id: "lease-1".into(),
            cause: pb::SupersedeCause::NodeLost as i32,
            job_disposition: pb::SupersedeJobDisposition::Requeued as i32,
            decided_at_unix_ms: 1_000,
            coordinator_id: "coordinator-1".into(),
            issued_at_unix_ms: 1_000,
            coordinator_signature: vec![b'K'; 64],
        }
    }

    fn delivery(notices: Vec<pb::SupersedeNotice>) -> pb::MailboxDelivery {
        pb::MailboxDelivery {
            schema_version: 1,
            node_id: NODE.into(),
            notices,
            coordinator_id: "coordinator-1".into(),
            issued_at_unix_ms: 2_000,
            session_nonce: NONCE.to_vec(),
            coordinator_signature: vec![b'K'; 64],
        }
    }

    fn handled(n: &pb::SupersedeNotice, action: pb::MailboxAction) -> pb::SupersedeHandled {
        pb::SupersedeHandled {
            notice_id: n.notice_id.clone(),
            notice_hash: Some(supersede_notice_hash(n)),
            action: action as i32,
        }
    }

    fn ack(handled: Vec<pb::SupersedeHandled>) -> pb::MailboxAck {
        pb::MailboxAck {
            schema_version: 1,
            node_id: NODE.into(),
            handled,
            issued_at_unix_ms: 3_000,
            session_nonce: NONCE.to_vec(),
            node_signature: vec![b'N'; 64],
        }
    }

    #[test]
    fn the_notice_id_binds_attempt_node_and_fence_with_length_prefixes() {
        let id = supersede_notice_id("att-1", NODE, 42);
        assert_eq!(id.len(), 64, "hex BLAKE3-256 이어야 한다");
        assert_eq!(
            id,
            supersede_notice_id("att-1", NODE, 42),
            "같은 신원은 같은 ID"
        );
        assert_ne!(id, supersede_notice_id("att-2", NODE, 42));
        assert_ne!(id, supersede_notice_id("att-1", "node-2", 42));
        assert_ne!(id, supersede_notice_id("att-1", NODE, 43));
        // 길이 앞머리가 없으면 경계가 섞여 같아진다
        assert_ne!(
            supersede_notice_id("ab", "c", 1),
            supersede_notice_id("a", "bc", 1)
        );
    }

    #[test]
    fn a_well_formed_notice_passes_and_each_broken_field_is_refused() {
        let good = notice("att-1", 42);
        assert_eq!(validate_supersede_notice(&good, NODE), Ok(()));
        let failed = pb::SupersedeNotice {
            job_disposition: pb::SupersedeJobDisposition::Failed as i32,
            ..good.clone()
        };
        assert_eq!(validate_supersede_notice(&failed, NODE), Ok(()));

        assert!(matches!(
            validate_supersede_notice(&good, "node-2"),
            Err(MailboxRuleError::WrongNode { .. })
        ));
        for cause in [0, 2, -1] {
            let bad = pb::SupersedeNotice {
                cause,
                ..good.clone()
            };
            assert_eq!(
                validate_supersede_notice(&bad, NODE),
                Err(MailboxRuleError::UnknownCause(cause))
            );
        }
        for disposition in [0, 3] {
            let bad = pb::SupersedeNotice {
                job_disposition: disposition,
                ..good.clone()
            };
            assert_eq!(
                validate_supersede_notice(&bad, NODE),
                Err(MailboxRuleError::UnknownDisposition(disposition))
            );
        }
        let late = pb::SupersedeNotice {
            issued_at_unix_ms: 1_001,
            ..good.clone()
        };
        assert_eq!(
            validate_supersede_notice(&late, NODE),
            Err(MailboxRuleError::IssuedNotDecided)
        );
        // 신원을 바꾸고 ID 를 그대로 두면 거부한다(다른 시도의 통지로 쓸 수 없다)
        let moved = pb::SupersedeNotice {
            fence_epoch: 43,
            ..good.clone()
        };
        assert_eq!(
            validate_supersede_notice(&moved, NODE),
            Err(MailboxRuleError::NoticeIdMismatch)
        );
        let empty = pb::SupersedeNotice {
            lease_id: String::new(),
            ..good
        };
        assert_eq!(
            validate_supersede_notice(&empty, NODE),
            Err(MailboxRuleError::EmptyIdentity("lease_id"))
        );
    }

    #[test]
    fn a_delivery_is_for_one_node_one_session_and_lists_each_notice_once() {
        let a = notice("att-1", 42);
        let b = notice("att-2", 43);
        assert_eq!(
            validate_mailbox_delivery(&delivery(vec![]), NODE, NONCE),
            Ok(()),
            "빈 배달은 '지금 우편함은 비었다' 다"
        );
        assert_eq!(
            validate_mailbox_delivery(&delivery(vec![a.clone(), b.clone()]), NODE, NONCE),
            Ok(())
        );
        assert!(matches!(
            validate_mailbox_delivery(&delivery(vec![a.clone()]), "node-2", NONCE),
            Err(MailboxRuleError::WrongNode { .. })
        ));
        assert_eq!(
            validate_mailbox_delivery(&delivery(vec![a.clone()]), NODE, &[8u8; 16]),
            Err(MailboxRuleError::WrongSessionNonce)
        );
        assert_eq!(
            validate_mailbox_delivery(&delivery(vec![a.clone(), a.clone()]), NODE, NONCE),
            Err(MailboxRuleError::DuplicateNoticeId(a.notice_id.clone()))
        );
        // 다른 노드 앞 통지가 섞이면 거부한다
        let other = pb::SupersedeNotice {
            node_id: "node-2".into(),
            notice_id: supersede_notice_id("att-3", "node-2", 44),
            attempt_id: "att-3".into(),
            fence_epoch: 44,
            ..a.clone()
        };
        assert!(matches!(
            validate_mailbox_delivery(&delivery(vec![a, other]), NODE, NONCE),
            Err(MailboxRuleError::WrongNode { .. })
        ));
    }

    #[test]
    fn an_ack_answers_only_delivered_notices_with_their_hash() {
        let a = notice("att-1", 42);
        let b = notice("att-2", 43);
        let delivered = vec![a.clone(), b.clone()];
        let good = ack(vec![
            handled(&a, pb::MailboxAction::Stopped),
            handled(&b, pb::MailboxAction::NotRunning),
        ]);
        assert_eq!(validate_mailbox_ack(&good, NODE, NONCE, &delivered), Ok(()));
        // 일부만 답해도 된다 — 답하지 않은 것은 다음 배달에 다시 간다
        assert_eq!(
            validate_mailbox_ack(
                &ack(vec![handled(&b, pb::MailboxAction::Stopped)]),
                NODE,
                NONCE,
                &delivered
            ),
            Ok(())
        );

        let c = notice("att-3", 44);
        assert_eq!(
            validate_mailbox_ack(
                &ack(vec![handled(&c, pb::MailboxAction::Stopped)]),
                NODE,
                NONCE,
                &delivered
            ),
            Err(MailboxRuleError::NotDelivered(c.notice_id.clone()))
        );
        let twice = ack(vec![
            handled(&a, pb::MailboxAction::Stopped),
            handled(&a, pb::MailboxAction::Stopped),
        ]);
        assert_eq!(
            validate_mailbox_ack(&twice, NODE, NONCE, &delivered),
            Err(MailboxRuleError::DuplicateNoticeId(a.notice_id.clone()))
        );
        // 해시가 다른 통지(같은 ID · 다른 내용)의 것이면 거부한다
        let altered = pb::SupersedeNotice {
            job_disposition: pb::SupersedeJobDisposition::Failed as i32,
            ..a.clone()
        };
        let wrong_hash = ack(vec![handled(&altered, pb::MailboxAction::Stopped)]);
        assert_eq!(
            validate_mailbox_ack(&wrong_hash, NODE, NONCE, &delivered),
            Err(MailboxRuleError::NoticeHashMismatch(a.notice_id.clone()))
        );
        let no_hash = ack(vec![pb::SupersedeHandled {
            notice_hash: None,
            ..handled(&a, pb::MailboxAction::Stopped)
        }]);
        assert_eq!(
            validate_mailbox_ack(&no_hash, NODE, NONCE, &delivered),
            Err(MailboxRuleError::NoticeHashMismatch(a.notice_id.clone()))
        );
        for action in [0, 3] {
            let bad = ack(vec![pb::SupersedeHandled {
                action,
                ..handled(&a, pb::MailboxAction::Stopped)
            }]);
            assert_eq!(
                validate_mailbox_ack(&bad, NODE, NONCE, &delivered),
                Err(MailboxRuleError::UnknownAction(action))
            );
        }
        assert!(matches!(
            validate_mailbox_ack(&good, "node-2", NONCE, &delivered),
            Err(MailboxRuleError::WrongNode { .. })
        ));
        assert_eq!(
            validate_mailbox_ack(&good, NODE, &[8u8; 16], &delivered),
            Err(MailboxRuleError::WrongSessionNonce)
        );
    }

    #[test]
    fn a_receipt_accepts_only_what_the_ack_handled() {
        let a = notice("att-1", 42);
        let b = notice("att-2", 43);
        let sent = ack(vec![handled(&a, pb::MailboxAction::Stopped)]);
        let receipt = |accepted: Vec<String>, nonce: &[u8]| pb::MailboxAckReceipt {
            schema_version: 1,
            node_id: NODE.into(),
            accepted,
            coordinator_id: "coordinator-1".into(),
            issued_at_unix_ms: 4_000,
            session_nonce: nonce.to_vec(),
            coordinator_signature: vec![b'K'; 64],
        };
        assert_eq!(
            validate_mailbox_ack_receipt(
                &receipt(vec![a.notice_id.clone()], NONCE),
                NODE,
                NONCE,
                &sent
            ),
            Ok(())
        );
        assert_eq!(
            validate_mailbox_ack_receipt(&receipt(vec![], NONCE), NODE, NONCE, &sent),
            Ok(()),
            "아무것도 받아들이지 않은 수신 확인도 형식은 맞다"
        );
        assert_eq!(
            validate_mailbox_ack_receipt(
                &receipt(vec![b.notice_id.clone()], NONCE),
                NODE,
                NONCE,
                &sent
            ),
            Err(MailboxRuleError::NotHandled(b.notice_id.clone()))
        );
        assert_eq!(
            validate_mailbox_ack_receipt(
                &receipt(vec![a.notice_id.clone(), a.notice_id.clone()], NONCE),
                NODE,
                NONCE,
                &sent
            ),
            Err(MailboxRuleError::DuplicateNoticeId(a.notice_id.clone()))
        );
        assert_eq!(
            validate_mailbox_ack_receipt(
                &receipt(vec![a.notice_id.clone()], &[8u8; 16]),
                NODE,
                NONCE,
                &sent
            ),
            Err(MailboxRuleError::WrongSessionNonce)
        );
        assert!(matches!(
            validate_mailbox_ack_receipt(&receipt(vec![], NONCE), "node-2", NONCE, &sent),
            Err(MailboxRuleError::WrongNode { .. })
        ));
    }
}
