//! 작업 → Agent 진행 보고(2026-10-01 · 끊겼을 때 판단표 ④ "곧 끝남" 의 선행 조각).
//!
//! # 작업과의 약속(워크로드 계약)
//!
//! ```text
//! GPUTEER_PROGRESS_FILE   작업이 진행을 적는 파일(체크포인트 폴더 안 `progress` — 컨테이너에서는 /gputeer/checkpoints/progress).
//!                         한 줄에 하나씩 `이름=정수`. 알 수 있는 것만 적는다(없는 값은 빼고 지어내지 않는다 — CLAUDE.md §1):
//!                           current_step=120
//!                           total_steps=1000
//!                           eta_seconds=360
//!                           last_committed_step=100
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
//! 모르는 이름 · 정수가 아닌 값 · 같은 이름 두 번 · `current_step > total_steps` · 너무 큰 파일은 **형식 오류**로 버린다.
//! 오타 하나로 값이 조용히 빠지면 "보고가 없다" 와 구별되지 않기 때문이다.

use std::path::Path;

use gputeer_protocol::pb;

/// 체크포인트 폴더 안 진행 파일 이름. 게시기는 `step-<숫자>` 만 보므로 이 파일을 건드리지 않는다.
pub const PROGRESS_FILENAME: &str = "progress";

/// 진행 파일 크기 상한(바이트). 정책값이다 — 네 줄이면 100 바이트도 안 된다.
pub const PROGRESS_MAX_BYTES: u64 = 4096;

/// 작업이 적은 진행(자기보고).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkloadProgress {
    pub current_step: u64,
    pub total_steps: u64,
    pub eta_seconds: u64,
    pub last_committed_step: u64,
}

impl WorkloadProgress {
    /// 갱신 요청에 싣는 꼴(복제 백로그는 작업이 알 수 없어 0 — 지어내지 않는다).
    pub fn to_report(&self) -> pb::ProgressReport {
        pb::ProgressReport {
            current_step: self.current_step,
            total_steps: self.total_steps,
            eta_seconds: self.eta_seconds,
            last_committed_step: self.last_committed_step,
            replication_backlog_bytes: 0,
        }
    }
}

/// 진행 파일을 읽은 결과.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgressRead {
    /// 파일이 없다 — 작업이 진행을 보고하지 않는다(정상).
    Absent,
    /// 있지만 받을 수 없다 — 이유.
    Malformed(String),
    Read(WorkloadProgress),
}

/// 진행 파일 내용을 읽는다(순수 함수).
pub fn parse(text: &str) -> Result<WorkloadProgress, String> {
    let mut progress = WorkloadProgress::default();
    let mut seen = std::collections::BTreeSet::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once('=')
            .ok_or_else(|| format!("{}번째 줄에 '=' 가 없다", index + 1))?;
        let (name, value) = (name.trim(), value.trim());
        let number: u64 = value
            .parse()
            .map_err(|_| format!("{name} 의 값이 0 이상의 정수가 아니다({value:?})"))?;
        if !seen.insert(name.to_string()) {
            return Err(format!("{name} 가 두 번 있다"));
        }
        match name {
            "current_step" => progress.current_step = number,
            "total_steps" => progress.total_steps = number,
            "eta_seconds" => progress.eta_seconds = number,
            "last_committed_step" => progress.last_committed_step = number,
            other => return Err(format!("모르는 이름이다({other:?}) — current_step · total_steps · eta_seconds · last_committed_step 만 받는다")),
        }
    }
    if seen.is_empty() {
        return Err("값이 하나도 없다".into());
    }
    if progress.total_steps > 0 && progress.current_step > progress.total_steps {
        return Err(format!(
            "current_step({}) 가 total_steps({}) 보다 크다",
            progress.current_step, progress.total_steps
        ));
    }
    if progress.last_committed_step > progress.current_step && seen.contains("current_step") {
        return Err(format!(
            "last_committed_step({}) 가 current_step({}) 보다 크다",
            progress.last_committed_step, progress.current_step
        ));
    }
    Ok(progress)
}

/// 진행 파일을 읽는다. 없으면 `Absent`, 크기 상한을 넘거나 형식이 틀리면 `Malformed`.
pub fn read(path: &Path) -> ProgressRead {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return ProgressRead::Absent,
        Err(error) => return ProgressRead::Malformed(format!("읽을 수 없다: {error}")),
    };
    if !metadata.is_file() {
        return ProgressRead::Malformed("보통 파일이 아니다".into());
    }
    if metadata.len() > PROGRESS_MAX_BYTES {
        return ProgressRead::Malformed(format!(
            "{} 바이트 — 상한 {PROGRESS_MAX_BYTES} 바이트를 넘는다",
            metadata.len()
        ));
    }
    match std::fs::read_to_string(path) {
        Ok(text) => match parse(&text) {
            Ok(progress) => ProgressRead::Read(progress),
            Err(why) => ProgressRead::Malformed(why),
        },
        Err(error) => ProgressRead::Malformed(format!("읽을 수 없다: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_well_formed_progress_file_is_read_and_missing_values_stay_zero() {
        let progress =
            parse("current_step=120\r\ntotal_steps=1000\n eta_seconds = 360 \n").unwrap();
        assert_eq!(
            progress,
            WorkloadProgress {
                current_step: 120,
                total_steps: 1000,
                eta_seconds: 360,
                last_committed_step: 0,
            }
        );
        assert_eq!(progress.to_report().replication_backlog_bytes, 0);
    }

    #[test]
    fn unknown_names_bad_numbers_duplicates_and_inconsistencies_are_refused() {
        for (text, why) in [
            ("current_stpe=1", "모르는 이름"),
            ("current_step=-1", "정수가 아니다"),
            ("current_step=1.5", "정수가 아니다"),
            ("current_step=1\ncurrent_step=2", "두 번"),
            ("current_step=11\ntotal_steps=10", "보다 크다"),
            ("current_step=5\nlast_committed_step=6", "보다 크다"),
            ("current_step", "'='"),
            ("\n \n", "하나도 없다"),
        ] {
            let error = parse(text).expect_err(text);
            assert!(error.contains(why), "{text:?} → {error}");
        }
    }

    #[test]
    fn an_absent_file_is_absent_and_an_oversized_one_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PROGRESS_FILENAME);
        assert_eq!(read(&path), ProgressRead::Absent);
        std::fs::write(&path, "current_step=3\ntotal_steps=10\n").unwrap();
        assert!(matches!(read(&path), ProgressRead::Read(p) if p.current_step == 3));
        std::fs::write(&path, "x".repeat(PROGRESS_MAX_BYTES as usize + 1)).unwrap();
        assert!(matches!(read(&path), ProgressRead::Malformed(why) if why.contains("상한")));
    }
}
