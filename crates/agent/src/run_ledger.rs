//! 노드 실행 원장 — 시도마다 시작 · 끝을 적는 SQLite 파일.
//!
//! 계획 `docs/plans/2026-09-29_0212_노드_실행원장_기존노드_이관_구현계획.md` · 상위 계약
//! `docs/contracts/proposals/2026-09-28_1034_실행여부불명_재배치보류_Lease_Attempt.md` §5.
//!
//! 이 모듈은 **파일 쪽만** 맡는다 — 형식 · 열기 검사(fail-closed) · 전이 규칙 · 시작 기록 식별 · 세대 짝 · 이관 파일 순서.
//! 루트 잠금 · 런타임 조회 · 사건 표식 · 보고 보관은 부르는 쪽(`lib.rs` · CLI)이 한다.
//!
//! ★ 알림 · ACK(FrameType 19 · 20) · OPEN · STOP_PENDING · ACKED 는 이 조각에 없다(다음 조각에서 migration 으로 더한다).

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// 원장 파일 이름 접미사 — `<루트>.run-ledger.sqlite3`.
pub const LEDGER_SUFFIX: &str = ".run-ledger.sqlite3";
/// 이관 진행 표식 접미사 — `<루트>.run-ledger.adopting`.
pub const ADOPTING_SUFFIX: &str = ".run-ledger.adopting";
/// 새 노드가 원장을 만드는 중이라는 표식 — `<루트>.run-ledger.creating`(코덱스 r1l ①: 세대 짝만 남은 상태가 "만들다 끊김" 인지 "쓰던 원장 유실" 인지 가른다).
pub const CREATING_SUFFIX: &str = ".run-ledger.creating";
/// 시작 기록 폴더 안의 세대 짝 파일 이름(`.` 으로 시작 — 시작 기록이 아니다).
pub const PAIR_NAME: &str = ".run-ledger-generation";
/// 원장 형식 버전(meta).
pub const FORMAT_VERSION: i64 = 1;
/// 행 형식 버전.
pub const ROW_FORMAT_VERSION: i64 = 1;

const WRITE_ONCE_LOCK_SUFFIX: &str = ".write_once.lock";
const WRITE_ONCE_TMP_SUFFIX: &str = ".tmp";

/// 원장의 행 상태. 이 조각은 셋뿐이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Active,
    Closed,
    LocalBlocked,
}

impl RowState {
    fn as_str(self) -> &'static str {
        match self {
            RowState::Active => "ACTIVE",
            RowState::Closed => "CLOSED",
            RowState::LocalBlocked => "LOCAL_BLOCKED",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        match s {
            "ACTIVE" => Some(RowState::Active),
            "CLOSED" => Some(RowState::Closed),
            "LOCAL_BLOCKED" => Some(RowState::LocalBlocked),
            _ => None,
        }
    }
}

/// 실행기 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Executor {
    Container,
    Host,
    /// 이관된 옛 시도 — 실행기를 모른다(시작 기록에는 attempt_id 한 줄뿐).
    Legacy,
}

impl Executor {
    fn as_str(self) -> &'static str {
        match self {
            Executor::Container => "container",
            Executor::Host => "host",
            Executor::Legacy => "legacy",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        match s {
            "container" => Some(Executor::Container),
            "host" => Some(Executor::Host),
            "legacy" => Some(Executor::Legacy),
            _ => None,
        }
    }
}

/// 행이 어디서 왔나.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// 이 Agent 가 실행하며 적었다.
    Agent,
    /// 이관 — 남은 컨테이너 조회를 거쳤다.
    LegacyAdopt,
    /// 이관 — 런타임 인자 없이 운영자 진술로(검증된 사실이 아니다).
    LegacyAdoptAttested,
}

impl Origin {
    fn as_str(self) -> &'static str {
        match self {
            Origin::Agent => "agent",
            Origin::LegacyAdopt => "legacy_adopt",
            Origin::LegacyAdoptAttested => "legacy_adopt_attested",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        match s {
            "agent" => Some(Origin::Agent),
            "legacy_adopt" => Some(Origin::LegacyAdopt),
            "legacy_adopt_attested" => Some(Origin::LegacyAdoptAttested),
            _ => None,
        }
    }
}

/// 원장 행 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptRow {
    pub attempt_id: String,
    pub origin: Origin,
    pub job_id: Option<String>,
    pub node_id: Option<String>,
    pub fence_epoch: Option<u64>,
    pub executor: Executor,
    pub runtime_program: Option<String>,
    pub runtime_kind: Option<String>,
    pub container_name: Option<String>,
    pub container_id: Option<String>,
    pub cgroup_path: Option<String>,
    pub state: RowState,
    pub reason: Option<String>,
    pub stopped: Option<bool>,
    pub logs_complete: Option<bool>,
    pub container_left: Option<bool>,
    /// 컨테이너를 지우고 새 조회로 "없음" 을 확인했는가. 컨테이너 행만 값이 있다(호스트 · legacy 는 None).
    pub container_removed: Option<bool>,
}

impl AttemptRow {
    /// 이 Agent 가 실행을 시작하기 직전의 행(ACTIVE).
    pub fn new_active(
        attempt_id: &str,
        job_id: &str,
        node_id: &str,
        fence_epoch: u64,
        executor: Executor,
    ) -> Self {
        AttemptRow {
            attempt_id: attempt_id.to_string(),
            origin: Origin::Agent,
            job_id: Some(job_id.to_string()),
            node_id: Some(node_id.to_string()),
            fence_epoch: Some(fence_epoch),
            executor,
            runtime_program: None,
            runtime_kind: None,
            container_name: None,
            container_id: None,
            cgroup_path: None,
            state: RowState::Active,
            reason: None,
            stopped: None,
            logs_complete: None,
            container_left: None,
            container_removed: (executor == Executor::Container).then_some(false),
        }
    }

    fn legacy(attempt_id: &str, origin: Origin) -> Self {
        AttemptRow {
            attempt_id: attempt_id.to_string(),
            origin,
            job_id: None,
            node_id: None,
            fence_epoch: None,
            executor: Executor::Legacy,
            runtime_program: None,
            runtime_kind: None,
            container_name: None,
            container_id: None,
            cgroup_path: None,
            state: RowState::Closed,
            reason: Some("legacy_adopt".into()),
            stopped: None,
            logs_complete: None,
            container_left: None,
            container_removed: None,
        }
    }
}

/// 원장 관련 경로 — 모두 **실경로로 정착한 체크포인트 루트** 기준이다.
#[derive(Debug, Clone)]
pub struct LedgerPaths {
    pub ledger: PathBuf,
    pub adopting: PathBuf,
    pub creating: PathBuf,
    pub started_dir: PathBuf,
}

impl LedgerPaths {
    /// `real_root` 는 잠금 · 실경로 정착을 마친 루트여야 한다.
    pub fn for_root(real_root: &Path) -> Result<Self, String> {
        Ok(LedgerPaths {
            ledger: crate::checkpoint_root_sibling(real_root, LEDGER_SUFFIX)?,
            adopting: crate::checkpoint_root_sibling(real_root, ADOPTING_SUFFIX)?,
            creating: crate::checkpoint_root_sibling(real_root, CREATING_SUFFIX)?,
            started_dir: crate::checkpoint_root_sibling(real_root, ".started-attempts")?,
        })
    }

    pub fn pair(&self) -> PathBuf {
        self.started_dir.join(PAIR_NAME)
    }

    fn parent_dir(&self) -> Result<&Path, String> {
        self.ledger
            .parent()
            .ok_or_else(|| format!("RUN_LEDGER: 원장 경로에 부모가 없다({:?})", self.ledger))
    }

    fn file_name(path: &Path) -> Result<String, String> {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
            .ok_or_else(|| format!("RUN_LEDGER: 경로 이름을 읽지 못했다({path:?})"))
    }

    fn temp_prefix(&self) -> Result<String, String> {
        Ok(format!("{}.tmp-", Self::file_name(&self.ledger)?))
    }
}

/// 루트에 원장 흔적이 있는가 — 스위치와 무관하게 기동 판단에 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Presence {
    pub ledger: bool,
    pub pair: bool,
    pub adopting: bool,
    pub creating: bool,
}

impl Presence {
    /// 한 번도 원장을 켜지 않은 루트 — 원장 · 짝 · 이관 표식 모두 없음.
    pub fn never_enabled(&self) -> bool {
        !self.ledger && !self.pair && !self.adopting && !self.creating
    }
}

fn exists_strict(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => Ok(true),
        Ok(_) => Err(format!(
            "RUN_LEDGER: 일반 파일이 아니다({path:?}) — 모르면 받지 않는다"
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("RUN_LEDGER: 확인하지 못했다({path:?}): {error}")),
    }
}

/// 원장 · 세대 짝 · 이관 표식이 있는지 본다. 확인하지 못하면 `Err`(모르면 받지 않는다).
pub fn detect(paths: &LedgerPaths) -> Result<Presence, String> {
    Ok(Presence {
        ledger: exists_strict(&paths.ledger)?,
        pair: exists_strict(&paths.pair())?,
        adopting: exists_strict(&paths.adopting)?,
        creating: exists_strict(&paths.creating)?,
    })
}

fn is_record_name(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_generation(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// 시작 기록 폴더를 읽어 attempt_id 집합을 돌려준다(계획 "시작 기록 식별").
///
/// - 시작 기록: 이름이 64자 소문자 hex · 내용이 정확히 `attempt_id=<id>\n` · blake3(<id>) 가 이름과 같다
/// - 세대 짝(`.run-ledger-generation`)은 기록이 아니다
/// - write_once 부산물(`<대상>.write_once.lock` · `<대상>.tmp`)은 대상이 기록 이름이거나 세대 짝일 때만 인정하고,
///   `remove_residue` 면 지우고 폴더를 sync 한다
/// - 그 밖의 파일 · 깨진 기록 · 이름과 해시가 다른 기록 · 하위 폴더 · 링크 → `Err`
///
/// 부르는 쪽이 루트 잠금을 쥐고 있어야 한다(같은 폴더에 쓰는 이가 없을 때만 부산물이 "죽은" 것이다).
pub fn scan_start_records(
    started_dir: &Path,
    remove_residue: bool,
) -> Result<BTreeSet<String>, String> {
    let entries = match fs::read_dir(started_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => {
            return Err(format!(
                "RUN_LEDGER: 시작 기록 폴더를 읽지 못했다({started_dir:?}): {error}"
            ))
        }
    };
    let mut ids = BTreeSet::new();
    let mut residue = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("RUN_LEDGER: 시작 기록 폴더 항목을 읽지 못했다: {error}"))?;
        let path = entry.path();
        let name = entry.file_name().into_string().map_err(|raw| {
            format!("RUN_LEDGER: 시작 기록 폴더에 UTF-8 아닌 이름이 있다({raw:?})")
        })?;
        let meta = fs::symlink_metadata(&path)
            .map_err(|error| format!("RUN_LEDGER: 확인하지 못했다({path:?}): {error}"))?;
        if !meta.file_type().is_file() {
            return Err(format!(
                "RUN_LEDGER: 시작 기록 폴더에 일반 파일이 아닌 것이 있다({path:?}) — 모르면 받지 않는다"
            ));
        }
        if name == PAIR_NAME {
            continue;
        }
        let residue_target = name
            .strip_suffix(WRITE_ONCE_LOCK_SUFFIX)
            .or_else(|| name.strip_suffix(WRITE_ONCE_TMP_SUFFIX));
        if let Some(target) = residue_target {
            if target == PAIR_NAME || is_record_name(target) {
                residue.push(path);
                continue;
            }
            return Err(format!(
                "RUN_LEDGER: 대상이 규칙에 맞지 않는 부산물이다({path:?})"
            ));
        }
        if !is_record_name(&name) {
            return Err(format!(
                "RUN_LEDGER: 시작 기록 폴더에 모르는 파일이 있다({path:?}) — 모르면 받지 않는다"
            ));
        }
        let body = fs::read(&path)
            .map_err(|error| format!("RUN_LEDGER: 시작 기록을 읽지 못했다({path:?}): {error}"))?;
        let text = std::str::from_utf8(&body)
            .map_err(|_| format!("RUN_LEDGER: 시작 기록이 UTF-8 이 아니다({path:?})"))?;
        let id = text
            .strip_prefix("attempt_id=")
            .and_then(|rest| rest.strip_suffix('\n'))
            .filter(|id| !id.is_empty() && !id.contains('\n'))
            .ok_or_else(|| format!("RUN_LEDGER: 시작 기록 내용이 깨졌다({path:?})"))?;
        if blake3::hash(id.as_bytes()).to_hex().as_str() != name {
            return Err(format!(
                "RUN_LEDGER: 시작 기록의 이름과 attempt_id 해시가 다르다({path:?})"
            ));
        }
        ids.insert(id.to_string());
    }
    if remove_residue && !residue.is_empty() {
        for path in &residue {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "RUN_LEDGER: 부산물을 지우지 못했다({path:?}): {error}"
                    ))
                }
            }
        }
        gputeer_checkpoint::sync_dir(started_dir)
            .map_err(|error| format!("RUN_LEDGER: 시작 기록 폴더를 sync 하지 못했다: {error:?}"))?;
    }
    Ok(ids)
}

fn read_generation_file(path: &Path) -> Result<String, String> {
    let value = fs::read_to_string(path)
        .map_err(|error| format!("RUN_LEDGER: 세대 값을 읽지 못했다({path:?}): {error}"))?;
    if !is_generation(&value) {
        return Err(format!(
            "RUN_LEDGER: 세대 값의 형식이 틀렸다({path:?}) — 32자 소문자 hex 여야 한다"
        ));
    }
    Ok(value)
}

fn new_generation() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| format!("RUN_LEDGER: 난수를 얻지 못했다: {error}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn write_once_value(dir: &Path, name: &str, value: &str) -> Result<(), String> {
    fs::create_dir_all(dir)
        .map_err(|error| format!("RUN_LEDGER: 폴더를 만들지 못했다({dir:?}): {error}"))?;
    gputeer_checkpoint::write_once(dir, name, value.as_bytes())
        .map(|_| ())
        .map_err(|error| format!("RUN_LEDGER: {name} 를 쓰지 못했다: {error:?}"))
}

const SCHEMA: &str = "
CREATE TABLE meta (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
CREATE TABLE attempts (
    attempt_id          TEXT PRIMARY KEY NOT NULL,
    row_format_version  INTEGER NOT NULL,
    origin              TEXT NOT NULL,
    job_id              TEXT,
    node_id             TEXT,
    fence_epoch         INTEGER,
    executor            TEXT NOT NULL,
    runtime_program     TEXT,
    runtime_kind        TEXT,
    container_name      TEXT,
    container_id        TEXT,
    cgroup_path         TEXT,
    state               TEXT NOT NULL,
    reason              TEXT,
    stopped             INTEGER,
    logs_complete       INTEGER,
    container_left      INTEGER,
    container_removed   INTEGER,
    created_at_unix_ms  INTEGER NOT NULL,
    updated_at_unix_ms  INTEGER NOT NULL
);
";

fn apply_pragmas(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("PRAGMA journal_mode = DELETE; PRAGMA synchronous = FULL;")
        .map_err(|error| format!("RUN_LEDGER: PRAGMA 실패: {error}"))
}

fn opt_bool(value: Option<bool>) -> Option<i64> {
    value.map(i64::from)
}

fn insert_row(conn: &Connection, row: &AttemptRow, now: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO attempts (attempt_id, row_format_version, origin, job_id, node_id, fence_epoch, executor,
            runtime_program, runtime_kind, container_name, container_id, cgroup_path, state, reason, stopped,
            logs_complete, container_left, container_removed, created_at_unix_ms, updated_at_unix_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?19)",
        params![
            row.attempt_id,
            ROW_FORMAT_VERSION,
            row.origin.as_str(),
            row.job_id,
            row.node_id,
            row.fence_epoch.map(|e| i64::try_from(e).unwrap_or(-1)),
            row.executor.as_str(),
            row.runtime_program,
            row.runtime_kind,
            row.container_name,
            row.container_id,
            row.cgroup_path,
            row.state.as_str(),
            row.reason,
            opt_bool(row.stopped),
            opt_bool(row.logs_complete),
            opt_bool(row.container_left),
            opt_bool(row.container_removed),
            now,
        ],
    )
}

fn remove_stale_temps(paths: &LedgerPaths) -> Result<(), String> {
    let parent = paths.parent_dir()?;
    let prefix = paths.temp_prefix()?;
    let entries = fs::read_dir(parent)
        .map_err(|error| format!("RUN_LEDGER: 폴더를 읽지 못했다({parent:?}): {error}"))?;
    let mut removed = false;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("RUN_LEDGER: 폴더 항목을 읽지 못했다: {error}"))?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with(&prefix) {
            fs::remove_file(entry.path()).map_err(|error| {
                format!("RUN_LEDGER: 옛 임시 원장을 지우지 못했다({name}): {error}")
            })?;
            removed = true;
        }
    }
    if removed {
        gputeer_checkpoint::sync_dir(parent)
            .map_err(|error| format!("RUN_LEDGER: 폴더 sync 실패: {error:?}"))?;
    }
    Ok(())
}

/// 임시 이름에 끝까지 쓰고 fsync → 덮지 않는 이름 바꾸기 → 폴더 sync.
///
/// 덮지 않는 이름 바꾸기는 하드 링크로 한다 — 대상이 이미 있으면 두 플랫폼 모두 실패한다(`std::fs::rename` 은 덮는다).
fn create_ledger_file(
    paths: &LedgerPaths,
    generation: &str,
    rows: &[AttemptRow],
) -> Result<(), String> {
    remove_stale_temps(paths)?;
    let parent = paths.parent_dir()?;
    let temp = parent.join(format!("{}{generation}", paths.temp_prefix()?));
    {
        let mut conn = Connection::open(&temp)
            .map_err(|error| format!("RUN_LEDGER: 임시 원장을 만들지 못했다({temp:?}): {error}"))?;
        apply_pragmas(&conn)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("RUN_LEDGER: 트랜잭션을 열지 못했다: {error}"))?;
        tx.execute_batch(SCHEMA)
            .map_err(|error| format!("RUN_LEDGER: 형식을 만들지 못했다: {error}"))?;
        let now = now_unix_ms();
        for (key, value) in [
            ("format_version", FORMAT_VERSION.to_string()),
            ("generation", generation.to_string()),
            ("created_at_unix_ms", now.to_string()),
        ] {
            tx.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .map_err(|error| format!("RUN_LEDGER: meta 를 쓰지 못했다: {error}"))?;
        }
        for row in rows {
            insert_row(&tx, row, now).map_err(|error| {
                format!("RUN_LEDGER: 행을 쓰지 못했다({}): {error}", row.attempt_id)
            })?;
        }
        tx.commit()
            .map_err(|error| format!("RUN_LEDGER: 커밋하지 못했다: {error}"))?;
        conn.close()
            .map_err(|(_, error)| format!("RUN_LEDGER: 임시 원장을 닫지 못했다: {error}"))?;
    }
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&temp)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("RUN_LEDGER: 임시 원장을 fsync 하지 못했다: {error}"))?;
    fs::hard_link(&temp, &paths.ledger).map_err(|error| {
        format!(
            "RUN_LEDGER: 원장을 제자리에 두지 못했다({:?}) — 이미 있으면 덮지 않는다: {error}",
            paths.ledger
        )
    })?;
    fs::remove_file(&temp)
        .map_err(|error| format!("RUN_LEDGER: 임시 원장을 지우지 못했다: {error}"))?;
    gputeer_checkpoint::sync_dir(parent)
        .map_err(|error| format!("RUN_LEDGER: 폴더 sync 실패: {error:?}"))?;
    Ok(())
}

/// 한 프로세스 안에서 기동 · 실행 흐름이 같이 쓰는 원장 손잡이.
pub type SharedRunLedger = std::sync::Arc<std::sync::Mutex<RunLedger>>;

/// 열린 원장.
#[derive(Debug)]
pub struct RunLedger {
    conn: Connection,
    generation: String,
}

fn row_from_sql(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
    Ok(RawRow {
        attempt_id: r.get(0)?,
        row_format_version: r.get(1)?,
        origin: r.get(2)?,
        job_id: r.get(3)?,
        node_id: r.get(4)?,
        fence_epoch: r.get(5)?,
        executor: r.get(6)?,
        runtime_program: r.get(7)?,
        runtime_kind: r.get(8)?,
        container_name: r.get(9)?,
        container_id: r.get(10)?,
        cgroup_path: r.get(11)?,
        state: r.get(12)?,
        reason: r.get(13)?,
        stopped: r.get(14)?,
        logs_complete: r.get(15)?,
        container_left: r.get(16)?,
        container_removed: r.get(17)?,
    })
}

const SELECT_ROW: &str = "SELECT attempt_id, row_format_version, origin, job_id, node_id, fence_epoch, executor,
    runtime_program, runtime_kind, container_name, container_id, cgroup_path, state, reason, stopped,
    logs_complete, container_left, container_removed FROM attempts";

struct RawRow {
    attempt_id: String,
    row_format_version: i64,
    origin: String,
    job_id: Option<String>,
    node_id: Option<String>,
    fence_epoch: Option<i64>,
    executor: String,
    runtime_program: Option<String>,
    runtime_kind: Option<String>,
    container_name: Option<String>,
    container_id: Option<String>,
    cgroup_path: Option<String>,
    state: String,
    reason: Option<String>,
    stopped: Option<i64>,
    logs_complete: Option<i64>,
    container_left: Option<i64>,
    container_removed: Option<i64>,
}

fn flag(name: &str, id: &str, value: Option<i64>) -> Result<Option<bool>, String> {
    match value {
        None => Ok(None),
        Some(0) => Ok(Some(false)),
        Some(1) => Ok(Some(true)),
        Some(other) => Err(format!(
            "RUN_LEDGER: 행 {id} 의 {name} 값이 0 · 1 · 비어 있음이 아니다({other})"
        )),
    }
}

impl RawRow {
    /// 행 하나를 검사한다 — 아는 값이 아니면 `Err`(r1d ② · r1f ②).
    fn validate(self) -> Result<AttemptRow, String> {
        let id = self.attempt_id.clone();
        if self.row_format_version != ROW_FORMAT_VERSION {
            return Err(format!(
                "RUN_LEDGER: 행 {id} 의 형식 버전을 모른다({})",
                self.row_format_version
            ));
        }
        let origin = Origin::parse(&self.origin)
            .ok_or_else(|| format!("RUN_LEDGER: 행 {id} 의 출처를 모른다({})", self.origin))?;
        let executor = Executor::parse(&self.executor)
            .ok_or_else(|| format!("RUN_LEDGER: 행 {id} 의 실행기를 모른다({})", self.executor))?;
        let state = RowState::parse(&self.state)
            .ok_or_else(|| format!("RUN_LEDGER: 행 {id} 의 상태를 모른다({})", self.state))?;
        let container_removed = flag("container_removed", &id, self.container_removed)?;
        match (executor, container_removed) {
            (Executor::Container, Some(_)) | (Executor::Host | Executor::Legacy, None) => {}
            _ => {
                return Err(format!(
                    "RUN_LEDGER: 행 {id} 의 container_removed 가 실행기와 맞지 않는다"
                ))
            }
        }
        let legacy_origin = matches!(origin, Origin::LegacyAdopt | Origin::LegacyAdoptAttested);
        if (executor == Executor::Legacy) != legacy_origin {
            return Err(format!(
                "RUN_LEDGER: 행 {id} 의 실행기와 출처가 맞지 않는다"
            ));
        }
        if executor == Executor::Legacy && state != RowState::Closed {
            return Err(format!("RUN_LEDGER: legacy 행 {id} 가 CLOSED 가 아니다"));
        }
        let fence_epoch = match self.fence_epoch {
            None => None,
            Some(v) => Some(
                u64::try_from(v)
                    .map_err(|_| format!("RUN_LEDGER: 행 {id} 의 fence_epoch 가 음수다"))?,
            ),
        };
        Ok(AttemptRow {
            attempt_id: self.attempt_id,
            origin,
            job_id: self.job_id,
            node_id: self.node_id,
            fence_epoch,
            executor,
            runtime_program: self.runtime_program,
            runtime_kind: self.runtime_kind,
            container_name: self.container_name,
            container_id: self.container_id,
            cgroup_path: self.cgroup_path,
            state,
            reason: self.reason,
            stopped: flag("stopped", &id, self.stopped)?,
            logs_complete: flag("logs_complete", &id, self.logs_complete)?,
            container_left: flag("container_left", &id, self.container_left)?,
            container_removed,
        })
    }
}

/// ACTIVE → CLOSED 의 이유. 컨테이너 행은 이유에 따라 부재 확인(container_removed)이 필요하다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    /// 시작 기록(sentinel) 쓰기에 실패해 띄우지 않았다.
    NotStarted,
    /// 기동 때 풀기 — ACTIVE 인데 시작 기록이 없다(시작 안 함이 확실하다).
    NoSentinel,
    /// 종료 보고를 보관했다(정상 경로 또는 기동 때 보고 인정) — 컨테이너 행은 부재 확인이 있어야 한다.
    ReportPersisted,
    /// 기동 때 풀기 — 호스트 행(D5 조각 전 · 지금과 같다).
    HostRestart,
}

impl CloseReason {
    fn as_str(self) -> &'static str {
        match self {
            CloseReason::NotStarted => "not_started",
            CloseReason::NoSentinel => "no_sentinel",
            CloseReason::ReportPersisted => "report_persisted",
            CloseReason::HostRestart => "host_restart",
        }
    }
}

impl RunLedger {
    /// 원장 파일을 열고 검사한다(integrity · meta · 세대 짝 · 모든 행).
    fn open_existing(paths: &LedgerPaths) -> Result<Self, String> {
        let conn = Connection::open_with_flags(
            &paths.ledger,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| {
            format!(
                "RUN_LEDGER: 원장을 열지 못했다({:?}): {error}",
                paths.ledger
            )
        })?;
        apply_pragmas(&conn)?;
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(|error| format!("RUN_LEDGER: 무결성 검사를 못 했다: {error}"))?;
        if integrity != "ok" {
            return Err(format!("RUN_LEDGER: 무결성 검사 실패({integrity})"));
        }
        let meta = |key: &str| -> Result<String, String> {
            conn.query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|error| format!("RUN_LEDGER: meta 를 읽지 못했다: {error}"))?
            .ok_or_else(|| format!("RUN_LEDGER: meta 에 {key} 가 없다"))
        };
        let version = meta("format_version")?;
        if version != FORMAT_VERSION.to_string() {
            return Err(format!("RUN_LEDGER: 원장 형식 버전을 모른다({version})"));
        }
        let generation = meta("generation")?;
        if !is_generation(&generation) {
            return Err("RUN_LEDGER: 원장의 세대 값 형식이 틀렸다".into());
        }
        let ledger = RunLedger { conn, generation };
        // 모든 행을 검사한다 — 메타만 보지 않는다.
        ledger.rows()?;
        Ok(ledger)
    }

    pub fn generation(&self) -> &str {
        &self.generation
    }

    /// 모든 행(검사를 통과한 것만 — 하나라도 이상하면 `Err`).
    pub fn rows(&self) -> Result<Vec<AttemptRow>, String> {
        let mut stmt = self
            .conn
            .prepare(&format!("{SELECT_ROW} ORDER BY attempt_id"))
            .map_err(|error| format!("RUN_LEDGER: 행을 읽지 못했다: {error}"))?;
        let raw = stmt
            .query_map([], row_from_sql)
            .map_err(|error| format!("RUN_LEDGER: 행을 읽지 못했다: {error}"))?;
        let mut rows = Vec::new();
        for item in raw {
            let item = item.map_err(|error| format!("RUN_LEDGER: 행을 읽지 못했다: {error}"))?;
            rows.push(item.validate()?);
        }
        Ok(rows)
    }

    pub fn row(&self, attempt_id: &str) -> Result<Option<AttemptRow>, String> {
        self.conn
            .query_row(
                &format!("{SELECT_ROW} WHERE attempt_id = ?1"),
                params![attempt_id],
                row_from_sql,
            )
            .optional()
            .map_err(|error| format!("RUN_LEDGER: 행을 읽지 못했다: {error}"))?
            .map(RawRow::validate)
            .transpose()
    }

    /// 새 작업을 받으면 안 되는가 — ACTIVE(풀지 않은 것) 나 LOCAL_BLOCKED 가 있으면 참.
    pub fn blocks_new_work(&self) -> Result<bool, String> {
        Ok(self
            .rows()?
            .iter()
            .any(|row| matches!(row.state, RowState::Active | RowState::LocalBlocked)))
    }

    /// 실행 순서 1 — 시작 기록 · 기동 전에 ACTIVE 행을 적는다.
    pub fn insert_active(&mut self, row: &AttemptRow) -> Result<(), String> {
        if row.state != RowState::Active
            || row.origin != Origin::Agent
            || row.executor == Executor::Legacy
        {
            return Err("RUN_LEDGER: 새 행은 이 Agent 의 ACTIVE 행이어야 한다".into());
        }
        // SQLite INTEGER 는 i64 다 — 넘는 값을 조용히 바꿔 적으면 보고와의 신원 대조가 깨진다(r1j ②). 쓰기 전에 거부한다.
        if row.fence_epoch.is_some_and(|e| i64::try_from(e).is_err()) {
            return Err(format!(
                "RUN_LEDGER: fence_epoch 가 원장이 담을 수 있는 범위(i64)를 넘는다({:?}) — 적지 않는다",
                row.fence_epoch
            ));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("RUN_LEDGER: 트랜잭션을 열지 못했다: {error}"))?;
        insert_row(&tx, row, now_unix_ms()).map_err(|error| {
            format!(
                "RUN_LEDGER: ACTIVE 행을 쓰지 못했다({}): {error}",
                row.attempt_id
            )
        })?;
        tx.commit()
            .map_err(|error| format!("RUN_LEDGER: 커밋하지 못했다: {error}"))
    }

    fn update_state(
        &mut self,
        attempt_id: &str,
        guard: impl FnOnce(&AttemptRow) -> Result<(), String>,
        sql: &str,
        extra: &[&dyn rusqlite::ToSql],
    ) -> Result<(), String> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("RUN_LEDGER: 트랜잭션을 열지 못했다: {error}"))?;
        let current = tx
            .query_row(
                &format!("{SELECT_ROW} WHERE attempt_id = ?1"),
                params![attempt_id],
                row_from_sql,
            )
            .optional()
            .map_err(|error| format!("RUN_LEDGER: 행을 읽지 못했다: {error}"))?
            .ok_or_else(|| format!("RUN_LEDGER: 행이 없다({attempt_id})"))?
            .validate()?;
        guard(&current)?;
        let mut values: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(extra.len() + 2);
        let now = now_unix_ms();
        values.push(&now);
        values.extend_from_slice(extra);
        values.push(&attempt_id);
        let changed = tx
            .execute(sql, values.as_slice())
            .map_err(|error| format!("RUN_LEDGER: 행을 바꾸지 못했다({attempt_id}): {error}"))?;
        if changed != 1 {
            return Err(format!(
                "RUN_LEDGER: 행을 바꾸지 못했다({attempt_id}) — {changed}건"
            ));
        }
        tx.commit()
            .map_err(|error| format!("RUN_LEDGER: 커밋하지 못했다: {error}"))
    }

    /// 종료 뒤 관측한 판정 사실을 적는다(계약 §5 "판정" 칸 — 원장만으로 해제 증거를 다시 만들 수 있게). ACTIVE 행에만.
    pub fn record_facts(
        &mut self,
        attempt_id: &str,
        stopped: Option<bool>,
        logs_complete: Option<bool>,
        container_left: Option<bool>,
    ) -> Result<(), String> {
        let (stopped, logs_complete, container_left) = (
            opt_bool(stopped),
            opt_bool(logs_complete),
            opt_bool(container_left),
        );
        self.update_state(
            attempt_id,
            |row| {
                if row.state != RowState::Active {
                    return Err(format!(
                        "RUN_LEDGER: 판정 사실은 ACTIVE 행에만 적는다({attempt_id})"
                    ));
                }
                Ok(())
            },
            "UPDATE attempts SET updated_at_unix_ms = ?1, stopped = ?2, logs_complete = ?3, container_left = ?4
                 WHERE attempt_id = ?5",
            &[&stopped, &logs_complete, &container_left],
        )
    }

    /// 실행 순서 4 — 컨테이너를 지우고 새 조회로 "없음" 을 확인했다(아직 ACTIVE).
    pub fn mark_container_removed(
        &mut self,
        attempt_id: &str,
        container_id: Option<&str>,
    ) -> Result<(), String> {
        let container_id = container_id.map(str::to_string);
        self.update_state(
            attempt_id,
            |row| {
                if row.executor != Executor::Container || row.state != RowState::Active {
                    return Err(format!(
                        "RUN_LEDGER: 부재 확인은 ACTIVE 컨테이너 행에만 적는다({attempt_id})"
                    ));
                }
                Ok(())
            },
            "UPDATE attempts SET updated_at_unix_ms = ?1, container_removed = 1,
                 container_id = COALESCE(?2, container_id) WHERE attempt_id = ?3",
            &[&container_id],
        )
    }

    /// ACTIVE → LOCAL_BLOCKED(컨테이너를 남겼다 · 기동 때 보고 없음 · 보고는 있는데 부재 확인이 없음).
    pub fn mark_local_blocked(&mut self, attempt_id: &str, reason: &str) -> Result<(), String> {
        let reason = reason.to_string();
        self.update_state(
            attempt_id,
            |row| {
                if row.state != RowState::Active {
                    return Err(format!(
                        "RUN_LEDGER: LOCAL_BLOCKED 로는 ACTIVE 에서만 간다({attempt_id} · {:?})",
                        row.state
                    ));
                }
                Ok(())
            },
            "UPDATE attempts SET updated_at_unix_ms = ?1, state = 'LOCAL_BLOCKED', reason = ?2 WHERE attempt_id = ?3",
            &[&reason],
        )
    }

    /// ACTIVE → CLOSED. 컨테이너 행의 `ReportPersisted` 는 부재 확인(container_removed=1)이 있어야 한다.
    pub fn close_active(&mut self, attempt_id: &str, reason: CloseReason) -> Result<(), String> {
        let reason_text = reason.as_str();
        self.update_state(
            attempt_id,
            |row| {
                if row.state != RowState::Active {
                    return Err(format!(
                        "RUN_LEDGER: CLOSED 로는 ACTIVE 에서만 간다(해제는 따로 — {attempt_id} · {:?})",
                        row.state
                    ));
                }
                match (row.executor, reason) {
                    (Executor::Container, CloseReason::ReportPersisted)
                        if row.container_removed != Some(true) =>
                    {
                        Err(format!(
                            "RUN_LEDGER: 컨테이너 부재 확인 없이 닫지 않는다({attempt_id})"
                        ))
                    }
                    (Executor::Container, CloseReason::HostRestart) => Err(format!(
                        "RUN_LEDGER: 컨테이너 행을 호스트 규칙으로 닫지 않는다({attempt_id})"
                    )),
                    (Executor::Host, CloseReason::ReportPersisted | CloseReason::NotStarted
                        | CloseReason::NoSentinel | CloseReason::HostRestart) => Ok(()),
                    (Executor::Legacy, _) => Err(format!("RUN_LEDGER: legacy 행은 바꾸지 않는다({attempt_id})")),
                    _ => Ok(()),
                }
            },
            "UPDATE attempts SET updated_at_unix_ms = ?1, state = 'CLOSED', reason = ?2 WHERE attempt_id = ?3",
            &[&reason_text],
        )
    }

    /// LOCAL_BLOCKED → CLOSED — 해제 명령만 부른다.
    pub fn clear_local_blocked(&mut self, attempt_id: &str) -> Result<(), String> {
        self.update_state(
            attempt_id,
            |row| {
                if row.state != RowState::LocalBlocked {
                    return Err(format!(
                        "RUN_LEDGER: 해제는 LOCAL_BLOCKED 에서만 된다({attempt_id} · {:?})",
                        row.state
                    ));
                }
                Ok(())
            },
            "UPDATE attempts SET updated_at_unix_ms = ?1, state = 'CLOSED', reason = 'cleared' WHERE attempt_id = ?2",
            &[],
        )
    }
}

/// Agent 가 `--run-ledger true` 로 기동할 때 — 원장을 열거나(없으면 새 노드일 때만) 만든다. 모든 거부는 `Err`.
///
/// 부르는 쪽이 루트 잠금 · 실경로 정착을 마치고, 기동 GC · 보관함 전송 **전에** 부른다.
pub fn open_for_agent(paths: &LedgerPaths) -> Result<RunLedger, String> {
    let presence = detect(paths)?;
    if presence.adopting {
        return Err(
            "RUN_LEDGER_ADOPT_UNFINISHED: 이관이 끝나지 않았다 — `gputeer run-ledger adopt-legacy` 를 다시 돌린다".into(),
        );
    }
    let records = scan_start_records(&paths.started_dir, true)?;
    if !presence.ledger {
        if !records.is_empty() {
            return Err(if presence.pair {
                "RUN_LEDGER_LOST: 쓰던 원장이 없다(세대 짝 · 시작 기록은 있다) — 자동으로 다시 만들지 않는다. 수동 복구가 필요하다".into()
            } else {
                "RUN_LEDGER_ADOPT_FIRST: 이 노드는 전에 실행한 기록이 있다 — Agent 를 멈추고 `gputeer run-ledger adopt-legacy` 를 먼저 돌린다".into()
            });
        }
        // 새 노드 — "만드는 중" 표식이 있어야만 짝을 이어 쓴다. 표식 없이 짝만 있으면 **쓰던 원장을 잃은 것**이다(코덱스 r1l ① — ACTIVE 행을 적은 뒤
        //   시작 기록 전에 죽고 원장까지 잃으면 시작 기록이 비어 있어도 돌던 시도가 있었을 수 있다).
        if presence.pair && !presence.creating {
            return Err("RUN_LEDGER_LOST: 쓰던 원장이 없다(세대 짝은 있고 \"만드는 중\" 표식은 없다) — 자동으로 다시 만들지 않는다. 수동 복구가 필요하다".into());
        }
        let generation = if presence.creating {
            let g = read_generation_file(&paths.creating)?;
            if presence.pair && read_generation_file(&paths.pair())? != g {
                return Err("RUN_LEDGER_GENERATION_MISMATCH: \"만드는 중\" 표식과 세대 짝의 값이 다르다 — 수동 복구가 필요하다".into());
            }
            g
        } else {
            new_generation()?
        };
        write_once_value(
            paths.parent_dir()?,
            &LedgerPaths::file_name(&paths.creating)?,
            &generation,
        )?;
        write_once_value(&paths.started_dir, PAIR_NAME, &generation)?;
        create_ledger_file(paths, &generation, &[])?;
        finish_marker(&paths.creating, paths)?;
    } else if !presence.pair {
        return Err(
            "RUN_LEDGER_PAIR_MISSING: 원장은 있는데 세대 짝이 없다 — 수동 복구가 필요하다".into(),
        );
    }
    remove_stale_temps(paths)?;
    let ledger = RunLedger::open_existing(paths)?;
    let pair = read_generation_file(&paths.pair())?;
    if pair != ledger.generation {
        return Err(
            "RUN_LEDGER_GENERATION_MISMATCH: 원장과 세대 짝의 값이 다르다 — 원장이 바뀌었다".into(),
        );
    }
    // 만들기를 마쳤는데 표식을 지우기 전에 끊긴 경우 — 값이 같으면 표식만 지운다.
    if presence.creating && presence.ledger {
        if read_generation_file(&paths.creating)? != pair {
            return Err("RUN_LEDGER_GENERATION_MISMATCH: \"만드는 중\" 표식과 원장의 값이 다르다 — 수동 복구가 필요하다".into());
        }
        finish_marker(&paths.creating, paths)?;
    }
    let rows: BTreeSet<String> = ledger.rows()?.into_iter().map(|r| r.attempt_id).collect();
    if let Some(missing) = records.iter().find(|id| !rows.contains(*id)) {
        return Err(format!(
            "RUN_LEDGER_ROW_MISSING: 시작 기록은 있는데 원장에 행이 없다({missing}) — 원장이 바뀌었다"
        ));
    }
    Ok(ledger)
}

/// 이관 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdoptOutcome {
    /// 새로 이관했다 — 옮긴 행 수.
    Adopted { rows: usize },
    /// 끊긴 이관을 마무리했다(원장은 이미 있었다 · 표식만 지웠다).
    Resumed,
}

/// 이관 명령의 파일 부분 — 부르는 쪽이 잠금 · 실경로 · 열린 사건 · 남은 컨테이너 조회(또는 진술)를 마친 뒤 부른다.
///
/// `attested` 는 런타임 인자 없이 운영자 진술로 통과한 경우다(행 origin 에 남는다).
pub fn adopt_legacy_files(paths: &LedgerPaths, attested: bool) -> Result<AdoptOutcome, String> {
    let presence = detect(paths)?;
    // 끊긴 이관을 "이미 있음" 거부보다 먼저 본다(r1e ②).
    if presence.adopting && presence.ledger {
        let marker = read_generation_file(&paths.adopting)?;
        if !presence.pair {
            return Err(
                "RUN_LEDGER_PAIR_MISSING: 원장은 있는데 세대 짝이 없다 — 수동 복구가 필요하다"
                    .into(),
            );
        }
        let pair = read_generation_file(&paths.pair())?;
        let ledger = RunLedger::open_existing(paths)?;
        if marker != pair || pair != ledger.generation {
            return Err("RUN_LEDGER_GENERATION_MISMATCH: 이관 표식 · 세대 짝 · 원장의 값이 다르다 — 수동 복구가 필요하다".into());
        }
        let records = scan_start_records(&paths.started_dir, true)?;
        let rows: BTreeSet<String> = ledger.rows()?.into_iter().map(|r| r.attempt_id).collect();
        if records.iter().any(|id| !rows.contains(id)) {
            return Err("RUN_LEDGER_ROW_MISSING: 이관된 원장에 시작 기록의 행이 빠졌다 — 수동 복구가 필요하다".into());
        }
        drop(ledger);
        finish_adopting(paths)?;
        return Ok(AdoptOutcome::Resumed);
    }
    if presence.ledger {
        return Err("RUN_LEDGER_EXISTS: 원장이 이미 있다 — 두 번 이관하지 않는다".into());
    }
    if presence.creating {
        return Err("RUN_LEDGER_CREATE_UNFINISHED: Agent 가 새 원장을 만들다 끊겼다 — 이관하지 않는다. Agent 를 --run-ledger true 로 다시 띄워 마무리한다".into());
    }
    let records = scan_start_records(&paths.started_dir, true)?;
    // 이관 표식 없이 짝만 있으면 시작 기록이 없어도 **쓰던 원장을 잃은 것**이다(코덱스 r1l ①).
    if presence.pair && !presence.adopting {
        return Err(
            "RUN_LEDGER_LOST: 쓰던 원장이 없다(세대 짝은 있고 이관 표식은 없다) — 이관으로 다시 만들지 않는다".into(),
        );
    }
    // G — 이미 적힌 값 우선(짝 → 표식 → 새로).
    let pair_value = presence
        .pair
        .then(|| read_generation_file(&paths.pair()))
        .transpose()?;
    let marker_value = presence
        .adopting
        .then(|| read_generation_file(&paths.adopting))
        .transpose()?;
    let generation = match (pair_value, marker_value) {
        (Some(p), Some(m)) if p != m => {
            return Err("RUN_LEDGER_GENERATION_MISMATCH: 세대 짝과 이관 표식의 값이 다르다 — 수동 복구가 필요하다".into())
        }
        (Some(p), _) => p,
        (None, Some(m)) => m,
        (None, None) => new_generation()?,
    };
    let parent = paths.parent_dir()?;
    write_once_value(
        parent,
        &LedgerPaths::file_name(&paths.adopting)?,
        &generation,
    )?;
    write_once_value(&paths.started_dir, PAIR_NAME, &generation)?;
    let origin = if attested {
        Origin::LegacyAdoptAttested
    } else {
        Origin::LegacyAdopt
    };
    let rows: Vec<AttemptRow> = records
        .iter()
        .map(|id| AttemptRow::legacy(id, origin))
        .collect();
    create_ledger_file(paths, &generation, &rows)?;
    finish_adopting(paths)?;
    Ok(AdoptOutcome::Adopted { rows: rows.len() })
}

fn finish_adopting(paths: &LedgerPaths) -> Result<(), String> {
    finish_marker(&paths.adopting, paths)
}

/// 표식(이관 · 만드는 중)을 지우고 폴더를 sync 한다.
fn finish_marker(marker: &Path, paths: &LedgerPaths) -> Result<(), String> {
    match fs::remove_file(marker) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "RUN_LEDGER: 표식을 지우지 못했다({marker:?}): {error}"
            ))
        }
    }
    gputeer_checkpoint::sync_dir(paths.parent_dir()?)
        .map_err(|error| format!("RUN_LEDGER: 폴더 sync 실패: {error:?}"))
}

/// 해제 명령이 원장을 어떻게 다룰지 — 한 번도 켜지 않은 루트만 옛 동작(표식만)이다(r1g ①).
pub fn open_for_clear(paths: &LedgerPaths) -> Result<Option<RunLedger>, String> {
    let presence = detect(paths)?;
    if presence.never_enabled() {
        return Ok(None);
    }
    if presence.adopting {
        return Err("RUN_LEDGER_ADOPT_UNFINISHED: 이관이 끝나지 않았다 — 해제하지 않는다".into());
    }
    if !presence.ledger {
        return Err("RUN_LEDGER_LOST: 원장을 켰던 루트인데 원장이 없다 — 표식만 지우지 않는다. 수동 복구가 필요하다".into());
    }
    let ledger = RunLedger::open_existing(paths)?;
    if !presence.pair || read_generation_file(&paths.pair())? != ledger.generation {
        return Err(
            "RUN_LEDGER_GENERATION_MISMATCH: 원장과 세대 짝이 맞지 않는다 — 해제하지 않는다".into(),
        );
    }
    // Agent 열기와 같은 전수 대조(r1j ①) — 행이 빠진 원장으로 표식만 지우지 않는다.
    let records = scan_start_records(&paths.started_dir, true)?;
    let rows: BTreeSet<String> = ledger.rows()?.into_iter().map(|r| r.attempt_id).collect();
    if let Some(missing) = records.iter().find(|id| !rows.contains(*id)) {
        return Err(format!(
            "RUN_LEDGER_ROW_MISSING: 시작 기록은 있는데 원장에 행이 없다({missing}) — 해제하지 않는다. 수동 복구가 필요하다"
        ));
    }
    Ok(Some(ledger))
}
