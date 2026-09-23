//! 운영 명령 — `gputeer keygen` · `gputeer status` (신뢰망 남은 일 J).

use std::path::Path;

/// `gputeer keygen --out <파일>` — 서명 시드를 만들어 **파일에만** 쓰고 공개키를 찍는다.
///
/// ★ 이미 있는 파일은 덮지 않는다 — 키를 잃으면 그 노드 · Coordinator 의 신원이 바뀐다.
/// ★ 유닉스에서는 0600 으로 만든다. Windows 는 사용자 프로필 아래에 두는 것을 권한다(런북).
pub fn keygen(args: &[String]) -> Result<String, String> {
    let out = match (args.first().map(String::as_str), args.get(1)) {
        (Some("--out"), Some(path)) if args.len() == 2 => path,
        _ => return Err("KEYGEN_ARGS_REFUSED: gputeer keygen --out <파일>".to_string()),
    };
    let path = Path::new(out);
    if path.exists() {
        return Err(format!(
            "KEYGEN_REFUSED: {out} 가 이미 있다 — 키를 덮지 않는다(잃으면 신원이 바뀐다)"
        ));
    }
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| format!("KEYGEN: CSPRNG 실패: {e}"))?;
    let hex: String = seed.iter().map(|b| format!("{b:02x}")).collect();
    {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(path)
            .map_err(|e| format!("KEYGEN: {out} 를 만들지 못했다: {e}"))?;
        file.write_all(hex.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|e| format!("KEYGEN: {out} 에 쓰지 못했다: {e}"))?;
    }
    let public: String = gputeer_crypto::SigningKey::from_bytes(&seed)
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(format!("SEED_FILE {out}\nPUBLIC_KEY {public}"))
}

/// `gputeer status --control-db <path>` — Job · 노드 · 예약을 한눈에. 읽기만 한다.
pub fn status(args: &[String]) -> Result<String, String> {
    let db = match (args.first().map(String::as_str), args.get(1)) {
        (Some("--control-db"), Some(path)) if args.len() == 2 => path,
        _ => return Err("STATUS_ARGS_REFUSED: gputeer status --control-db <path>".to_string()),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;
    gputeer_coordinator::status::status_report(Path::new(db), now)
}
