//! 대체 통지 우편함 — Agent 쪽(제안 `docs/contracts/proposals/2026-10-02_2207_대체_통지_우편함.md` v3 §2 · §4).
//!
//! ```text
//! 세션        새 연결 -> Hello(MAILBOX) -> 배달(겉 서명 · 안의 통지 서명을 **각각** Coordinator 키로 검증 · 조합 규칙) -> 통지마다 처리 ->
//!             답(노드 서명) -> 수신 확인(검증) -> 닫는다. 답할 것이 없으면 답하지 않고 닫는다(다음 세션에 다시 온다)
//! 회차 전     (`--use-mailbox`) FRESH 를 열기 전에 우편함을 비운다 — 남은 통지가 있으면 이 회차는 일을 받으러 가지 않는다
//!             (Coordinator 의 새 실행 관문을 헛되이 두드리지 않는다). 실패하면 다음 회차에 다시
//! NOT_RUNNING 원장(run-ledger)에 그 시도 행이 없거나 CLOSED 이고, 이 프로세스의 실행 목록에도 없을 때만. ACTIVE · LOCAL_BLOCKED 면 **답하지 않는다** —
//!             살아 있을 수 있는 컨테이너다. 그 행을 닫는 것은 실행 알림 계약(v17)의 몫이다
//! ```
//! ★ 수신 확인을 잃으면 다음 배달을 본다 — 거기 없으면 커밋됐다(처리 끝) · 있으면 같은 처리를 되풀이한다(멱등).
//! ★ 우편함 오류 · 답 보류는 **새 작업**만 막는다. 소유자의 즉시 비우기 · 강제 종료는 언제나 된다(§0.1).
//! 실행 중     그 시도가 도는 동안 갱신 스레드와 **따로** 감시 스레드가 갱신 주기로 묻는다(갱신 거부 · 로컬 만료 · 연결 실패와 무관하게 실행이 끝날
//!             때까지). 지금 도는 이 시도의 통지면 서명된 거부와 같은 경로로 멈추게 하고(소유자 "계속" 도 무시), 정지가 **성공한 뒤에만** STOPPED 로 답한다

use std::io::Write;

use gputeer_crypto::{
    read_frame, sign, write_frame, Clock, Ed25519Verifier, FrameType, InMemoryKeyring,
    InMemoryReplayGuard, IngressMessage, KeyDirectorySource, SigningKey, SystemClock,
};
use gputeer_protocol::constants::{MAILBOX_MAX_SCHEMA_VERSION, MODE_MAILBOX};
use gputeer_protocol::mailbox_rules::{
    supersede_notice_hash, validate_mailbox_ack_receipt, validate_mailbox_delivery,
};
use gputeer_protocol::{pb, verify};
use prost::Message;

use crate::AgentConfig;

/// 한 MAILBOX 세션의 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MailboxRound {
    /// 지금 우편함은 비었다.
    Empty,
    /// 배달된 통지를 모두 처리했고 Coordinator 가 커밋했다.
    Cleared { accepted: Vec<String> },
    /// 처리하지 못한(답을 보류한) 통지가 남았다.
    Pending { remaining: usize },
}

/// MAILBOX 세션 한 번. `decide` 는 통지마다 답(STOPPED · NOT_RUNNING)을 정하고, 정하지 못하면 `None`(답 보류)이다.
pub(crate) fn session_once(
    config: &AgentConfig,
    signing_key: &SigningKey,
    decide: &mut dyn FnMut(&pb::SupersedeNotice) -> Option<pb::MailboxAction>,
) -> Result<MailboxRound, String> {
    let clock = SystemClock;
    let mut stream = crate::connect_with_timeout(&config.coordinator_addr, crate::IO_TIMEOUT)?;
    stream
        .set_read_timeout(Some(crate::IO_TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(crate::IO_TIMEOUT))
        .map_err(|e| e.to_string())?;
    let mut hello = pb::AgentSessionHello {
        schema_version: 1,
        mode: MODE_MAILBOX,
        session_id: config.session_id.clone(),
        node_id: config.agent_device_id.clone(),
        // MAILBOX 는 Coordinator 가 연결 번호를 대조하지 않는다 — REPORT · RENEW 와 같다.
        connection_attempt: 0,
        issued_at_unix_ms: clock.now_unix_ms(),
        nonce: crate::fresh_nonce()?,
        ..Default::default()
    };
    hello.node_signature = sign(signing_key, &hello).to_vec();
    let frame = write_frame(FrameType::SessionHello, &hello.encode_to_vec())
        .map_err(|e| format!("Hello(MAILBOX) 프레임 인코딩 실패: {e}"))?;
    stream
        .write_all(&frame)
        .and_then(|()| stream.flush())
        .map_err(|e| format!("Hello(MAILBOX) 전송 실패: {e}"))?;

    let mut keys = InMemoryKeyring::new();
    keys.insert(
        config.coordinator_device_id.clone(),
        config.coordinator_verifying_key,
    );
    let mut replay = InMemoryReplayGuard::new();
    let message = read_frame(
        &mut stream,
        MAILBOX_MAX_SCHEMA_VERSION,
        KeyDirectorySource::Provided(&keys),
        &mut replay,
        &clock,
    )
    .map_err(|e| format!("MAILBOX_DELIVERY_REJECTED: 배달을 받지 못했거나 검증하지 못했다: {e}"))?;
    let delivery = match message {
        IngressMessage::MailboxDelivery(verified) => verified
            .require_replay_checked()
            .map_err(|e| format!("MAILBOX_DELIVERY_REJECTED: replay 검사 실패: {e:?}"))?
            .clone(),
        _ => return Err("MAILBOX_DELIVERY_REJECTED: 배달이 아닌 프레임이다".into()),
    };
    // 서명 검증 뒤에야 필드를 본다.
    if delivery.coordinator_id != config.coordinator_device_id {
        return Err(format!(
            "MAILBOX_DELIVERY_REJECTED: coordinator_id 불일치 — 기대값 {}",
            config.coordinator_device_id
        ));
    }
    validate_mailbox_delivery(&delivery, &config.agent_device_id, &hello.nonce)
        .map_err(|rule| format!("MAILBOX_DELIVERY_REJECTED: {rule}"))?;
    // ★ 안의 통지는 겉 서명에 서명 칸이 묶이지 않는다(규칙 i) — 하나씩 Coordinator 키로 따로 검증한다. Evidence 라 며칠 뒤에도 유효하다.
    let verifier = Ed25519Verifier::new(&keys);
    for notice in &delivery.notices {
        verify(
            notice,
            MAILBOX_MAX_SCHEMA_VERSION,
            &verifier,
            clock.now_unix_ms(),
            &mut gputeer_protocol::signing::NoReplayCheck,
        )
        .map_err(|e| {
            format!(
                "MAILBOX_DELIVERY_REJECTED: 통지 {} 의 서명을 검증하지 못했다: {e:?}",
                notice.notice_id
            )
        })?;
        if notice.coordinator_id != config.coordinator_device_id {
            return Err(format!(
                "MAILBOX_DELIVERY_REJECTED: 통지 {} 의 서명자가 이 Coordinator 가 아니다",
                notice.notice_id
            ));
        }
    }
    println!("MAILBOX_DELIVERY_VERIFIED count={}", delivery.notices.len());
    if delivery.notices.is_empty() {
        return Ok(MailboxRound::Empty);
    }

    let mut handled = Vec::new();
    for notice in &delivery.notices {
        match decide(notice) {
            Some(action) => {
                println!(
                    "MAILBOX_NOTICE_HANDLED notice_id={} attempt_id={} action={action:?}",
                    notice.notice_id, notice.attempt_id
                );
                handled.push(pb::SupersedeHandled {
                    notice_id: notice.notice_id.clone(),
                    notice_hash: Some(supersede_notice_hash(notice)),
                    action: action as i32,
                });
            }
            None => println!(
                "MAILBOX_NOTICE_WITHHELD notice_id={} attempt_id={} — 아직 답하지 않는다(다음 세션에 다시 온다)",
                notice.notice_id, notice.attempt_id
            ),
        }
    }
    if handled.is_empty() {
        return Ok(MailboxRound::Pending {
            remaining: delivery.notices.len(),
        });
    }
    let mut ack = pb::MailboxAck {
        schema_version: 1,
        node_id: config.agent_device_id.clone(),
        handled,
        issued_at_unix_ms: clock.now_unix_ms(),
        session_nonce: hello.nonce.clone(),
        node_signature: Vec::new(),
    };
    ack.node_signature = sign(signing_key, &ack).to_vec();
    let frame = write_frame(FrameType::MailboxAck, &ack.encode_to_vec())
        .map_err(|e| format!("MailboxAck 프레임 인코딩 실패: {e}"))?;
    stream
        .write_all(&frame)
        .and_then(|()| stream.flush())
        .map_err(|e| format!("MailboxAck 전송 실패: {e}"))?;
    let message = read_frame(
        &mut stream,
        MAILBOX_MAX_SCHEMA_VERSION,
        KeyDirectorySource::Provided(&keys),
        &mut replay,
        &clock,
    )
    .map_err(|e| {
        format!(
            "MAILBOX_RECEIPT_MISSING: 수신 확인을 받지 못했다 — 다음 배달에 없으면 커밋된 것이다: {e}"
        )
    })?;
    let receipt = match message {
        IngressMessage::MailboxAckReceipt(verified) => verified
            .require_replay_checked()
            .map_err(|e| format!("MAILBOX_RECEIPT_REJECTED: replay 검사 실패: {e:?}"))?
            .clone(),
        _ => return Err("MAILBOX_RECEIPT_REJECTED: 수신 확인이 아닌 프레임이다".into()),
    };
    if receipt.coordinator_id != config.coordinator_device_id {
        return Err(format!(
            "MAILBOX_RECEIPT_REJECTED: coordinator_id 불일치 — 기대값 {}",
            config.coordinator_device_id
        ));
    }
    validate_mailbox_ack_receipt(&receipt, &config.agent_device_id, &hello.nonce, &ack)
        .map_err(|rule| format!("MAILBOX_RECEIPT_REJECTED: {rule}"))?;
    println!(
        "MAILBOX_RECEIPT_VERIFIED accepted={}",
        receipt.accepted.join(",")
    );
    let remaining = delivery
        .notices
        .len()
        .saturating_sub(receipt.accepted.len());
    Ok(if remaining == 0 {
        MailboxRound::Cleared {
            accepted: receipt.accepted,
        }
    } else {
        MailboxRound::Pending { remaining }
    })
}

/// 회차 전 처리 — 이 노드에서 그 시도가 **돌지 않는다고 확인될 때만** NOT_RUNNING. 확인하지 못하면 답하지 않는다.
///
/// ```text
/// 이 프로세스의 실행 목록에 있다         답하지 않는다(실행 중 처리는 다음 조각 — 감시 스레드가 멈춘 뒤 STOPPED)
/// 원장 행이 없다 · CLOSED               NOT_RUNNING
/// 원장 행이 ACTIVE · LOCAL_BLOCKED      답하지 않는다 — 재기동 뒤 살아 있는 컨테이너일 수 있다
/// 원장을 못 읽었다 · 원장이 꺼져 있다     답하지 않는다(우편함은 원장을 요구한다 — 기동 검사가 막는다)
/// ```
pub(crate) fn decide_when_idle(
    config: &AgentConfig,
    notice: &pb::SupersedeNotice,
) -> Option<pb::MailboxAction> {
    if config.owner_panel_state.is_running(&notice.attempt_id) {
        return None;
    }
    match crate::with_run_ledger(config, |ledger| ledger.row(&notice.attempt_id)) {
        Some(Ok(None)) => Some(pb::MailboxAction::NotRunning),
        Some(Ok(Some(row))) if row.state == crate::run_ledger::RowState::Closed => {
            Some(pb::MailboxAction::NotRunning)
        }
        Some(Ok(Some(row))) => {
            println!(
                "MAILBOX_LEDGER_OPEN attempt_id={} state={:?} — 원장 행이 닫히지 않아 답하지 않는다",
                notice.attempt_id, row.state
            );
            None
        }
        Some(Err(error)) => {
            println!(
                "MAILBOX_LEDGER_UNREADABLE attempt_id={} detail={error}",
                notice.attempt_id
            );
            None
        }
        None => None,
    }
}

/// 회차 전 우편함 비우기 — 비었거나 다 처리했으면 `Ok(())`, 남았으면 FRESH 를 열지 않도록 오류.
pub(crate) fn empty_before_fresh(
    config: &AgentConfig,
    signing_key: &SigningKey,
) -> Result<(), String> {
    match session_once(config, signing_key, &mut |notice| {
        decide_when_idle(config, notice)
    }) {
        Ok(MailboxRound::Empty) | Ok(MailboxRound::Cleared { .. }) => Ok(()),
        Ok(MailboxRound::Pending { remaining }) => Err(format!(
            "MAILBOX_NOT_EMPTY: 처리하지 못한 대체 통지 {remaining}건이 남아 이 회차는 일을 받지 않는다(다음 회차에 다시 묻는다)"
        )),
        Err(error) => Err(format!(
            "MAILBOX_SESSION_FAILED: 우편함을 비우지 못해 이 회차는 일을 받지 않는다 — {error}"
        )),
    }
}

/// 실행 중 처리 — 지금 도는 이 시도의 통지면 멈추게 하고 정지가 성공한 뒤에만 STOPPED. 다른 시도는 회차 전과 같다(원장).
///   `requested` 는 정지를 요청했는지 — 실행이 끝날 때 마지막 답을 보낼지 정한다.
pub(crate) fn decide_while_running(
    config: &AgentConfig,
    running_attempt: &str,
    notice: &pb::SupersedeNotice,
    requested: &mut bool,
) -> Option<pb::MailboxAction> {
    if notice.attempt_id != running_attempt {
        return decide_when_idle(config, notice);
    }
    let panel = &config.owner_panel_state;
    if panel.stopped_for_disconnect(running_attempt) {
        return Some(pb::MailboxAction::Stopped);
    }
    if !*requested {
        println!(
            "MAILBOX_STOP_REQUESTED attempt_id={running_attempt} notice_id={} — Coordinator 가 이 시도를 폐기했다. 소유자 \"계속\" 도 무시하고 멈춘다",
            notice.notice_id
        );
    }
    *requested = true;
    // 서명된 갱신 거부와 같은 경로 — 끊김 시한을 지금으로 당겨 감시 스레드가 멈춘다(DISCONNECT_STOPPED · 소유자 되찾음이 아니다).
    panel.renew_refused(running_attempt, SystemClock.now_unix_ms());
    None
}

/// 실행 중 우편함 감시 스레드. `stop` 이 서면(실행이 끝나면) 끝난다 — 그때 정지를 요청했고 정지가 성공했으면 마지막으로 한 번 더 물어 STOPPED 로 답한다.
pub(crate) fn spawn_watch_during_execution(
    config: AgentConfig,
    signing_key: SigningKey,
    running_attempt: String,
    interval: std::time::Duration,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut requested = false;
        let once = |requested: &mut bool| {
            match session_once(&config, &signing_key, &mut |notice| {
                decide_while_running(&config, &running_attempt, notice, requested)
            }) {
                Ok(MailboxRound::Empty) => {}
                Ok(round) => println!("MAILBOX_WATCH attempt_id={running_attempt} result={round:?}"),
                Err(error) => println!(
                    "MAILBOX_WATCH_FAILED attempt_id={running_attempt} detail={error} — 다음 주기에 다시"
                ),
            }
        };
        'watch: loop {
            // 정지를 요청한 뒤에는 짧게 다시 묻는다 — 멈춘 것을 곧바로 STOPPED 로 알린다.
            let wait = if requested {
                std::time::Duration::from_millis(200)
            } else {
                interval
            };
            let due = std::time::Instant::now() + wait;
            while std::time::Instant::now() < due {
                if stop.load(std::sync::atomic::Ordering::SeqCst) {
                    break 'watch;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            once(&mut requested);
        }
        if requested
            && config
                .owner_panel_state
                .stopped_for_disconnect(&running_attempt)
        {
            once(&mut requested);
        }
    })
}
