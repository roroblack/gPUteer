//! 작업 → Agent 진행 보고(2026-10-01 · 끊겼을 때 판단표 ④ "곧 끝남" 의 선행 조각).
//!
//! # 작업과의 약속(워크로드 계약)
//!
//! ```text
//! GPUTEER_PROGRESS_FILE   작업이 진행을 적는 파일(체크포인트 폴더 안 `progress` — 컨테이너에서는 /gputeer/checkpoints/progress).
//!                         한 줄에 하나씩 `이름=정수`. current_step 은 **반드시**, 나머지는 알 때만 적는다(모르면 빼고 지어내지 않는다 — CLAUDE.md §1):
//!                           current_step=120          (필수)
//!                           total_steps=1000          (1 이상)
//!                           eta_seconds=360
//!                           last_committed_step=100   (current_step 이하)
//!                         ★ 다 쓴 뒤 그 이름으로 바꾸기를 권한다(예: progress.tmp 에 쓰고 progress 로 rename) — 반쯤 쓴 파일은
//!                           형식 오류로 버려진다(다음 번에 다시 읽는다)
//! ```
//!
//! # 이 값의 신뢰 수준
//!
//! **작업의 자기보고**(`WORKER_REPORTED`)다 — 위조할 수 있다. 화면 · 로그에 그 출처를 붙인다(CLAUDE.md §1 "관측값에 출처를 붙인다").
//! Agent 는 이 값을 갱신 요청(`RenewLeaseRequest.progress` — 이미 서명 대상인 칸)에 실어 보낼 뿐, 이것으로 정지 · 계속을 정하지 않는다.
//! "곧 끝나니 계속" 판정(판단표 ④)은 Coordinator 가 유예(grace)를 서명해 알려 줄 때에야 뜻이 생긴다 — 그 전에는 끊김 시한과 재배치
//! 시각이 같아 판정할 것이 없다(docs/plans/2026-09-30_1239_끝을_못본_작업_자동정리_합의.md).
//!
//! # 엄격하게 읽는다
//!
//! 모르는 이름 · 정수가 아닌 값 · 같은 이름 두 번 · current_step 없음 · total_steps 0 · `current_step > total_steps` ·
//! `last_committed_step > current_step` · 너무 큰 파일은 **형식 오류**로 버린다. 오타 하나로 값이 조용히 빠지면 "보고가 없다" 와 구별되지 않는다.
//!
//! # 작업이 만든 파일을 읽는다 — 그래서 방어한다(검수 pr1 · pr2)
//!
//! ★ **판정은 연 핸들 자체로 한다** — 경로로 미리 본 값은 빨리 거르는 데만 쓰고, 읽을지 말지는 실제로 읽을 그 핸들의 속성으로 정한다.
//!   그래서 미리 본 뒤 · 열기 전에 다른 것으로 바꿔치기돼도, 판정한 그 객체만 읽는다(검수 pr2 — 윈도에서 경로로 본 파일과 연 파일이
//!   같은지 대조하지 않던 것을 이렇게 닫았다).
//!
//! ```text
//! 링크          따라가지 않는다. 리눅스 O_NOFOLLOW · 윈도 FILE_FLAG_OPEN_REPARSE_POINT 로 열고, 연 핸들이 링크 · 재분석 지점이면 거부
//! 하드 링크     연 핸들의 링크 수가 2 이상이면 거부. ★ 그것만으로는 모자란다(검수 pr3) — 연 뒤에 작업이 `progress` 이름을 지우면
//!               바깥 파일의 링크 수가 1 로 줄어 통과한다. 그래서 **연 뒤에 그 이름을 다시 보고**(`path_still_names`) — 그 이름이 연 핸들과
//!               같은 파일(리눅스 dev · inode, 윈도 볼륨 번호 · 파일 ID)이고 · 링크가 아니고 · 이름이 하나뿐이어야 읽는다. 그 순간 그 파일의
//!               유일한 이름이 체크포인트 폴더 안에 있다 — 작업은 폴더 밖에 새 이름을 만들 수 없으니, 그 뒤에 바뀌어도 바깥 파일이 되지 않는다
//! 특수 파일     연 핸들이 보통 파일이 아니면 거부. 리눅스는 O_NONBLOCK 으로 열어 FIFO 에 멈추지 않는다
//! 크기          연 파일에서 상한 + 1 바이트까지만 읽는다(미리 본 크기를 믿지 않는다)
//! 시한          그래도 멈추는 경우(느린 파일 시스템 등)에 대비해 읽기는 시한(`READ_TIMEOUT`) 안에서만 기다린다. 넘기면 이 시도의 진행
//!               읽기를 끈다. 멈춘 읽기 스레드는 프로세스 전체에서 `MAX_STUCK_READS` 개까지만 둔다 — 그만큼 멈춰 있으면 새로 띄우지
//!               않고 바로 시한 초과로 답한다(Agent 가 오래 돌며 시도마다 하나씩 쌓이지 않게 — 검수 pr2)
//! 내용 노출     오류 메시지에 파일 내용(이름 · 값)을 싣지 않는다 — 줄 번호와 이유만. 서명된 요청에는 파싱한 정수만 간다
//! 못 막는 것    부모 폴더 쪽의 재분석 지점 — 체크포인트 폴더는 Agent 가 만들고, 컨테이너에는 그 폴더만 마운트된다.
//!               바깥 이름이 (작업이 아니라) 호스트 쪽에서 지워져 이름이 체크포인트 폴더 안에만 남은 파일 — 그 파일은 읽는다.
//!               ★ 이 방어는 작업이 폴더 밖 이름을 지우거나 만들 수 없다는 전제(컨테이너 마운트 격리)에 기댄다. 호스트에서 바로 도는 작업은
//!               Agent 와 같은 사용자 권한이라 그 파일을 스스로 읽을 수 있다 — 이 방어가 더 막아 주는 것이 없다
//! ```

use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use gputeer_protocol::pb;

/// 체크포인트 폴더 안 진행 파일 이름. 게시기는 `step-<숫자>` 만 보므로 이 파일을 건드리지 않는다.
pub const PROGRESS_FILENAME: &str = "progress";

/// 진행 파일 크기 상한(바이트). 정책값이다 — 네 줄이면 100 바이트도 안 된다.
pub const PROGRESS_MAX_BYTES: u64 = 4096;

/// 진행 파일 하나를 읽는 데 기다리는 시한. 보통 파일은 밀리초다 — 넘기면 특수 파일로 바꿔치기된 것으로 본다.
pub const READ_TIMEOUT: Duration = Duration::from_secs(1);

/// 시한을 넘겨 멈춰 있는 읽기 스레드를 프로세스 전체에서 몇 개까지 둘지. 정책값이다 — 넘으면 새로 띄우지 않는다.
pub const MAX_STUCK_READS: usize = 4;

/// 지금 돌고 있는(아직 안 끝난) 읽기 스레드 수 — 시한을 넘겨 멈춘 것도 끝날 때까지 센다.
static READS_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// 작업이 적은 진행(자기보고). 적지 않은 값은 `None` 이다 — 0 으로 지어내지 않는다.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkloadProgress {
    pub current_step: u64,
    pub total_steps: Option<u64>,
    pub eta_seconds: Option<u64>,
    pub last_committed_step: Option<u64>,
}

impl WorkloadProgress {
    /// 갱신 요청에 싣는 꼴.
    ///
    /// ★ proto 의 칸은 "없음" 을 따로 담지 못한다 — 서명 규칙상 0 은 칸이 빠진 것과 같다(규칙 b). 그래서 **0 은 "보고하지 않음"** 으로
    ///   읽어야 한다(CLAUDE.md §1 의 "미상이면 0" 과 같은 약속). 복제 백로그는 작업이 알 수 없어 늘 0(= 모름)이다 — "백로그 없음" 이 아니다.
    pub fn to_report(&self) -> pb::ProgressReport {
        pb::ProgressReport {
            current_step: self.current_step,
            total_steps: self.total_steps.unwrap_or(0),
            eta_seconds: self.eta_seconds.unwrap_or(0),
            last_committed_step: self.last_committed_step.unwrap_or(0),
            replication_backlog_bytes: 0,
        }
    }
}

/// 진행 파일을 읽은 결과.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgressRead {
    /// 파일이 없다 — 작업이 진행을 보고하지 않는다(정상).
    Absent,
    /// 있지만 받을 수 없다 — 이유(파일 내용은 담지 않는다).
    Malformed(String),
    /// 읽기가 시한 안에 끝나지 않았다 — 특수 파일로 바꿔치기된 것으로 보고 이 시도의 진행 읽기를 끈다.
    TimedOut,
    Read(WorkloadProgress),
}

/// 진행 파일 내용을 읽는다(순수 함수). 오류에는 줄 번호와 이유만 싣는다.
pub fn parse(text: &str) -> Result<WorkloadProgress, String> {
    let mut current_step = None;
    let mut total_steps = None;
    let mut eta_seconds = None;
    let mut last_committed_step = None;
    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once('=')
            .ok_or_else(|| format!("{line_no}번째 줄에 '=' 가 없다"))?;
        let number: u64 = value
            .trim()
            .parse()
            .map_err(|_| format!("{line_no}번째 줄의 값이 0 이상의 정수가 아니다"))?;
        let slot = match name.trim() {
            "current_step" => &mut current_step,
            "total_steps" => &mut total_steps,
            "eta_seconds" => &mut eta_seconds,
            "last_committed_step" => &mut last_committed_step,
            _ => {
                return Err(format!(
                    "{line_no}번째 줄의 이름을 모른다 — current_step · total_steps · eta_seconds · last_committed_step 만 받는다"
                ))
            }
        };
        if slot.replace(number).is_some() {
            return Err(format!("{line_no}번째 줄 — 같은 이름이 두 번 있다"));
        }
    }
    let current_step = current_step.ok_or("current_step 이 없다(필수)")?;
    if total_steps == Some(0) {
        return Err("total_steps 가 0 이다 — 1 이상이어야 한다(모르면 빼라)".into());
    }
    if total_steps.is_some_and(|total| current_step > total) {
        return Err("current_step 이 total_steps 보다 크다".into());
    }
    if last_committed_step.is_some_and(|committed| committed > current_step) {
        return Err("last_committed_step 이 current_step 보다 크다".into());
    }
    Ok(WorkloadProgress {
        current_step,
        total_steps,
        eta_seconds,
        last_committed_step,
    })
}

/// 진행 파일을 읽는다 — 링크를 따라가지 않고, 상한까지만, 시한 안에서만(모듈 문서 "방어한다").
pub fn read(path: &Path) -> ProgressRead {
    read_bounded(path, READ_TIMEOUT, &READS_IN_FLIGHT, read_now)
}

/// `read` 의 몸통 — 시한 · 스레드 수 세기 · 읽는 함수를 바꿔 끼울 수 있게(시험이 프로세스 전체 셈을 건드리지 않게) 뗐다.
fn read_bounded(
    path: &Path,
    timeout: Duration,
    in_flight: &'static AtomicUsize,
    reader: fn(&Path) -> ProgressRead,
) -> ProgressRead {
    // 정상 읽기는 밀리초에 끝나 셈이 곧 0 으로 돌아온다 — 상한까지 차 있으면 그만큼 멈춰 있다는 뜻이다.
    if in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_STUCK_READS {
        in_flight.fetch_sub(1, Ordering::SeqCst);
        return ProgressRead::TimedOut;
    }
    let path = path.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("gputeer-progress-read".into())
        .spawn(move || {
            // 읽는 함수가 패닉해도 셈은 돌려놓는다 — 안 그러면 멈춘 것이 없는데 상한이 찬다.
            let _done = InFlight(in_flight);
            let result = reader(&path);
            // 받는 쪽이 시한으로 떠났으면 보낼 곳이 없다 — 그 결과는 버린다(이미 TimedOut 으로 알렸다).
            if tx.send(result).is_err() {
                eprintln!("gputeer-progress-read: 시한 뒤에 끝난 읽기 결과를 버린다");
            }
        });
    if let Err(error) = spawned {
        // 띄우지 못했으면 클로저(와 그 안의 셈 돌려놓기)는 만들어지지도 않았다 — 여기서 돌려놓는다.
        in_flight.fetch_sub(1, Ordering::SeqCst);
        return ProgressRead::Malformed(format!("읽기 스레드를 띄우지 못했다: {error}"));
    }
    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => ProgressRead::TimedOut,
        // 결과 없이 끝났다(읽는 함수 패닉) — 멈춘 것이 아니므로 시한 초과로 세지 않는다.
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            ProgressRead::Malformed("읽기가 결과 없이 끝났다".into())
        }
    }
}

/// 읽기 스레드가 끝날 때(정상 · 패닉 모두) 셈을 하나 돌려놓는다.
struct InFlight(&'static AtomicUsize);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn read_now(path: &Path) -> ProgressRead {
    read_now_with(path, || {})
}

/// `read_now` 의 몸통 — `after_open` 은 연 직후에 부른다(시험이 "연 뒤 이름 지우기" 를 실제 읽기 경로에 끼워 넣는 자리).
fn read_now_with(path: &Path, after_open: impl FnOnce()) -> ProgressRead {
    let before = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return ProgressRead::Absent,
        Err(error) => return ProgressRead::Malformed(format!("읽을 수 없다({:?})", error.kind())),
    };
    if before.file_type().is_symlink() {
        return ProgressRead::Malformed("링크다 — 따라가지 않는다".into());
    }
    if !before.is_file() {
        return ProgressRead::Malformed("보통 파일이 아니다".into());
    }
    let file = match open_without_following(path) {
        Ok(file) => file,
        Err(error) => return ProgressRead::Malformed(format!("열 수 없다({:?})", error.kind())),
    };
    after_open();
    // ★ 여기부터의 판정은 연 핸들 자체로 한다 — 읽을 그 객체다(모듈 문서 "판정은 연 핸들 자체로").
    if let Some(why) = refuse_opened(&file) {
        return ProgressRead::Malformed(why);
    }
    // ★ 검수 pr3 — 연 뒤 이름을 지워 링크 수를 1 로 줄이는 경우: 그 이름이 아직 이 파일의 유일한 이름인지 본다.
    if let Some(why) = path_still_names(&file, path) {
        return ProgressRead::Malformed(why);
    }
    let mut bytes = Vec::new();
    if let Err(error) = file.take(PROGRESS_MAX_BYTES + 1).read_to_end(&mut bytes) {
        return ProgressRead::Malformed(format!("읽을 수 없다({:?})", error.kind()));
    }
    if bytes.len() as u64 > PROGRESS_MAX_BYTES {
        return ProgressRead::Malformed(format!("상한 {PROGRESS_MAX_BYTES} 바이트를 넘는다"));
    }
    match String::from_utf8(bytes) {
        Ok(text) => match parse(&text) {
            Ok(progress) => ProgressRead::Read(progress),
            Err(why) => ProgressRead::Malformed(why),
        },
        Err(_) => ProgressRead::Malformed("UTF-8 이 아니다".into()),
    }
}

#[cfg(windows)]
fn open_without_following(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
    // 재분석 지점(symlink · junction)을 따라가지 않고 그 자체를 연다 — 연 것이 재분석 지점이면 `refuse_opened` 가 거부한다.
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(unix)]
fn open_without_following(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    // O_NOFOLLOW — 마지막 이름이 링크면 열기가 실패한다(ELOOP). O_NONBLOCK — FIFO 를 열어도 멈추지 않는다(보통 파일 읽기에는 영향 없다).
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(any(unix, windows)))]
fn open_without_following(_path: &Path) -> std::io::Result<std::fs::File> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "이 플랫폼에서는 링크를 따라가지 않는 열기를 확인할 수 없어 읽지 않는다",
    ))
}

/// 연 핸들을 읽어도 되는가 — 보통 파일이고 · 링크가 아니고 · 이름이 하나뿐이어야 한다. 거부 사유를 돌려준다.
#[cfg(unix)]
fn refuse_opened(file: &std::fs::File) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) => return Some(format!("연 파일을 볼 수 없다({:?})", error.kind())),
    };
    if !metadata.file_type().is_file() {
        return Some("연 것이 보통 파일이 아니다".into());
    }
    (metadata.nlink() > 1)
        .then(|| "하드 링크다(이름이 둘 이상) — 다른 파일일 수 있어 읽지 않는다".into())
}

#[cfg(windows)]
fn refuse_opened(file: &std::fs::File) -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DEVICE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    };
    let info = match handle_info(file) {
        Ok(info) => info,
        Err(error) => return Some(format!("연 파일을 볼 수 없다({:?})", error.kind())),
    };
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Some("연 것이 재분석 지점이다 — 링크를 따라가지 않는다".into());
    }
    if info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_DEVICE) != 0 {
        return Some("연 것이 보통 파일이 아니다".into());
    }
    (info.nNumberOfLinks > 1)
        .then(|| "하드 링크다(이름이 둘 이상) — 다른 파일일 수 있어 읽지 않는다".into())
}

#[cfg(not(any(unix, windows)))]
fn refuse_opened(_file: &std::fs::File) -> Option<String> {
    Some("이 플랫폼에서는 연 파일을 확인할 수 없어 읽지 않는다".into())
}

/// 연 뒤에 그 이름을 다시 본다 — 같은 파일이고 · 링크가 아니고 · 이름이 하나뿐이어야 한다(모듈 문서 "하드 링크"). 거부 사유를 돌려준다.
#[cfg(unix)]
fn path_still_names(file: &std::fs::File, path: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let opened = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) => return Some(format!("연 파일을 볼 수 없다({:?})", error.kind())),
    };
    let named = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return Some("연 뒤에 이름이 사라졌다 — 다른 파일일 수 있어 읽지 않는다".into()),
    };
    if named.file_type().is_symlink() || named.dev() != opened.dev() || named.ino() != opened.ino()
    {
        return Some("연 뒤에 이름이 다른 것을 가리킨다 — 읽지 않는다".into());
    }
    (named.nlink() != 1)
        .then(|| "하드 링크다(이름이 둘 이상) — 다른 파일일 수 있어 읽지 않는다".into())
}

#[cfg(windows)]
fn path_still_names(file: &std::fs::File, path: &Path) -> Option<String> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };
    let opened = match handle_info(file) {
        Ok(info) => info,
        Err(error) => return Some(format!("연 파일을 볼 수 없다({:?})", error.kind())),
    };
    // 이름으로 한 번 더 연다 — 내용 접근 없이(속성만) · 링크를 따라가지 않고.
    let again = std::fs::OpenOptions::new()
        .access_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path);
    let named = match again.as_ref().map(handle_info) {
        Ok(Ok(info)) => info,
        _ => return Some("연 뒤에 이름이 사라졌다 — 다른 파일일 수 있어 읽지 않는다".into()),
    };
    if named.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || named.dwVolumeSerialNumber != opened.dwVolumeSerialNumber
        || named.nFileIndexHigh != opened.nFileIndexHigh
        || named.nFileIndexLow != opened.nFileIndexLow
    {
        return Some("연 뒤에 이름이 다른 것을 가리킨다 — 읽지 않는다".into());
    }
    (named.nNumberOfLinks != 1)
        .then(|| "하드 링크다(이름이 둘 이상) — 다른 파일일 수 있어 읽지 않는다".into())
}

#[cfg(windows)]
fn handle_info(
    file: &std::fs::File,
) -> std::io::Result<windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    // SAFETY: `file` 이 살아 있는 동안의 유효한 핸들이고, 출력 구조체는 이 함수 안의 지역 변수다.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(info)
}

#[cfg(not(any(unix, windows)))]
fn path_still_names(_file: &std::fs::File, _path: &Path) -> Option<String> {
    Some("이 플랫폼에서는 이름을 다시 확인할 수 없어 읽지 않는다".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_well_formed_progress_file_is_read_and_unreported_values_stay_unknown() {
        let progress =
            parse("current_step=120\r\ntotal_steps=1000\n eta_seconds = 360 \n").unwrap();
        assert_eq!(
            progress,
            WorkloadProgress {
                current_step: 120,
                total_steps: Some(1000),
                eta_seconds: Some(360),
                last_committed_step: None,
            }
        );
        let report = progress.to_report();
        assert_eq!(
            (report.last_committed_step, report.replication_backlog_bytes),
            (0, 0)
        );
        assert_eq!(
            parse("current_step=0").unwrap().total_steps,
            None,
            "적지 않은 값을 지어냈다"
        );
    }

    #[test]
    fn malformed_files_are_refused_without_echoing_their_contents() {
        for (text, why) in [
            ("current_step=1\nsecret_token=7", "이름을 모른다"),
            ("current_step=hunter2", "정수가 아니다"),
            ("current_step=-1", "정수가 아니다"),
            ("current_step=1\ncurrent_step=2", "두 번"),
            ("total_steps=10", "current_step 이 없다"),
            ("current_step=1\ntotal_steps=0", "1 이상"),
            ("current_step=11\ntotal_steps=10", "보다 크다"),
            ("current_step=5\nlast_committed_step=6", "보다 크다"),
            ("current_step", "'='"),
            ("\n \n", "current_step 이 없다"),
        ] {
            let error = parse(text).expect_err(text);
            assert!(error.contains(why), "{text:?} → {error}");
            assert!(
                !error.contains("secret_token") && !error.contains("hunter2"),
                "오류에 파일 내용이 실렸다: {error}"
            );
        }
    }

    #[test]
    fn an_absent_file_is_absent_and_an_oversized_or_non_regular_one_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PROGRESS_FILENAME);
        assert_eq!(read(&path), ProgressRead::Absent);
        std::fs::write(&path, "current_step=3\ntotal_steps=10\n").unwrap();
        assert!(matches!(read(&path), ProgressRead::Read(p) if p.current_step == 3));
        std::fs::write(&path, "x".repeat(PROGRESS_MAX_BYTES as usize + 1)).unwrap();
        assert!(matches!(read(&path), ProgressRead::Malformed(why) if why.contains("상한")));
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(matches!(read(&path), ProgressRead::Malformed(why) if why.contains("보통 파일")));
    }

    /// ★ 검수 pr1 — 진행 파일이 다른 파일을 가리키는 링크면 따라가지 않는다(호스트 파일을 읽어 싣지 않는다).
    #[test]
    fn a_progress_file_that_is_a_link_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("host-file");
        std::fs::write(&outside, "current_step=42\n").unwrap();
        let link = dir.path().join(PROGRESS_FILENAME);
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&outside, &link);
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&outside, &link);
        if let Err(error) = made {
            // 윈도는 개발자 모드 · 권한이 없으면 링크를 못 만든다 — 그 환경에서는 재지 못한다(통과로 세지 않는다고 적는다).
            eprintln!("ENVIRONMENT-BLOCKED: 링크를 만들 수 없어 링크 거부를 재지 않았다: {error}");
            return;
        }
        match read(&link) {
            ProgressRead::Malformed(why) => assert!(why.contains("링크"), "{why}"),
            other => panic!("링크를 따라가 읽었다: {other:?}"),
        }
        // ★ 검수 pr2 — 미리 보기를 건너뛰고 연 핸들만으로도 거부한다(미리 본 뒤 링크로 바꿔치기된 경우와 같다)
        let opened = open_without_following(&link);
        let refused = match &opened {
            Ok(file) => refuse_opened(file),
            Err(_) => Some("열기 자체가 거부됐다".into()),
        };
        assert!(refused.is_some(), "연 핸들이 링크인데 받아들였다");
    }

    /// ★ 검수 pr2 — 다른 파일에 걸어 둔 하드 링크는 읽지 않는다(호스트 파일의 정수를 싣지 않는다).
    #[test]
    fn a_progress_file_with_another_name_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("host-file");
        std::fs::write(&outside, "current_step=42\n").unwrap();
        let path = dir.path().join(PROGRESS_FILENAME);
        std::fs::hard_link(&outside, &path).unwrap();
        match read(&path) {
            ProgressRead::Malformed(why) => assert!(why.contains("하드 링크"), "{why}"),
            other => panic!("하드 링크를 읽었다: {other:?}"),
        }
        // 이름이 하나로 돌아오면 읽는다(거부가 링크 수 때문임을 확인)
        std::fs::remove_file(&outside).unwrap();
        assert!(matches!(read(&path), ProgressRead::Read(p) if p.current_step == 42));
    }

    /// ★ 검수 pr3 — 하드 링크를 연 뒤 작업이 그 이름을 지우면 링크 수가 1 로 줄어 첫 검사는 통과한다. 이름을 다시 보는 검사가 막는다.
    #[test]
    fn removing_the_name_after_open_does_not_let_another_file_through() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("host-file");
        std::fs::write(&outside, "current_step=42\n").unwrap();
        let path = dir.path().join(PROGRESS_FILENAME);
        std::fs::hard_link(&outside, &path).unwrap();
        let file = open_without_following(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            refuse_opened(&file),
            None,
            "전제: 이름을 지우면 링크 수 검사만으로는 통과한다"
        );
        assert!(
            path_still_names(&file, &path).is_some_and(|why| why.contains("사라졌다")),
            "이름이 사라진 파일을 읽으려 했다"
        );
        // 지운 뒤 같은 바깥 파일에 다시 걸면 — 이름은 같은 파일을 가리키지만 이름이 둘이다
        std::fs::hard_link(&outside, &path).unwrap();
        assert!(
            path_still_names(&file, &path).is_some_and(|why| why.contains("하드 링크")),
            "다시 건 하드 링크를 받아들였다"
        );
        // 지운 뒤 다른 보통 파일을 그 이름에 두면 — 다른 파일이다
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "current_step=1\n").unwrap();
        assert!(
            path_still_names(&file, &path).is_some_and(|why| why.contains("다른 것")),
            "다른 파일로 바뀐 이름을 받아들였다"
        );
        // 대조군 — 작업의 보통 파일은 그대로 읽는다
        let own = open_without_following(&path).unwrap();
        assert_eq!(path_still_names(&own, &path), None);

        // 실제 읽기 경로 — 연 직후에 이름을 지우면 바깥 파일의 정수를 싣지 않는다(읽기 경로에서 이 검사를 빼면 여기서 실패한다)
        std::fs::remove_file(&path).unwrap();
        std::fs::hard_link(&outside, &path).unwrap();
        let removed = read_now_with(&path, || std::fs::remove_file(&path).unwrap());
        assert!(
            matches!(&removed, ProgressRead::Malformed(why) if why.contains("사라졌다")),
            "연 뒤 이름을 지운 바깥 파일을 읽었다: {removed:?}"
        );
    }

    /// ★ 검수 pr2 — FIFO 로 바꿔치기해도 열기에서 멈추지 않고 바로 거부한다.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_fifo_is_refused_without_blocking() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PROGRESS_FILENAME);
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: 널로 끝나는 유효한 경로 문자열이다.
        assert_eq!(
            unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) },
            0,
            "FIFO 를 못 만들었다"
        );
        let started = std::time::Instant::now();
        // 미리 보기에서 걸러지지 않게 바로 읽는 함수로 — 열기 자체가 멈추지 않는지 본다
        let refused = refuse_opened(&open_without_following(&path).expect("FIFO 열기가 실패했다"));
        assert!(refused.is_some_and(|why| why.contains("보통 파일")));
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "FIFO 열기에서 멈췄다"
        );
    }

    static PANIC_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

    fn panicking_reader(_path: &Path) -> ProgressRead {
        panic!("읽는 함수가 패닉했다(시험)");
    }

    /// 읽는 함수가 패닉해도 셈은 돌아온다 — 멈춘 것 없이 상한이 차지 않는다.
    #[test]
    fn a_panicking_read_gives_its_slot_back() {
        for _ in 0..MAX_STUCK_READS + 2 {
            assert_eq!(
                read_bounded(
                    Path::new("unused"),
                    Duration::from_secs(5),
                    &PANIC_IN_FLIGHT,
                    panicking_reader
                ),
                ProgressRead::Malformed("읽기가 결과 없이 끝났다".into())
            );
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while PANIC_IN_FLIGHT.load(Ordering::SeqCst) != 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "패닉한 읽기가 셈을 돌려놓지 않았다"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    static TEST_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
    static RELEASE: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
    static RELEASED: std::sync::Condvar = std::sync::Condvar::new();

    fn stuck_reader(_path: &Path) -> ProgressRead {
        let mut released = RELEASE.lock().unwrap();
        while !*released {
            released = RELEASED.wait(released).unwrap();
        }
        ProgressRead::Absent
    }

    /// ★ 검수 pr2 — 멈춘 읽기 스레드는 프로세스 전체에서 상한까지만 쌓인다. 상한이면 새로 띄우지 않고 바로 시한 초과로 답한다.
    #[test]
    fn stuck_reads_are_capped_for_the_whole_process() {
        let path = Path::new("unused");
        let short = Duration::from_millis(20);
        for _ in 0..MAX_STUCK_READS {
            assert_eq!(
                read_bounded(path, short, &TEST_IN_FLIGHT, stuck_reader),
                ProgressRead::TimedOut
            );
        }
        assert_eq!(TEST_IN_FLIGHT.load(Ordering::SeqCst), MAX_STUCK_READS);
        let started = std::time::Instant::now();
        let long = Duration::from_secs(30);
        assert_eq!(
            read_bounded(path, long, &TEST_IN_FLIGHT, stuck_reader),
            ProgressRead::TimedOut,
            "상한에서 새 스레드를 띄웠다"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "상한에서 시한까지 기다렸다"
        );
        assert_eq!(TEST_IN_FLIGHT.load(Ordering::SeqCst), MAX_STUCK_READS);
        // 멈춘 것이 풀리면 셈이 돌아오고 다시 읽는다
        *RELEASE.lock().unwrap() = true;
        RELEASED.notify_all();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while TEST_IN_FLIGHT.load(Ordering::SeqCst) != 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "풀린 스레드가 셈을 돌려놓지 않았다"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            read_bounded(path, long, &TEST_IN_FLIGHT, stuck_reader),
            ProgressRead::Absent
        );
    }
}
