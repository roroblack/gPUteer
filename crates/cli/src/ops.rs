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

/// `gputeer submitter-add --keyring <파일> --submitter-id <id> --public-key <hex64> [--i-understand-plaintext-keyring-is-unsafe true]`
///
/// ★ 2026-09-29 — 제출자 keyring 을 만드는 명령이 **없었다.** Coordinator · scheduler · import-manifest 가 모두 `--submitter-keyring` 을 요구하는데
///   그 파일은 이진 형식이라 손으로 못 쓰고, 만드는 길은 Rust API(시험 코드)뿐이었다 — 팀원이 런북만 보고는 풀을 세울 수 없었다.
///   공개키만 넣는다(서명 비밀키는 제출자 기계 밖으로 나오지 않는다). 이미 있는 제출자는 덮지 않는다.
///   파일이 없으면 새로 만든다 — 기본은 OS 보호 저장(K1). 평문(K0)은 `--i-understand-plaintext-keyring-is-unsafe true` 일 때만이고,
///   그 플래그 없이 평문 keyring 을 열면 거부한다(다른 명령들과 같은 규칙).
pub fn submitter_add(args: &[String]) -> Result<String, String> {
    let mut keyring_path = None;
    let mut submitter_id = None;
    let mut public_hex = None;
    let mut plaintext = false;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest
            .next()
            .ok_or_else(|| format!("SUBMITTER_ADD_ARGS_REFUSED: {flag} 에 값이 없다"))?;
        match flag.as_str() {
            "--keyring" => keyring_path = Some(value.clone()),
            "--submitter-id" => submitter_id = Some(value.clone()),
            "--public-key" => public_hex = Some(value.clone()),
            "--i-understand-plaintext-keyring-is-unsafe" => {
                plaintext = match value.as_str() {
                    "true" => true,
                    "false" => false,
                    other => {
                        return Err(format!(
                            "SUBMITTER_ADD_ARGS_REFUSED: --i-understand-plaintext-keyring-is-unsafe 는 true · false 다(받은 값 {other:?})"
                        ))
                    }
                }
            }
            other => return Err(format!("SUBMITTER_ADD_ARGS_REFUSED: 모르는 플래그 {other}")),
        }
    }
    let usage = "gputeer submitter-add --keyring <파일> --submitter-id <id> --public-key <hex64>";
    let keyring_path = keyring_path
        .ok_or_else(|| format!("SUBMITTER_ADD_ARGS_REFUSED: --keyring 이 없다 — {usage}"))?;
    let submitter_id = submitter_id
        .ok_or_else(|| format!("SUBMITTER_ADD_ARGS_REFUSED: --submitter-id 가 없다 — {usage}"))?;
    let public_hex = public_hex
        .ok_or_else(|| format!("SUBMITTER_ADD_ARGS_REFUSED: --public-key 가 없다 — {usage}"))?;
    if public_hex.len() != 64 || !public_hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("SUBMITTER_ADD_ARGS_REFUSED: --public-key 는 16진수 64자리다(keygen 이 찍은 PUBLIC_KEY)".into());
    }
    let mut bytes = [0u8; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&public_hex[i * 2..i * 2 + 2], 16)
            .map_err(|e| format!("SUBMITTER_ADD_ARGS_REFUSED: --public-key 파싱 실패: {e}"))?;
    }
    let public = gputeer_crypto::VerifyingKey::from_bytes(&bytes)
        .map_err(|e| format!("SUBMITTER_ADD_REFUSED: 유효한 Ed25519 공개키가 아니다: {e}"))?;
    let policy = if plaintext {
        gputeer_crypto::PlaintextPolicy::Allow
    } else {
        gputeer_crypto::PlaintextPolicy::Reject
    };
    let path = Path::new(&keyring_path);
    let (mut keyring, created) = if path.exists() {
        (
            gputeer_crypto::PersistentKeyring::load(path, policy).map_err(|e| {
                format!("SUBMITTER_ADD_REFUSED: keyring 을 열지 못했다({keyring_path}): {e:?}")
            })?,
            false,
        )
    } else {
        let protection = if plaintext {
            gputeer_crypto::KeyProtection::K0Plaintext
        } else {
            gputeer_crypto::KeyProtection::K1OsProtected
        };
        (
            gputeer_crypto::PersistentKeyring::new(path, protection, policy).map_err(|e| {
                format!("SUBMITTER_ADD_REFUSED: keyring 을 만들지 못했다({keyring_path}): {e:?}")
            })?,
            true,
        )
    };
    keyring.insert_public(submitter_id.clone(), public).map_err(|e| {
        format!("SUBMITTER_ADD_REFUSED: {submitter_id} 를 넣지 못했다(이미 있으면 덮지 않는다): {e:?}")
    })?;
    keyring.save().map_err(|e| {
        format!("SUBMITTER_ADD_REFUSED: keyring 을 저장하지 못했다({keyring_path}): {e:?}")
    })?;
    Ok(format!(
        "SUBMITTER_ADDED submitter_id={submitter_id} keyring={keyring_path} created={created} protection={:?}",
        keyring.protection()
    ))
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

#[cfg(test)]
mod submitter_add_tests {
    //! 제출자 keyring 을 명령으로 만든다 — 만든 파일을 다른 명령이 쓰는 것과 같은 방식(`PersistentKeyring::load`)으로 다시 읽어 확인한다.
    use super::submitter_add;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    const PUBLIC: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

    #[test]
    fn a_submitter_is_added_to_a_new_plaintext_keyring_and_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("submitters.keyring");
        let p = path.to_str().unwrap();
        let out = submitter_add(&args(&[
            "--keyring",
            p,
            "--submitter-id",
            "SUBMITTER01",
            "--public-key",
            PUBLIC,
            "--i-understand-plaintext-keyring-is-unsafe",
            "true",
        ]))
        .unwrap();
        assert!(out.contains("created=true"), "{out}");
        let keyring =
            gputeer_crypto::PersistentKeyring::load(&path, gputeer_crypto::PlaintextPolicy::Allow)
                .unwrap();
        assert_eq!(
            keyring.status_at("SUBMITTER01", 1),
            gputeer_crypto::KeyDirectoryStatus::Active
        );
        // 두 번째 제출자는 같은 파일에 더한다.
        let out = submitter_add(&args(&[
            "--keyring",
            p,
            "--submitter-id",
            "SUBMITTER02",
            "--public-key",
            PUBLIC,
            "--i-understand-plaintext-keyring-is-unsafe",
            "true",
        ]))
        .unwrap();
        assert!(out.contains("created=false"), "{out}");
        // 이미 있는 제출자는 덮지 않는다.
        let error = submitter_add(&args(&[
            "--keyring",
            p,
            "--submitter-id",
            "SUBMITTER01",
            "--public-key",
            PUBLIC,
            "--i-understand-plaintext-keyring-is-unsafe",
            "true",
        ]))
        .unwrap_err();
        assert!(error.contains("SUBMITTER_ADD_REFUSED"), "{error}");
        // 평문 keyring 을 플래그 없이 열면 거부한다(다른 명령과 같은 규칙).
        let error = submitter_add(&args(&[
            "--keyring",
            p,
            "--submitter-id",
            "SUBMITTER03",
            "--public-key",
            PUBLIC,
        ]))
        .unwrap_err();
        assert!(error.contains("SUBMITTER_ADD_REFUSED"), "{error}");
    }

    #[test]
    fn a_bad_public_key_or_missing_argument_is_refused_without_creating_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("submitters.keyring");
        let p = path.to_str().unwrap();
        for bad in ["abc", &"zz".repeat(32)] {
            let error = submitter_add(&args(&[
                "--keyring",
                p,
                "--submitter-id",
                "S",
                "--public-key",
                bad,
            ]))
            .unwrap_err();
            assert!(error.contains("SUBMITTER_ADD_ARGS_REFUSED"), "{error}");
        }
        assert!(submitter_add(&args(&["--keyring", p, "--public-key", PUBLIC])).is_err());
        assert!(!path.exists(), "거부했는데 파일을 만들었다");
    }
}
