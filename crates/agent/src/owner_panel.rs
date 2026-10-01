//! Owner Panel — 노드 소유자가 자기 GPU 를 되찾는 로컬 화면.
//!
//! # 이것이 `CLAUDE.md` §0.1 그 자체다
//!
//! §0.1 은 다른 어떤 규칙보다 앞에 이렇게 정한다.
//!
//! ```text
//! 노드 소유자는 언제든 자기 GPU 를 즉시 비울 수 있어야 한다.
//! 네트워크가 끊겨도, quorum 이 없어도, Coordinator 가 죽어도 동작해야 한다.
//! 이것을 원격 서비스에 의존하게 만들지 않는다.
//!
//! 소유자 통제 UI(Owner Panel)는 로컬 Agent 가 제공한다. 127.0.0.1 바인딩 고정.
//! 소유자 화면에는 누가 · 어느 Job 을 · 언제부터 · 얼마나 돌리는지 항상 보인다.
//! 강제 종료 시 손실 범위를 미리 계산해 보여준다.
//! ```
//!
//! 그래서 이 모듈은 **아무 원격 의존이 없다.** Coordinator 에 묻지 않고,
//! 서명을 검증하지 않으며, 네트워크가 죽어도 그대로 돈다. 알고 있는
//! 사실을 로컬 메모리에서 읽어 보여주고, 정지 손잡이를 부를 뿐이다.
//!
//! # 왜 HTTP 라이브러리를 안 쓰는가
//!
//! 이 워크스페이스에는 HTTP 서버 크레이트가 없고, 이 화면 하나 때문에
//! 의존성을 늘리면 그만큼 남의 PC 에서 도는 코드가 늘어난다. 필요한 건
//! `GET` 두 개와 `POST` 하나뿐이라 직접 처리한다.
//!
//! # 로컬 서버라고 안전한 게 아니다
//!
//! ★ `127.0.0.1` 에만 바인딩해도 **브라우저로 열린 아무 웹페이지가**
//!   이 서버에 요청을 보낼 수 있다. 그러면 남의 사이트가 소유자 몰래
//!   작업을 정지시킬 수 있다. 그래서 세 겹으로 막는다.
//!
//! ```text
//! 1  127.0.0.1 바인딩         외부 인터페이스에 절대 안 붙는다
//! 2  Host 헤더 검사           DNS 리바인딩 차단 — 이름으로 들어온 요청은 거부
//! 3  정지에 토큰 필요         기동 시 찍은 토큰을 커스텀 헤더로 요구.
//!                             커스텀 헤더는 브라우저가 preflight 를 거치고
//!                             이 서버는 CORS 를 허용하지 않으므로 막힌다
//! ```
//!
//! # 이 모듈이 하지 않는 것
//!
//! ```text
//! 인증          토큰은 CSRF 방어지 신원 확인이 아니다. 이 기계에
//!               로그인한 다른 사용자는 토큰을 읽을 수 있다
//! TLS           로컬 루프백 전용이라 안 붙였다
//! 캐시 삭제     §0.5 의 데이터셋 삭제는 이 조각에 없다
//! 일시정지      §0.1 의 정지만 있다. PAUSE 는 상태 전이 계층이 필요하다
//! ```

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::exec::WorkloadStopper;

/// 지금 이 노드에서 도는 작업 하나.
///
/// ★ 필드가 §0.1 의 "누가 · 어느 Job 을 · 언제부터 · 얼마나" 에 1:1 로
///   대응한다. 하나라도 비면 소유자는 무엇을 멈추는지 모른 채 누른다.
pub struct RunningWorkload {
    /// 어느 Job 인가.
    pub job_id: String,
    /// 어느 시도인가. 같은 Job 이 여러 번 시도될 수 있다.
    pub attempt_id: String,
    /// **누가** 냈는가. 이것이 없으면 소유자는 자기 GPU 를 누가 쓰는지 모른다.
    pub submitter_device_id: String,
    /// 언제부터 도는가(밀리초).
    pub started_at_unix_ms: u64,
    /// 무엇을 돌리는가.
    pub entrypoint: String,
    /// 마지막으로 확정된 체크포인트. 손실 범위 계산의 기준점이다.
    ///
    /// `None` 은 **아직 하나도 확정 안 됐다**는 뜻이며, 그 경우 지금
    /// 멈추면 시작부터 지금까지 전부 잃는다.
    pub last_checkpoint_at_unix_ms: Option<u64>,
    /// 이 작업을 멈추는 손잡이. ★ 2026-09-30 — 일시정지 명령을 목록 잠금 밖에서 부르려고 나눠 쥔다(검수 pz1).
    pub stopper: std::sync::Arc<WorkloadStopper>,
}

/// ★ 2026-09-30 (검수 pz1) — 끊김 감시가 없는 작업의 일시정지 · 다시 시작을 거부하는 이유.
const NO_WATCH_REASON: &str = "이 작업은 중개 서버 연결을 감시하지 않는다(실행 중 갱신이 꺼진 Agent) — Lease 가 살아 있는지 알 수 없어 \
     다시 시작할 수 없으므로 일시정지를 받지 않는다. 비우려면 지금 멈춤이다";

/// ★ 2026-09-30 (사용자 승인 규범 · docs/plans/2026-09-30_1239_끝을_못본_작업_자동정리_합의.md) — 끊겼을 때 소유자가 고른 것.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerChoice {
    /// 고르지 않았다 — 판단표대로(지금 조각: 끊김 시한에 스스로 멈춘다).
    Auto,
    /// 끊김 시한을 넘겨도 계속 돌린다(부작용 없는 작업만 — `keep_running_allowed`).
    KeepRunning,
}

/// 작업 하나의 Coordinator 연결 상태 — 실행 중 갱신 스레드가 적고, 화면이 읽는다.
#[derive(Clone, Debug)]
pub struct ConnectionWatch {
    /// 마지막으로 갱신에 성공한 시각(밀리초). 아직 없으면 감시를 건 시각.
    pub last_renew_ok_unix_ms: u64,
    /// 이 시각이 지나면 스스로 멈춘다(끊김 시한 = Lease 만료 − 여유). 갱신에 성공할 때마다 뒤로 간다.
    pub self_stop_at_unix_ms: u64,
    /// 마지막 갱신 시도가 실패했다(연결 · 전송 실패 · 결과를 못 받음).
    pub disconnected: bool,
    /// Coordinator 가 갱신을 거부했다(대체 · 폐기 · 만료) — 다른 노드로 넘어갔다. 소유자 "계속" 도 무시하고 멈춘다.
    pub refused: bool,
    /// 소유자가 "계속" 을 고를 수 있는가 — 서명된 선언이 PURE · IDEMPOTENT 일 때만(부작용 두 번을 막을 장치가 없다).
    pub keep_running_allowed: bool,
    pub choice: OwnerChoice,
    /// 끊김 시한으로 스스로 멈췄다(정지 요청이 성공했다).
    pub self_stopped: bool,
    /// ★ 2026-09-30 (검수 ss1) — 스스로 멈추기로 **정했고** 정지 손잡이를 부르는 중이다. 이 동안(그리고 멈춘 뒤) "계속" 은 받지 않는다 —
    ///   전에는 정지를 정한 뒤 잠금을 놓은 사이에 "계속" 이 200 으로 받아들여지고도 작업이 멈췄다.
    pub stopping: bool,
}

/// 끊김 시한 — `기준 = min(요청을 보낸 시각 + Lease 길이, Lease 만료)`, `시한 = 기준 − min(여유, (기준 − 보낸 시각) / 2)`.
///
/// ★ Lease 길이(만료 − 발급)는 Coordinator 시계끼리의 차라 노드 시계와 섞이지 않는다. 요청을 **보낸** 시각부터 재야 응답 지연만큼
///   늦어지지 않는다. 만료 시각 자체와도 비교해(노드 시계가 맞다는 전제 — 런북 NTP) 둘 중 이른 쪽을 쓴다. 순수 함수다.
/// ★ 갱신된 Lease 의 발급 시각은 **처음 발급한 때 그대로**다(누적 상한용 — `coordinator/src/lease_store.rs`). 그래서 갱신 뒤에는
///   "만료 − 발급" 이 한 번 연장분보다 길고, 기준은 사실상 만료 시각이 된다.
/// ★ 여유는 남은 창(기준 − 보낸 시각)의 **절반까지만** 쓴다. 처음 구현은 여유를 그대로 빼, 연장분(예 7초)이 여유(기본 10초)보다 짧은 풀에서
///   **연결이 멀쩡한데도** 시작하자마자 멈췄다(`trusted_party_failover.rs` 가 잡았다). 절반으로 줄여도 시한은 만료 **전**이다 — Coordinator 는
///   만료 + 유예 뒤에야 다른 노드에 넘긴다.
pub fn disconnect_self_stop_at(
    lease_issued_at_unix_ms: u64,
    lease_expires_at_unix_ms: u64,
    request_sent_at_unix_ms: u64,
    margin_ms: u64,
) -> u64 {
    let length = lease_expires_at_unix_ms.saturating_sub(lease_issued_at_unix_ms);
    let base = request_sent_at_unix_ms
        .saturating_add(length)
        .min(lease_expires_at_unix_ms);
    let window = base.saturating_sub(request_sent_at_unix_ms);
    base.saturating_sub(margin_ms.min(window / 2))
}

/// 지금 멈추면 무엇을 잃는가.
///
/// ★ §0.1 은 "'최대 12분 진행 손실' 을 모른 채 누르게 하지 않는다" 고
///   요구한다. 그 문장을 만들기 위한 계산이다.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LossEstimate {
    /// 마지막 확정 지점 이후 흐른 시간(밀리초).
    pub lost_ms: u64,
    /// 확정된 체크포인트가 하나도 없는가.
    ///
    /// `true` 면 `lost_ms` 는 "체크포인트 이후" 가 아니라 **시작 이후
    /// 전부**다. 두 경우를 같은 숫자로 보여주면 소유자가 오해한다.
    pub nothing_committed_yet: bool,
}

/// 손실 범위를 계산한다. 순수 함수다 — 시계를 스스로 읽지 않는다.
///
/// # 왜 `now` 를 인자로 받는가
///
/// 안에서 시계를 읽으면 테스트가 시간에 의존하게 되고, 같은 입력에
/// 다른 답이 나올 수 있다. 이 저장소의 다른 순수 kernel 들과 같은 규칙이다.
///
/// # 시계가 거꾸로 간 경우
///
/// ★ `now` 가 기준 시각보다 이전이면 **음수 대신 0** 을 낸다(`saturating_sub`).
///   일반 뺄셈이었다면 overflow 검사 설정에 따라 panic 하거나 거대한 값으로
///   감싸돌 수 있다 — 감싸돌면 "3억 년 손실" 같은 숫자가 화면까지 갈 수 있다.
///   시계 보정·NTP 점프로 `now` 가 기준보다 이전이 되는 일은 일어날 수 있다
///   (재검수 32 — 전에는 감싸돌아 화면에 뜬다고만 적었다).
pub fn estimate_loss(workload: &RunningWorkload, now_unix_ms: u64) -> LossEstimate {
    let (baseline, nothing_committed_yet) = match workload.last_checkpoint_at_unix_ms {
        Some(at) => (at, false),
        None => (workload.started_at_unix_ms, true),
    };
    LossEstimate {
        lost_ms: now_unix_ms.saturating_sub(baseline),
        nothing_committed_yet,
    }
}

/// Owner Panel 이 보여주고 조작하는 대상.
///
/// `Arc<Mutex<..>>` 로 Agent 실행 스레드와 공유한다 — Agent 가 작업을
/// 시작하면 여기 넣고, 끝나면 뺀다.
#[derive(Clone)]
pub struct OwnerPanelState {
    inner: Arc<Mutex<BTreeMap<String, RunningWorkload>>>,
    /// ★ 2026-09-23 (신뢰망 남은 일 H) — **소유자가** 멈춘 시도. 종료 보고가 "실패" 가 아니라 "중단(선점)" 을 말하게 한다.
    ///   관측한 사실이다 — 정지 요청이 성공한 attempt 만 들어간다. 실행 계층은 누가 죽였는지 모르지만 이 패널은 안다.
    owner_stopped: Arc<Mutex<std::collections::BTreeSet<String>>>,
    /// ★ 2026-09-30 — 작업별 Coordinator 연결 상태(끊김 표시 · 스스로 멈춤 · 소유자 선택).
    connection: Arc<Mutex<BTreeMap<String, ConnectionWatch>>>,
    /// ★ 2026-09-30 — 끊김 시한으로 **스스로** 멈춘 시도. 연결 감시(`connection`)는 작업이 끝나면(`unregister`) 빠지지만 종료 보고는
    ///   그 **뒤에** 만들어진다 — 그래서 소유자 정지(`owner_stopped`)처럼 따로 남긴다. 처음 구현은 감시에서 읽어 보고가 FAILED 로 나갔다
    ///   (실제 프로세스 시험이 잡았다).
    disconnect_stopped: Arc<Mutex<std::collections::BTreeSet<String>>>,
    /// ★ 2026-10-01 (미리 알린 끊김 · signing.md §6.7) — 노드 단위 알림. 주인 화면에서 "N 분 뒤까지 끊긴다" 를 받는다.
    announcement: Arc<Mutex<Option<DisconnectAnnouncement>>>,
    /// 알림이 바뀔 때마다 올라간다 — 갱신 스레드가 보고 기다리지 않고 곧바로 갱신을 보낸다.
    announcement_generation: Arc<std::sync::atomic::AtomicU64>,
    /// ★ 2026-10-01 — 작업별 진행 자기보고(마지막으로 읽은 것 · 형식 오류).
    progress: Arc<Mutex<BTreeMap<String, ProgressView>>>,
}

/// ★ 2026-10-01 — 화면에 보이는 진행. **작업의 자기보고**(`WORKER_REPORTED`)다 — 위조할 수 있다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressView {
    /// 마지막으로 받아들인 진행. 형식 오류가 나도 지우지 않는다(그 전 값을 "n초 전" 으로 보인다).
    pub last: Option<crate::progress::WorkloadProgress>,
    pub last_read_at_unix_ms: u64,
    /// 마지막으로 읽었을 때의 형식 오류(없으면 `None`).
    pub error: Option<String>,
}

/// ★ 2026-10-01 — 노드 단위의 미리 알린 끊김.
///
/// 알림은 **요청**일 뿐이다 — 계속 돌 자격은 Coordinator 가 서명해 준 Lease 만료로만 생긴다(`granted_expires_at_unix_ms`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisconnectAnnouncement {
    /// "이 시각까지 Coordinator 와 연락이 안 될 것이다"(이 기계 시계 · 밀리초).
    pub until_unix_ms: u64,
    pub announced_at_unix_ms: u64,
    /// 이 알림을 실은 갱신이 받아들여져 받은 Lease 만료(서명된 결과). 아직 없으면 `None`.
    pub granted_expires_at_unix_ms: Option<u64>,
    /// 이 알림을 실은 마지막 갱신이 실패했다(연결 실패 · 옛 Coordinator 가 v2 를 거부 등).
    pub last_send_failed: bool,
}

/// 알림으로 받을 수 있는 가장 긴 시간(분). 정책값이다 — 실제로 얼마나 늘려 줄지는 Coordinator 운영자 상한이 정한다.
pub const ANNOUNCEMENT_MAX_MINUTES: u64 = 24 * 60;

impl Default for OwnerPanelState {
    fn default() -> Self {
        Self::new()
    }
}

impl OwnerPanelState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(BTreeMap::new())),
            owner_stopped: Arc::new(Mutex::new(std::collections::BTreeSet::new())),
            connection: Arc::new(Mutex::new(BTreeMap::new())),
            disconnect_stopped: Arc::new(Mutex::new(std::collections::BTreeSet::new())),
            announcement: Arc::new(Mutex::new(None)),
            announcement_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            progress: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// ★ 2026-10-01 — 작업의 진행 자기보고를 적는다(갱신 스레드가 갱신마다 부른다).
    pub fn record_progress(
        &self,
        attempt_id: &str,
        read: Result<crate::progress::WorkloadProgress, String>,
        now_unix_ms: u64,
    ) {
        let mut all = match self.progress.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let view = all.entry(attempt_id.to_string()).or_insert(ProgressView {
            last: None,
            last_read_at_unix_ms: 0,
            error: None,
        });
        match read {
            Ok(progress) => {
                view.last = Some(progress);
                view.last_read_at_unix_ms = now_unix_ms;
                view.error = None;
            }
            Err(why) => view.error = Some(why),
        }
    }

    fn progress_of(&self, attempt_id: &str) -> Option<ProgressView> {
        match self.progress.lock() {
            Ok(all) => all.get(attempt_id).cloned(),
            Err(poisoned) => poisoned.into_inner().get(attempt_id).cloned(),
        }
    }

    /// ★ 2026-10-01 — "지금부터 `minutes` 분 동안 끊긴다" 를 적는다. 0 이면 알림을 지운다. 상한(`ANNOUNCEMENT_MAX_MINUTES`)을 넘으면 거부.
    pub fn announce_disconnect(
        &self,
        minutes: u64,
        now_unix_ms: u64,
    ) -> Result<Option<u64>, String> {
        if minutes > ANNOUNCEMENT_MAX_MINUTES {
            return Err(format!(
                "{minutes} 분은 너무 길다 — 최대 {ANNOUNCEMENT_MAX_MINUTES} 분(실제로 얼마나 늘려 줄지는 중개 서버 운영자 상한이 정한다)"
            ));
        }
        let next = (minutes > 0).then(|| DisconnectAnnouncement {
            until_unix_ms: now_unix_ms.saturating_add(minutes.saturating_mul(60_000)),
            announced_at_unix_ms: now_unix_ms,
            granted_expires_at_unix_ms: None,
            last_send_failed: false,
        });
        let until = next.as_ref().map(|a| a.until_unix_ms);
        *self.announcement_lock() = next;
        self.announcement_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(until)
    }

    /// 다음 갱신에 실을 알림 시각. 없거나 이미 지났으면 0(= 싣지 않는다).
    pub fn announcement_to_send(&self, now_unix_ms: u64) -> u64 {
        self.announcement_lock()
            .as_ref()
            .map(|a| a.until_unix_ms)
            .filter(|until| *until > now_unix_ms)
            .unwrap_or(0)
    }

    /// 알림이 바뀐 횟수(갱신 스레드가 보고 곧바로 갱신한다).
    pub fn announcement_generation(&self) -> u64 {
        self.announcement_generation
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// 알림을 실은 갱신의 결과를 적는다 — 받아들여졌으면 서명된 Lease 만료, 아니면 실패.
    ///   ★ 알림 시각이 그 사이 바뀌었으면(다시 알림 · 취소) 옛 결과를 새 알림에 적지 않는다.
    pub fn announcement_sent(
        &self,
        sent_until_unix_ms: u64,
        granted_expires_at_unix_ms: Option<u64>,
    ) {
        if let Some(current) = self.announcement_lock().as_mut() {
            if current.until_unix_ms == sent_until_unix_ms {
                current.last_send_failed = granted_expires_at_unix_ms.is_none();
                if granted_expires_at_unix_ms.is_some() {
                    current.granted_expires_at_unix_ms = granted_expires_at_unix_ms;
                }
            }
        }
    }

    /// 지금의 알림.
    pub fn announcement(&self) -> Option<DisconnectAnnouncement> {
        self.announcement_lock().clone()
    }

    fn announcement_lock(&self) -> std::sync::MutexGuard<'_, Option<DisconnectAnnouncement>> {
        match self.announcement.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// 작업이 시작됐음을 등록한다. 키는 `attempt_id` 다.
    ///
    /// ★ `job_id` 가 아니라 `attempt_id` 를 키로 쓴다. 같은 Job 이 실패해
    ///   다시 시도되면 두 시도가 동시에 보일 수 있는데, `job_id` 를
    ///   키로 쓰면 뒤엣것이 앞엣것을 덮어써 **멈출 수 없는 작업이
    ///   생긴다**.
    pub fn register(&self, workload: RunningWorkload) {
        self.lock().insert(workload.attempt_id.clone(), workload);
    }

    /// 작업이 끝났음을 알린다.
    pub fn unregister(&self, attempt_id: &str) {
        self.lock().remove(attempt_id);
        self.connections().remove(attempt_id);
        match self.progress.lock() {
            Ok(mut all) => all.remove(attempt_id),
            Err(poisoned) => poisoned.into_inner().remove(attempt_id),
        };
    }

    /// 끊김 감시를 건다 — 실행 중 갱신 스레드가 시작할 때.
    pub fn watch_connection(
        &self,
        attempt_id: &str,
        self_stop_at_unix_ms: u64,
        keep_running_allowed: bool,
        now_unix_ms: u64,
    ) {
        self.connections().insert(
            attempt_id.to_string(),
            ConnectionWatch {
                last_renew_ok_unix_ms: now_unix_ms,
                self_stop_at_unix_ms,
                disconnected: false,
                refused: false,
                keep_running_allowed,
                choice: OwnerChoice::Auto,
                self_stopped: false,
                stopping: false,
            },
        );
    }

    /// 갱신 성공 — 연결됨 · 끊김 시한을 새 Lease 로 미룬다.
    pub fn renew_succeeded(&self, attempt_id: &str, self_stop_at_unix_ms: u64, now_unix_ms: u64) {
        if let Some(watch) = self.connections().get_mut(attempt_id) {
            watch.last_renew_ok_unix_ms = now_unix_ms;
            watch.self_stop_at_unix_ms = self_stop_at_unix_ms;
            watch.disconnected = false;
        }
    }

    /// 갱신 실패(연결 · 결과) — 끊김으로 표시한다. 시한은 그대로(마지막 성공한 Lease 의 것).
    pub fn renew_failed(&self, attempt_id: &str) {
        if let Some(watch) = self.connections().get_mut(attempt_id) {
            watch.disconnected = true;
        }
    }

    /// 갱신 거부(대체 · 폐기 · 만료) — 다른 노드로 넘어갔다. 곧바로 멈출 대상이 된다(소유자 "계속" 도 무시).
    pub fn renew_refused(&self, attempt_id: &str, now_unix_ms: u64) {
        if let Some(watch) = self.connections().get_mut(attempt_id) {
            watch.refused = true;
            watch.self_stop_at_unix_ms = watch.self_stop_at_unix_ms.min(now_unix_ms);
        }
    }

    /// 소유자가 "계속 돌리기" 를 골랐다. 부작용 있는 작업 · 이미 거부된 작업은 받지 않는다.
    pub fn keep_running(&self, attempt_id: &str) -> Result<(), String> {
        let mut connections = self.connections();
        let watch = connections
            .get_mut(attempt_id)
            .ok_or_else(|| format!("끊김 감시 중인 작업이 아니다: {attempt_id}"))?;
        if watch.refused {
            return Err("Coordinator 가 이 작업을 다른 노드로 넘겼다 — 계속 돌릴 수 없다".into());
        }
        if watch.stopping || watch.self_stopped {
            return Err(
                "끊김 시한이 지나 이미 멈추는 중이다(또는 멈췄다) — 계속 돌릴 수 없다".into(),
            );
        }
        if !watch.keep_running_allowed {
            return Err("부작용이 있거나 등급을 모르는 작업이다 — 두 번 실행을 막을 장치가 없어 계속 돌리기를 받지 않는다".into());
        }
        watch.choice = OwnerChoice::KeepRunning;
        Ok(())
    }

    /// 끊김 시한이 지났고 소유자가 "계속" 을 고르지 않았으면(또는 거부됐으면) 멈춘다. 멈출 때가 아니면 `None`.
    /// 정지가 **성공한 뒤에만** 멈췄다고 적는다 — 실패하면 다음 번에 다시 시도한다.
    pub fn self_stop_if_due(
        &self,
        attempt_id: &str,
        now_unix_ms: u64,
    ) -> Option<Result<(), String>> {
        // 결정과 "멈추는 중" 표시를 **한 잠금 안에서** 한다 — 그 뒤로 "계속" 은 거부된다(`keep_running`).
        let due = {
            let mut connections = self.connections();
            let watch = connections.get_mut(attempt_id)?;
            let due = !watch.self_stopped
                && !watch.stopping
                && now_unix_ms >= watch.self_stop_at_unix_ms
                && (watch.refused || watch.choice == OwnerChoice::Auto);
            if due {
                watch.stopping = true;
            }
            due
        };
        if !due {
            return None;
        }
        let result = {
            let guard = self.lock();
            match guard.get(attempt_id) {
                Some(workload) => workload.stopper.stop().map_err(|error| error.to_string()),
                None => Err(format!("그런 작업이 없다: {attempt_id}")),
            }
        };
        if let Some(watch) = self.connections().get_mut(attempt_id) {
            // 실패하면 "멈추는 중" 을 풀어 다음 확인 때 다시 정한다.
            watch.self_stopped = result.is_ok();
            watch.stopping = result.is_ok();
        }
        if result.is_ok() {
            match self.disconnect_stopped.lock() {
                Ok(mut stopped) => stopped.insert(attempt_id.to_string()),
                Err(poisoned) => poisoned.into_inner().insert(attempt_id.to_string()),
            };
        }
        Some(result)
    }

    /// 이 시도를 끊김 시한으로 스스로 멈췄나.
    ///   작업이 끝나 목록에서 빠진 뒤에도 답한다(종료 보고가 그 뒤에 만들어진다).
    pub fn stopped_for_disconnect(&self, attempt_id: &str) -> bool {
        match self.disconnect_stopped.lock() {
            Ok(stopped) => stopped.contains(attempt_id),
            Err(poisoned) => poisoned.into_inner().contains(attempt_id),
        }
    }

    fn connections(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, ConnectionWatch>> {
        match self.connection.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// 지금 도는 작업들을 사람이 읽는 요약으로 낸다.
    pub fn snapshot(&self, now_unix_ms: u64) -> Vec<WorkloadSummary> {
        let connections = self.connections().clone();
        self.lock()
            .values()
            .map(|workload| {
                let loss = estimate_loss(workload, now_unix_ms);
                WorkloadSummary {
                    job_id: workload.job_id.clone(),
                    attempt_id: workload.attempt_id.clone(),
                    submitter_device_id: workload.submitter_device_id.clone(),
                    entrypoint: workload.entrypoint.clone(),
                    started_at_unix_ms: workload.started_at_unix_ms,
                    running_ms: now_unix_ms.saturating_sub(workload.started_at_unix_ms),
                    loss,
                    connection: connections.get(&workload.attempt_id).cloned(),
                    paused: workload.stopper.is_paused(),
                    pause_state: workload.stopper.pause_state(),
                    progress: self.progress_of(&workload.attempt_id),
                    pause_unsupported_reason: workload.stopper.pause_support().err().or_else(
                        || {
                            (!connections.contains_key(&workload.attempt_id))
                                .then(|| NO_WATCH_REASON.to_string())
                        },
                    ),
                }
            })
            .collect()
    }

    /// 작업 하나를 멈춘다.
    ///
    /// 목록에서 빼지 않는다 — 실제로 끝났는지는 실행 스레드가
    /// `unregister()` 로 알려준다. 여기서 미리 빼면 정지가 실패했을 때
    /// 화면에서 사라져 소유자가 멈춘 줄 알게 된다.
    pub fn stop(&self, attempt_id: &str) -> Result<(), String> {
        let guard = self.lock();
        let workload = guard
            .get(attempt_id)
            .ok_or_else(|| format!("그런 작업이 없다: {attempt_id}"))?;
        workload.stopper.stop().map_err(|error| error.to_string())?;
        // 정지가 **성공한 뒤에만** 적는다 — 실패한 요청을 선점으로 보고하지 않는다.
        match self.owner_stopped.lock() {
            Ok(mut stopped) => {
                stopped.insert(attempt_id.to_string());
            }
            Err(poisoned) => {
                poisoned.into_inner().insert(attempt_id.to_string());
            }
        }
        Ok(())
    }

    /// ★ 2026-09-30 (소유자 "일시정지" — 합의 판단표 ②) — 작업을 얼린다. 검증된 수단(컨테이너 pause · 리눅스 cgroup freeze)에서만 되고,
    ///   얼리기가 **확인된** 뒤에만 성공이다. 윈도 호스트 실행은 이유와 함께 거부한다(화면은 버튼을 끄고 그 이유를 보인다).
    ///   ★ GPU 메모리는 쥔 채로 있다 — 비우려면 "지금 멈춤" 이다.
    ///   ★ 검수 pz1 — 끊김 감시가 없는 작업(실행 중 갱신을 안 켠 Agent)은 받지 않는다. Lease 가 살아 있는지 알 수 없어 **다시 시작할 수 없는**
    ///     일시정지가 된다. 그리고 목록 잠금을 쥔 채 런타임 명령(최대 10초)을 부르지 않는다 — 손잡이를 꺼내 놓고 부른다.
    pub fn pause(&self, attempt_id: &str) -> Result<(), String> {
        if !self.connections().contains_key(attempt_id) {
            return Err(NO_WATCH_REASON.into());
        }
        self.stopper_of(attempt_id)?.pause()
    }

    /// 얼린 작업을 푼다 — **새 Lease 없이 재개하지 않는다**(합의 판단표). 끊김 감시가 있고, 지금 연결이 살아 있고(마지막 갱신 성공)
    ///   끊김 시한 전이며 거부 · 스스로 멈춤이 없을 때만 푼다. 끊긴 채 풀면 다른 노드로 넘어갔을 수 있는 작업이 다시 돈다.
    ///   ★ 검수 pz1 — 끊김 감시가 없으면 거부한다(Lease 를 확인할 길이 없다). 판정은 **풀기 직전에 새 시각으로 한 번 더** 한다.
    ///   ★ 검수 pz2 — 그리고 **푼 직후에 또 한 번** 본다. 풀기 명령이 도는 사이(최대 10초) 시한이 지나면, 소유자가 "계속" 을 골라 둔 작업은 감시
    ///     스레드가 멈추지 않는다 — 새 Lease 없이 도는 것이다. 그러면 다시 얼리고, 얼리지 못하면 멈춘다(끊김 정지로 적는다). 남는 창은 풀기
    ///     명령이 도는 그 몇 초다.
    pub fn resume(&self, attempt_id: &str, clock: impl Fn() -> u64) -> Result<(), String> {
        self.resume_allowed(attempt_id, clock())?;
        let stopper = self.stopper_of(attempt_id)?;
        self.resume_allowed(attempt_id, clock())?;
        // ★ 검수 pz3 — 풀기가 **실패로 답해도** 사후 판정을 건너뛰지 않는다. 적용됐는데 확인만 못 했으면(상태 "모름") 작업이 돌고 있을 수 있다.
        //   조건이 살아 있으면 풀기 오류를 그대로 알리고(화면이 "확인하지 못했다" 를 보인다), 깨졌으면 되돌린다. 확실히 안 풀렸으면(Paused) 되돌릴 것이 없다.
        let resumed = stopper.resume();
        let why = match (resumed, self.resume_allowed(attempt_id, clock())) {
            (resumed, Ok(())) => return resumed,
            (Err(error), Err(_)) if stopper.pause_state() == crate::exec::PauseState::Paused => {
                return Err(error)
            }
            (_, Err(why)) => why,
        };
        match stopper.pause() {
            Ok(()) => Err(format!("다시 시작하는 사이 조건이 깨져 다시 얼렸다 — {why}")),
            Err(freeze) => match stopper.stop() {
                Ok(()) => {
                    match self.disconnect_stopped.lock() {
                        Ok(mut stopped) => stopped.insert(attempt_id.to_string()),
                        Err(poisoned) => poisoned.into_inner().insert(attempt_id.to_string()),
                    };
                    Err(format!(
                        "다시 시작하는 사이 조건이 깨졌는데 다시 얼리지 못해 멈췄다({freeze}) — {why}"
                    ))
                }
                Err(stop) => Err(format!(
                    "다시 시작하는 사이 조건이 깨졌는데 다시 얼리지도 멈추지도 못했다(얼리기: {freeze} · 멈추기: {stop}) — {why}"
                )),
            },
        }
    }

    fn resume_allowed(&self, attempt_id: &str, now_unix_ms: u64) -> Result<(), String> {
        let connections = self.connections();
        let watch = connections.get(attempt_id).ok_or(NO_WATCH_REASON)?;
        if watch.refused {
            return Err("Coordinator 가 이 작업을 다른 노드로 넘겼다 — 다시 시작할 수 없다(지금 멈춤으로 비운다)".into());
        }
        if watch.stopping || watch.self_stopped {
            return Err("끊김 시한이 지나 멈추는 중이다(또는 멈췄다) — 다시 시작할 수 없다".into());
        }
        if watch.disconnected {
            return Err("중개 서버와 끊겨 있다 — 다시 연결돼 갱신이 되면 풀 수 있다".into());
        }
        if now_unix_ms >= watch.self_stop_at_unix_ms {
            return Err("끊김 시한이 지났다 — 새 Lease 없이 다시 시작하지 않는다".into());
        }
        Ok(())
    }

    /// 목록에서 손잡이만 꺼낸다(잠금은 곧바로 놓는다).
    fn stopper_of(&self, attempt_id: &str) -> Result<std::sync::Arc<WorkloadStopper>, String> {
        self.lock()
            .get(attempt_id)
            .map(|workload| std::sync::Arc::clone(&workload.stopper))
            .ok_or_else(|| format!("그런 작업이 없다: {attempt_id}"))
    }

    /// 이 시도를 **소유자가** 멈췄나.
    pub fn stopped_by_owner(&self, attempt_id: &str) -> bool {
        match self.owner_stopped.lock() {
            Ok(stopped) => stopped.contains(attempt_id),
            Err(poisoned) => poisoned.into_inner().contains(attempt_id),
        }
    }

    /// ★ 락이 poisoned 여도 계속 간다.
    ///
    ///   어떤 스레드가 이 상태를 들고 panic 했다는 뜻인데, 그렇다고
    ///   Owner Panel 을 죽이면 **소유자가 자기 GPU 를 되찾을 방법이
    ///   사라진다.** §0.1 은 "Coordinator 가 죽어도 동작해야 한다" 고
    ///   했고, 같은 이유로 이 프로세스 안의 다른 스레드가 죽어도
    ///   동작해야 한다. 목록이 조금 이상해도 정지 버튼은 살아 있어야 한다.
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, RunningWorkload>> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// 화면에 보여줄 작업 하나.
#[derive(Clone, Debug)]
pub struct WorkloadSummary {
    pub job_id: String,
    pub attempt_id: String,
    pub submitter_device_id: String,
    pub entrypoint: String,
    pub started_at_unix_ms: u64,
    pub running_ms: u64,
    pub loss: LossEstimate,
    /// 끊김 감시가 걸린 작업이면 연결 상태(실행 중 갱신을 켠 Agent).
    pub connection: Option<ConnectionWatch>,
    /// ★ 2026-09-30 — 소유자가 얼려 두었나(얼리기가 확인된 뒤에만 true).
    pub paused: bool,
    /// 일시정지 상태 — 확인하지 못했으면 Unknown(검수 pz1).
    pub pause_state: crate::exec::PauseState,
    /// ★ 2026-10-01 — 작업의 진행 자기보고(없으면 `None`).
    pub progress: Option<ProgressView>,
    /// 얼릴 수 없으면 그 이유(윈도 호스트 실행 등). `None` 이면 일시정지 버튼을 쓸 수 있다.
    pub pause_unsupported_reason: Option<String>,
}

/// 로컬 전용 패널 서버.
pub struct OwnerPanel {
    listener: TcpListener,
    state: OwnerPanelState,
    /// 정지 요청에 요구하는 토큰. 기동 시 표준 출력에 찍는다.
    token: String,
}

impl OwnerPanel {
    /// `127.0.0.1` 에 바인딩한다. **이 주소는 인자가 아니다.**
    ///
    /// ★ 주소를 받게 만들면 언젠가 누가 `0.0.0.0` 을 넣는다. §0.1 은
    ///   "외부 인터페이스에 바인딩하지 않는다" 고 못박았으므로,
    ///   그걸 표현할 방법 자체를 두지 않는다.
    ///
    /// 포트 0 을 주면 OS 가 고른다 — `local_addr()` 로 확인한다.
    pub fn bind(port: u16, state: OwnerPanelState, token: String) -> std::io::Result<Self> {
        if token.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "빈 토큰으로는 시작하지 않는다 — 토큰 검사가 통과만 하게 된다",
            ));
        }
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        let listener = TcpListener::bind(addr)?;
        Ok(Self {
            listener,
            state,
            token,
        })
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// 요청 하나를 처리한다. 테스트가 한 걸음씩 몰기 위해 분리했다.
    pub fn serve_one(&self) -> std::io::Result<()> {
        let (stream, _peer) = self.listener.accept()?;
        // ★ 타임아웃을 반드시 건다. 없으면 헤더만 보내고 멈춘 상대
        //   하나가 패널 전체를 잠근다 — 그러면 소유자가 정지를 못 한다.
        //   `framed_ingress` 에서 이미 같은 교훈을 얻었다.
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        self.handle(stream)
    }

    /// 계속 받는다.
    pub fn serve_forever(&self) -> std::io::Result<()> {
        loop {
            // 요청 하나가 실패해도 패널을 죽이지 않는다 — 소유자의
            // 정지 수단이 사라지면 안 된다.
            if let Err(error) = self.serve_one() {
                eprintln!("owner-panel: 요청 처리 실패(계속 받는다): {error}");
            }
        }
    }

    fn handle(&self, mut stream: TcpStream) -> std::io::Result<()> {
        let request = match read_request(&mut stream) {
            Ok(request) => request,
            Err(message) => {
                return respond(&mut stream, 400, "text/plain; charset=utf-8", &message)
            }
        };

        // ★ Host 검사 — DNS 리바인딩 방어.
        //   공격자가 자기 도메인을 127.0.0.1 로 가리키게 만들면, 브라우저는
        //   그 도메인의 페이지로 이 서버에 붙을 수 있다. 그때 Host 헤더는
        //   그 도메인 이름이므로 여기서 걸린다.
        if !host_is_loopback(request.host.as_deref()) {
            return respond(
                &mut stream,
                403,
                "text/plain; charset=utf-8",
                "이 패널은 로컬에서만 쓴다(Host 가 loopback 이 아니다)",
            );
        }

        let now = now_unix_ms();
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/") => respond(&mut stream, 200, "text/html; charset=utf-8", &render_html()),
            ("GET", "/api/workloads") => respond(
                &mut stream,
                200,
                "application/json; charset=utf-8",
                &render_workloads_json(
                    &self.state.snapshot(now),
                    now,
                    &self.token,
                    self.state.announcement().as_ref(),
                ),
            ),
            ("POST", "/api/stop") => {
                // ★ 토큰을 **먼저** 본다. 어떤 작업을 멈추라는 요청인지
                //   읽기도 전에 막는다.
                if request.token.as_deref() != Some(self.token.as_str()) {
                    return respond(
                        &mut stream,
                        403,
                        "text/plain; charset=utf-8",
                        "토큰이 없거나 다르다 — 정지는 이 기계의 패널에서만 할 수 있다",
                    );
                }
                let attempt_id = request.body.trim();
                if attempt_id.is_empty() {
                    return respond(
                        &mut stream,
                        400,
                        "text/plain; charset=utf-8",
                        "멈출 attempt_id 가 없다",
                    );
                }
                match self.state.stop(attempt_id) {
                    Ok(()) => respond(
                        &mut stream,
                        200,
                        "text/plain; charset=utf-8",
                        &format!("STOP_REQUESTED attempt_id={attempt_id}"),
                    ),
                    // ★ 정지 실패를 200 으로 돌려주지 않는다. 소유자가
                    //   멈춘 줄 알면 안 된다.
                    Err(message) => respond(
                        &mut stream,
                        500,
                        "text/plain; charset=utf-8",
                        &format!("STOP_FAILED {message}"),
                    ),
                }
            }
            ("POST", "/api/keep-running") => {
                // ★ 정지와 같은 토큰 규칙 — 이 기계의 패널에서만.
                if request.token.as_deref() != Some(self.token.as_str()) {
                    return respond(
                        &mut stream,
                        403,
                        "text/plain; charset=utf-8",
                        "토큰이 없거나 다르다 — 이 기계의 패널에서만 고를 수 있다",
                    );
                }
                let attempt_id = request.body.trim();
                match self.state.keep_running(attempt_id) {
                    Ok(()) => respond(
                        &mut stream,
                        200,
                        "text/plain; charset=utf-8",
                        &format!("KEEP_RUNNING attempt_id={attempt_id}"),
                    ),
                    Err(message) => respond(
                        &mut stream,
                        409,
                        "text/plain; charset=utf-8",
                        &format!("KEEP_RUNNING_REFUSED {message}"),
                    ),
                }
            }
            ("POST", "/api/announce-disconnect") => {
                // ★ 2026-10-01 — 정지와 같은 토큰 규칙. 본문은 분(0 = 취소).
                if request.token.as_deref() != Some(self.token.as_str()) {
                    return respond(
                        &mut stream,
                        403,
                        "text/plain; charset=utf-8",
                        "토큰이 없거나 다르다 — 이 기계의 패널에서만 알릴 수 있다",
                    );
                }
                let Ok(minutes) = request.body.trim().parse::<u64>() else {
                    return respond(
                        &mut stream,
                        400,
                        "text/plain; charset=utf-8",
                        "몇 분인지 정수로 적는다(0 은 알림 취소)",
                    );
                };
                match self.state.announce_disconnect(minutes, now) {
                    Ok(Some(until)) => respond(
                        &mut stream,
                        200,
                        "text/plain; charset=utf-8",
                        &format!("ANNOUNCED unreachable_until_unix_ms={until}"),
                    ),
                    Ok(None) => respond(
                        &mut stream,
                        200,
                        "text/plain; charset=utf-8",
                        "ANNOUNCEMENT_CANCELLED",
                    ),
                    Err(message) => respond(
                        &mut stream,
                        400,
                        "text/plain; charset=utf-8",
                        &format!("ANNOUNCE_REFUSED {message}"),
                    ),
                }
            }
            ("POST", "/api/pause") | ("POST", "/api/resume") => {
                // ★ 정지와 같은 토큰 규칙 — 이 기계의 패널에서만.
                if request.token.as_deref() != Some(self.token.as_str()) {
                    return respond(
                        &mut stream,
                        403,
                        "text/plain; charset=utf-8",
                        "토큰이 없거나 다르다 — 이 기계의 패널에서만 고를 수 있다",
                    );
                }
                let attempt_id = request.body.trim();
                let pausing = request.path == "/api/pause";
                let result = if pausing {
                    self.state.pause(attempt_id)
                } else {
                    self.state.resume(attempt_id, now_unix_ms)
                };
                let verb = if pausing { "PAUSED" } else { "RESUMED" };
                match result {
                    Ok(()) => respond(
                        &mut stream,
                        200,
                        "text/plain; charset=utf-8",
                        &format!("{verb} attempt_id={attempt_id}"),
                    ),
                    // ★ 못 했으면 200 을 주지 않는다 — 얼린 줄 알고 자리를 비우면 안 된다.
                    Err(message) => respond(
                        &mut stream,
                        409,
                        "text/plain; charset=utf-8",
                        &format!("{verb}_REFUSED {message}"),
                    ),
                }
            }
            _ => respond(&mut stream, 404, "text/plain; charset=utf-8", "없는 경로다"),
        }
    }
}

struct Request {
    method: String,
    path: String,
    host: Option<String>,
    token: Option<String>,
    body: String,
}

/// 정지 요청에 요구하는 헤더 이름.
///
/// ★ **커스텀 헤더여야 한다.** 브라우저는 커스텀 헤더가 붙은 교차
///   출처 요청 앞에 preflight(OPTIONS)를 보내는데, 이 서버는 CORS 를
///   허용하지 않으므로 그 지점에서 막힌다. 쿠키나 폼 필드로 하면
///   그 방어가 없다.
const TOKEN_HEADER: &str = "x-gputeer-owner-token";

fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let mut reader = BufReader::new(stream);
    let mut start_line = String::new();
    reader
        .read_line(&mut start_line)
        .map_err(|error| format!("요청 줄을 읽지 못했다: {error}"))?;
    let mut parts = start_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    if method.is_empty() || path.is_empty() {
        return Err("요청 줄이 비었다".to_string());
    }

    let mut host = None;
    let mut token = None;
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| format!("헤더를 읽지 못했다: {error}"))?;
        if read == 0 || line.trim().is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        match name.as_str() {
            "host" => host = Some(value),
            TOKEN_HEADER => token = Some(value),
            "content-length" => {
                content_length = value
                    .parse()
                    .map_err(|_| format!("Content-Length 를 읽을 수 없다: {value:?}"))?;
            }
            _ => {}
        }
    }

    // ★ 본문 길이에 상한을 둔다. 없으면 상대가 거대한 Content-Length 를
    //   선언해 이 프로세스의 메모리를 먹는다.
    const MAX_BODY: usize = 4096;
    if content_length > MAX_BODY {
        return Err(format!("본문이 너무 크다({content_length} > {MAX_BODY})"));
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader
            .read_exact(&mut body)
            .map_err(|error| format!("본문을 읽지 못했다: {error}"))?;
    }

    Ok(Request {
        method,
        path,
        host,
        token,
        body: String::from_utf8_lossy(&body).to_string(),
    })
}

/// `Host` 헤더가 loopback authority 인가.
///
/// ★ **문법을 실제로 검사한다**(2026-08-29, 독립 검수 지적). 초안은
///   대괄호 뒤에 무엇이 오든 무시하고, 비대괄호도 첫 `:` 뒤를 전부
///   버렸다. 그래서 아래 같은 깨진 Host 가 전부 통과했다.
///
///   ```text
///   [::1                    닫는 괄호가 없다
///   [::1]evil.example       괄호 뒤에 다른 이름이 붙었다
///   localhost:not-a-port    포트가 숫자가 아니다
///   127.0.0.1:1:2           콜론이 두 개다
///   ```
///
///   표준 브라우저는 이런 Host 를 만들지 못하므로 실제 CSRF 우회는
///   아니었지만, "Host 를 파싱해 loopback 만 허용한다" 는 주장이
///   코드보다 컸다. 애매하면 거부한다.
///
/// `localhost` 도 허용한다 — 소유자가 주소창에 그렇게 친다. 그 이름은
/// hosts 파일에서 loopback 으로 고정되며, 공격자가 자기 도메인을
/// 127.0.0.1 로 가리켜도 그 **도메인 이름**이 Host 에 들어오므로 걸린다.
fn host_is_loopback(host: Option<&str>) -> bool {
    let Some(host) = host else {
        // Host 없는 HTTP/1.1 요청은 규격 위반이다. 통과시키지 않는다.
        return false;
    };
    let (name, port) = match host.strip_prefix('[') {
        // IPv6 리터럴: `[::1]` 또는 `[::1]:8765` 만 받는다.
        Some(rest) => match rest.split_once(']') {
            Some((inside, after)) => (inside, after),
            // 닫는 대괄호가 없다 — 문법 위반이다.
            None => return false,
        },
        None => match host.split_once(':') {
            Some((name, port)) => (name, port),
            None => (host, ""),
        },
    };

    // 대괄호 형태의 나머지는 비었거나 `:포트` 여야 한다.
    let port = if host.starts_with('[') {
        match port {
            "" => "",
            rest => match rest.strip_prefix(':') {
                Some(port) => port,
                // `]` 뒤에 콜론 없이 뭔가 붙었다.
                None => return false,
            },
        }
    } else {
        port
    };

    // 포트가 있으면 숫자여야 하고 u16 범위여야 한다.
    if !port.is_empty() && port.parse::<u16>().is_err() {
        return false;
    }

    matches!(
        name.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "::1"
    )
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    // ★ 캐시 금지 + 프레임 금지. 남의 페이지가 이 화면을 iframe 으로
    //   덮어 소유자가 다른 걸 누르게 만드는 클릭재킹을 막는다.
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         X-Frame-Options: DENY\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\r\n",
        body.as_bytes().len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// JSON 을 손으로 만든다 — 값은 전부 이스케이프한다.
fn render_workloads_json(
    items: &[WorkloadSummary],
    now_unix_ms: u64,
    token: &str,
    announcement: Option<&DisconnectAnnouncement>,
) -> String {
    let entries: Vec<String> = items
        .iter()
        .map(|item| {
            let connection = match &item.connection {
                None => "null".to_string(),
                Some(watch) => format!(
                    "{{\"disconnected\":{},\"refused\":{},\"since_last_ok_ms\":{},\"ms_to_self_stop\":{},\
                     \"keep_running_allowed\":{},\"keep_running\":{},\"self_stopped\":{}}}",
                    watch.disconnected,
                    watch.refused,
                    now_unix_ms.saturating_sub(watch.last_renew_ok_unix_ms),
                    watch.self_stop_at_unix_ms.saturating_sub(now_unix_ms),
                    watch.keep_running_allowed,
                    watch.choice == OwnerChoice::KeepRunning,
                    watch.self_stopped
                ),
            };
            format!(
                "{{\"job_id\":{},\"attempt_id\":{},\"submitter_device_id\":{},\
                 \"entrypoint\":{},\"started_at_unix_ms\":{},\"running_ms\":{},\
                 \"lost_ms\":{},\"nothing_committed_yet\":{},\"connection\":{},\
                 \"paused\":{},\"pause_state\":{},\"pause_supported\":{},\"pause_unsupported_reason\":{},\
                 \"progress\":{}}}",
                json_string(&item.job_id),
                json_string(&item.attempt_id),
                json_string(&item.submitter_device_id),
                json_string(&item.entrypoint),
                item.started_at_unix_ms,
                item.running_ms,
                item.loss.lost_ms,
                item.loss.nothing_committed_yet,
                connection,
                item.paused,
                json_string(match item.pause_state {
                    crate::exec::PauseState::Running => "running",
                    crate::exec::PauseState::Paused => "paused",
                    crate::exec::PauseState::Unknown => "unknown",
                }),
                item.pause_unsupported_reason.is_none(),
                match &item.pause_unsupported_reason {
                    Some(reason) => json_string(reason),
                    None => "null".to_string(),
                },
                // ★ 2026-10-01 — 진행은 작업의 자기보고다 — 출처를 함께 싣는다(CLAUDE.md §1).
                match &item.progress {
                    None => "null".to_string(),
                    Some(view) => format!(
                        "{{\"provenance\":\"WORKER_REPORTED\",\"last\":{},\"age_ms\":{},\"error\":{}}}",
                        match &view.last {
                            None => "null".to_string(),
                            Some(p) => format!(
                                "{{\"current_step\":{},\"total_steps\":{},\"eta_seconds\":{},\"last_committed_step\":{}}}",
                                p.current_step, p.total_steps, p.eta_seconds, p.last_committed_step
                            ),
                        },
                        if view.last.is_some() {
                            now_unix_ms.saturating_sub(view.last_read_at_unix_ms).to_string()
                        } else {
                            "null".to_string()
                        },
                        view.error.as_deref().map_or("null".to_string(), json_string)
                    ),
                }
            )
        })
        .collect();
    // ★ 2026-10-01 — 미리 알린 끊김(노드 단위). 받은 연장은 서명된 Lease 만료다.
    let announcement = match announcement {
        None => "null".to_string(),
        Some(a) => format!(
            "{{\"until_unix_ms\":{},\"ms_until\":{},\"granted_expires_at_unix_ms\":{},\"last_send_failed\":{}}}",
            a.until_unix_ms,
            a.until_unix_ms.saturating_sub(now_unix_ms),
            a.granted_expires_at_unix_ms
                .map_or("null".to_string(), |at| at.to_string()),
            a.last_send_failed
        ),
    };
    format!(
        "{{\"now_unix_ms\":{now_unix_ms},\"token\":{},\"announcement\":{announcement},\"workloads\":[{}]}}",
        json_string(token),
        entries.join(",")
    )
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn render_html() -> String {
    // 화면은 의도적으로 최소다. 목적은 예쁜 UI 가 아니라 §0.1 이
    // 요구하는 네 가지(누가·무엇을·언제부터·얼마나 잃는지)를 보이고
    // 멈추는 것이다.
    include_str!("owner_panel.html").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workload(started: u64, checkpoint: Option<u64>) -> RunningWorkload {
        RunningWorkload {
            job_id: "job".into(),
            attempt_id: "attempt".into(),
            submitter_device_id: "device".into(),
            started_at_unix_ms: started,
            entrypoint: "x".into(),
            last_checkpoint_at_unix_ms: checkpoint,
            stopper: std::sync::Arc::new(crate::exec::WorkloadStopper::for_test()),
        }
    }

    /// 체크포인트가 있으면 그 이후만 잃는다.
    #[test]
    fn loss_is_measured_from_the_last_checkpoint() {
        let estimate = estimate_loss(&workload(1_000, Some(5_000)), 8_000);
        assert_eq!(estimate.lost_ms, 3_000);
        assert!(!estimate.nothing_committed_yet);
    }

    /// 체크포인트가 없으면 시작부터 전부 잃는다.
    ///
    /// ★ 두 경우를 같은 숫자로만 보여주면 소유자가 "3초만 잃는구나" 로
    ///   오해한다. 확정된 게 하나도 없다는 사실을 따로 표시한다.
    #[test]
    fn with_no_checkpoint_everything_since_start_is_lost() {
        let estimate = estimate_loss(&workload(1_000, None), 8_000);
        assert_eq!(estimate.lost_ms, 7_000);
        assert!(
            estimate.nothing_committed_yet,
            "확정된 체크포인트가 없다는 사실이 표시되지 않는다"
        );
    }

    /// 시계가 거꾸로 가도 거대한 숫자가 나오지 않는다.
    ///
    /// ★ NTP 보정으로 실제로 일어난다. u64 뺄셈이 underflow 하면
    ///   "3억 년 손실" 이 화면에 뜬다.
    #[test]
    fn a_backwards_clock_does_not_produce_a_giant_number() {
        let estimate = estimate_loss(&workload(1_000, Some(9_000)), 5_000);
        assert_eq!(estimate.lost_ms, 0, "시계 역행이 underflow 로 감싸돌았다");
    }

    /// `Host` 검사가 실제로 갈라내는가.
    #[test]
    fn host_check_accepts_loopback_and_rejects_names() {
        for good in [
            "127.0.0.1",
            "127.0.0.1:8765",
            "localhost",
            "localhost:1",
            "[::1]:9",
        ] {
            assert!(host_is_loopback(Some(good)), "{good} 이 거부됐다");
        }
        // ★ 문법이 깨진 Host 도 전부 거부해야 한다. 초안은 이것들을
        //   전부 통과시켰다(2026-08-29 독립 검수가 하나하나 짚어줌).
        for bad in [
            "evil.example.com",
            "evil.example.com:8765",
            "0.0.0.0",
            "192.168.0.5",
            "[::1",
            "[::1]evil.example",
            "[127.0.0.1]evil.example",
            "localhost:not-a-port",
            "127.0.0.1:1:2",
            "127.0.0.1:99999",
        ] {
            assert!(!host_is_loopback(Some(bad)), "{bad} 이 통과했다");
        }
        assert!(!host_is_loopback(None), "Host 없는 요청이 통과했다");
    }

    /// 빈 토큰으로는 시작하지 않는다.
    ///
    /// 빈 토큰을 허용하면 헤더 없는 요청의 `None` 과 비교가 어긋나
    /// 검사가 사실상 통과만 하게 될 위험이 있다 — 아예 막는다.
    #[test]
    fn an_empty_token_is_refused_at_bind_time() {
        let error = match OwnerPanel::bind(0, OwnerPanelState::new(), String::new()) {
            Err(error) => error,
            Ok(_) => panic!("빈 토큰이 받아들여졌다"),
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    /// 같은 Job 의 두 시도가 서로를 덮지 않는가.
    ///
    /// ★ `job_id` 를 키로 썼다면 뒤엣것이 앞엣것을 덮어써 **멈출 수 없는
    ///   작업**이 생긴다.
    #[test]
    fn two_attempts_of_one_job_are_both_visible() {
        let state = OwnerPanelState::new();
        let mut first = workload(1_000, None);
        first.attempt_id = "attempt-1".into();
        let mut second = workload(2_000, None);
        second.attempt_id = "attempt-2".into();
        state.register(first);
        state.register(second);

        let snapshot = state.snapshot(3_000);
        assert_eq!(snapshot.len(), 2, "한 시도가 다른 시도를 덮어썼다");
    }

    /// 2026-09-30 — 끊김 시한 공식: 요청을 보낸 시각부터 Lease 길이만큼 · 만료 시각과 이른 쪽 · 여유를 뺀다.
    #[test]
    fn the_self_stop_time_is_the_earlier_of_sent_plus_length_and_expiry_minus_margin() {
        // Lease 60초(발급 1_000 → 만료 61_000), 요청은 1_500 에 보냄 → 61_500 과 61_000 중 61_000 − 여유 10_000
        assert_eq!(
            disconnect_self_stop_at(1_000, 61_000, 1_500, 10_000),
            51_000
        );
        // 노드 시계가 뒤처져 보낸 시각이 이르면 보낸 시각 + 길이 쪽이 이르다
        assert_eq!(disconnect_self_stop_at(1_000, 61_000, 500, 10_000), 50_500);
        // 여유가 남은 창보다 길면 창의 절반만 뺀다 — 연결이 멀쩡한데 곧바로 멈추지 않는다(창 4_000 → 2_000 뺌)
        assert_eq!(disconnect_self_stop_at(1_000, 5_000, 1_000, 10_000), 3_000);
        // 갱신된 Lease — 발급 시각은 처음 그대로(1_000), 만료 20_000, 보낸 시각 13_000 → 기준 20_000, 창 7_000 → 3_500 뺌
        assert_eq!(
            disconnect_self_stop_at(1_000, 20_000, 13_000, 10_000),
            16_500
        );
        // 이미 만료 뒤에 보냈으면 창이 0 — 기준(만료) 그대로
        assert_eq!(disconnect_self_stop_at(1_000, 5_000, 6_000, 10_000), 5_000);
    }

    /// 2026-09-30 — 끊김 시한이 지나면 스스로 멈춘다 · 갱신에 성공하면 시한이 미뤄진다 · 소유자 "계속" 은 부작용 없는 작업만 · 거부되면 "계속" 도 무시.
    #[test]
    fn the_watch_stops_on_time_honours_keep_running_only_for_safe_work_and_obeys_refusal() {
        let state = OwnerPanelState::new();
        let mut safe = workload(1_000, None);
        safe.attempt_id = "safe".into();
        let mut risky = workload(1_000, None);
        risky.attempt_id = "risky".into();
        state.register(safe);
        state.register(risky);
        state.watch_connection("safe", 10_000, true, 1_000);
        state.watch_connection("risky", 10_000, false, 1_000);
        // 시한 전에는 멈추지 않는다
        assert!(state.self_stop_if_due("safe", 9_999).is_none());
        // 갱신 성공 → 시한이 뒤로
        state.renew_succeeded("safe", 20_000, 9_000);
        assert!(state.self_stop_if_due("safe", 15_000).is_none());
        // 끊김 표시
        state.renew_failed("safe");
        let snapshot = state.snapshot(15_000);
        let safe_view = snapshot.iter().find(|s| s.attempt_id == "safe").unwrap();
        assert!(safe_view.connection.as_ref().unwrap().disconnected);
        // 부작용 있는 작업은 "계속" 을 받지 않는다
        assert!(state.keep_running("risky").is_err());
        // 부작용 없는 작업은 "계속" → 시한이 지나도 멈추지 않는다
        state.keep_running("safe").unwrap();
        assert!(state.self_stop_if_due("safe", 25_000).is_none());
        // 거부(다른 노드로 넘어감)면 "계속" 도 무시하고 멈추려 한다. ★ 시험용 손잡이는 성공을 돌려주지 않는다(멈춘 척하지 않는다 — exec::WorkloadStopper::for_test)
        //   → 정지를 **시도**했고 · 실패했으니 멈췄다고 적지 않고 · 다음 번에 다시 시도한다. 성공 경로는 실제 프로세스 시험이 본다.
        state.renew_refused("safe", 26_000);
        assert!(matches!(
            state.self_stop_if_due("safe", 26_000),
            Some(Err(_))
        ));
        assert!(
            !state.stopped_for_disconnect("safe"),
            "정지가 실패했는데 멈췄다고 적었다"
        );
        assert!(
            state.self_stop_if_due("safe", 27_000).is_some(),
            "실패한 정지를 다시 시도하지 않는다"
        );
        // 부작용 있는 작업은 시한에 멈추려 한다
        assert!(state.self_stop_if_due("risky", 10_000).is_some());
        // 거부된 뒤에는 "계속" 을 고를 수 없다
        assert!(state.keep_running("safe").is_err());
        // 끝나면 감시도 빠진다(멈추지 못한 시도는 멈췄다고 남지 않는다)
        state.unregister("risky");
        assert!(state.connections().get("risky").is_none());
        assert!(!state.stopped_for_disconnect("risky"));
    }

    /// ★ 검수 ss1 — 멈추기로 정한 뒤(멈추는 중 · 멈춤)에는 "계속" 을 받지 않는다. 정지가 실패하면 "멈추는 중" 이 풀려 다시 정한다.
    #[test]
    fn keep_running_is_refused_once_the_self_stop_has_been_decided() {
        let state = OwnerPanelState::new();
        let mut safe = workload(1_000, None);
        safe.attempt_id = "safe".into();
        state.register(safe);
        state.watch_connection("safe", 10_000, true, 1_000);
        // 시한이 지나 멈추기로 정했다 — 시험용 손잡이는 실패하므로 "멈추는 중" 이 풀린다
        assert!(matches!(
            state.self_stop_if_due("safe", 10_000),
            Some(Err(_))
        ));
        assert!(!state.connections().get("safe").unwrap().stopping);
        // 정지 손잡이를 부르는 사이(멈추는 중)에 온 "계속" 은 거부된다
        state.connections().get_mut("safe").unwrap().stopping = true;
        assert!(state.keep_running("safe").is_err());
        assert!(
            state.self_stop_if_due("safe", 11_000).is_none(),
            "멈추는 중에 또 정했다"
        );
        // 멈춘 뒤에도 거부된다
        {
            let mut connections = state.connections();
            let watch = connections.get_mut("safe").unwrap();
            watch.stopping = false;
            watch.self_stopped = true;
        }
        assert!(state.keep_running("safe").is_err());
    }

    /// ★ 2026-09-30 — 일시정지: 수단이 없으면(시험용 손잡이 · 윈도 호스트) 거부하고 얼렸다고 적지 않는다. 다시 시작은 끊김 · 거부 · 시한 뒤에는
    ///   거부된다(새 Lease 없이 재개하지 않는다). 목록 JSON 에 상태와 이유가 실린다.
    #[test]
    fn pause_is_refused_without_a_verified_means_and_resume_needs_a_live_lease() {
        let state = OwnerPanelState::new();
        let mut safe = workload(1_000, None);
        safe.attempt_id = "a".into();
        state.register(safe);
        // ★ 검수 pz1 — 끊김 감시가 없으면 일시정지 · 다시 시작 둘 다 거부(Lease 를 확인할 길이 없다) · 화면에 그 이유
        let unwatched = state
            .pause("a")
            .expect_err("감시 없는 작업의 일시정지를 받았다");
        assert!(unwatched.contains("감시하지 않는다"), "{unwatched}");
        assert!(
            state.resume("a", || 5_000).is_err(),
            "감시 없이 다시 시작했다"
        );
        assert!(state.snapshot(2_000)[0].pause_unsupported_reason.is_some());
        state.watch_connection("a", 10_000, true, 1_000);
        assert!(state.pause("a").is_err(), "시험용 손잡이는 얼리지 못한다");
        assert!(state.pause("없는").is_err());
        let snapshot = state.snapshot(2_000);
        assert!(!snapshot[0].paused, "못 얼렸는데 얼렸다고 적었다");
        let json = render_workloads_json(&snapshot, 2_000, "t", None);
        assert!(json.contains("\"paused\":false"), "{json}");
        #[cfg(windows)]
        assert!(
            json.contains("\"pause_supported\":false")
                && snapshot[0].pause_unsupported_reason.is_some(),
            "윈도 호스트 실행은 일시정지를 막고 이유를 보여야 한다: {json}"
        );
        // 끊김 감시 — 연결 정상 · 시한 전이면 확인을 통과해 손잡이까지 간다(얼리지 않았으면 그대로 성공)
        assert!(state.resume("a", || 5_000).is_ok());
        // 끊겼으면 거부
        state.renew_failed("a");
        assert!(state.resume("a", || 5_000).is_err());
        // 다시 이어졌지만 시한이 지났으면 거부
        state.renew_succeeded("a", 10_000, 6_000);
        assert!(state.resume("a", || 5_000).is_ok());
        assert!(state.resume("a", || 10_000).is_err());
        // ★ 검수 pz2 — 두 판정은 통과했는데 푼 **직후** 판정이 시한 뒤면 되돌린다(다시 얼리기 → 시험용 손잡이는 못 얼리므로 멈추기 → 그것도 실패).
        //   되돌리려 한 것 자체가 사후 판정이 있다는 증거다.
        let ticks = std::cell::Cell::new(0u64);
        let clock = || {
            ticks.set(ticks.get() + 1);
            if ticks.get() <= 2 {
                5_000
            } else {
                10_000
            }
        };
        let after = state
            .resume("a", clock)
            .expect_err("푼 직후 조건이 깨졌는데 그대로 뒀다");
        assert!(
            after.contains("다시 시작하는 사이 조건이 깨졌는데"),
            "{after}"
        );
        assert_eq!(ticks.get(), 3, "푼 직후 판정을 하지 않았다");
        // ★ 검수 pz3 — 풀기가 실패로 답해도(적용됐는데 확인 못 함 = 상태 "모름") 사후 판정은 한다 — 깨졌으면 되돌린다.
        state
            .stopper_of("a")
            .unwrap()
            .set_pause_state(crate::exec::PauseState::Unknown);
        let ticks = std::cell::Cell::new(0u64);
        let clock = || {
            ticks.set(ticks.get() + 1);
            if ticks.get() <= 2 {
                5_000
            } else {
                10_000
            }
        };
        let unknown = state
            .resume("a", clock)
            .expect_err("상태를 모르는 채 조건이 깨졌는데 그대로 뒀다");
        assert!(
            unknown.contains("다시 시작하는 사이 조건이 깨졌는데"),
            "{unknown}"
        );
        assert_eq!(
            ticks.get(),
            3,
            "풀기가 실패로 답하자 사후 판정을 건너뛰었다"
        );
        // ★ 검수 pz1 — 첫 판정은 시한 전이어도 풀기 **직전** 재판정이 시한 뒤면 거부(두 번째 시각을 본다)
        let ticks = std::cell::Cell::new(0u64);
        let clock = || {
            ticks.set(ticks.get() + 1);
            if ticks.get() == 1 {
                5_000
            } else {
                10_000
            }
        };
        assert!(
            state.resume("a", clock).is_err(),
            "풀기 직전 재판정을 하지 않았다"
        );
        // 거부(다른 노드로 넘어감)면 거부
        state.renew_refused("a", 7_000);
        assert!(state.resume("a", || 7_000).is_err());
    }

    /// ★ 2026-10-01 — 미리 알린 끊김: 분으로 받아 시각으로 적고, 지나면 싣지 않고, 결과는 **그 알림**에만 적는다 · 상한을 넘으면 거부 · 0 은 취소.
    #[test]
    fn a_disconnect_announcement_is_sent_until_it_passes_and_results_stick_to_it() {
        let state = OwnerPanelState::new();
        assert_eq!(state.announcement_to_send(1_000), 0, "알림 없이 실었다");
        let generation = state.announcement_generation();
        let until = state.announce_disconnect(30, 1_000).unwrap().unwrap();
        assert_eq!(until, 1_000 + 30 * 60_000);
        assert!(
            state.announcement_generation() > generation,
            "갱신 스레드를 깨우지 않는다"
        );
        assert_eq!(state.announcement_to_send(2_000), until);
        assert_eq!(state.announcement_to_send(until), 0, "지난 알림을 실었다");
        // 결과는 보낸 그 알림에만
        state.announcement_sent(until + 1, Some(9_999_999));
        assert_eq!(
            state.announcement().unwrap().granted_expires_at_unix_ms,
            None
        );
        state.announcement_sent(until, None);
        assert!(state.announcement().unwrap().last_send_failed);
        state.announcement_sent(until, Some(until));
        let a = state.announcement().unwrap();
        assert_eq!(a.granted_expires_at_unix_ms, Some(until));
        assert!(!a.last_send_failed);
        let json = render_workloads_json(&[], 2_000, "t", Some(&a));
        assert!(
            json.contains(&format!("\"granted_expires_at_unix_ms\":{until}")),
            "{json}"
        );
        // 상한 · 취소
        assert!(state
            .announce_disconnect(ANNOUNCEMENT_MAX_MINUTES + 1, 1_000)
            .is_err());
        assert_eq!(state.announce_disconnect(0, 1_000).unwrap(), None);
        assert!(state.announcement().is_none());
        assert_eq!(state.announcement_to_send(2_000), 0);
    }

    /// ★ 2026-10-01 — 진행 자기보고: 받은 값은 출처(WORKER_REPORTED)와 함께 실리고, 형식 오류는 앞 값을 지우지 않으며, 끝나면 빠진다.
    #[test]
    fn workload_progress_is_shown_with_its_provenance_and_keeps_the_last_good_value() {
        let state = OwnerPanelState::new();
        let mut w = workload(1_000, None);
        w.attempt_id = "a".into();
        state.register(w);
        let good = crate::progress::WorkloadProgress {
            current_step: 3,
            total_steps: 10,
            eta_seconds: 42,
            last_committed_step: 2,
        };
        state.record_progress("a", Ok(good.clone()), 5_000);
        state.record_progress("a", Err("모르는 이름".into()), 6_000);
        let snapshot = state.snapshot(7_000);
        let view = snapshot[0].progress.clone().expect("진행이 실려야 한다");
        assert_eq!(view.last, Some(good));
        assert_eq!(view.error.as_deref(), Some("모르는 이름"));
        let json = render_workloads_json(&snapshot, 7_000, "t", None);
        assert!(
            json.contains("\"provenance\":\"WORKER_REPORTED\""),
            "{json}"
        );
        assert!(
            json.contains("\"current_step\":3,\"total_steps\":10"),
            "{json}"
        );
        assert!(json.contains("\"age_ms\":2000"), "{json}");
        state.unregister("a");
        assert!(
            state.progress_of("a").is_none(),
            "끝난 작업의 진행이 남았다"
        );
    }

    /// JSON 이스케이프가 실제로 되는가.
    #[test]
    fn json_values_are_escaped() {
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }
}
