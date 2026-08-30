//! 영속 키 관리 계층 — `signing.md` §11.
//!
//! 이 모듈이 보장하는 범위:
//!
//! - K0 평문 파일은 명시적으로 허용해야만 사용할 수 있다.
//! - Windows K1은 DPAPI로 개인키 바이트를 보호한다.
//! - 키 폐기·quarantine·회전 상태를 공개키와 함께 보존한다.
//! - 회전 중에는 24시간 동안 구 키와 신 키를 함께 검증한다.
//! - 손상된 파일은 checksum·길이·공개키/개인키 일치 검사를 통과하지 못한다.
//!
//! 이 모듈이 보장하지 않는 범위:
//!
//! - K0 파일에 대한 관리자·악성 코드 방어
//! - DPAPI가 풀린 뒤 프로세스 메모리·크래시 덤프 보호
//! - TPM 2.0/Secure Enclave 기반 비수출 키
//! - Linux의 OS 보호 저장소
//!
//! 개인키를 담는 타입은 `Debug`, `Display`, `serde` 직렬화를 제공하지 않는다.
//! 사람이 출력할 수 있는 경로에는 개인키 바이트가 절대 들어가지 않는다.

use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use ed25519_dalek::{SigningKey, VerifyingKey};

use super::KeyDirectory;

const FILE_MAGIC: &[u8] = b"GPUTEER-KEYRING\0";
const FILE_VERSION: u8 = 1;
const ROTATION_GRACE_PERIOD_MS: u64 = 24 * 60 * 60 * 1_000;
const MAX_SIGNER_ID_BYTES: usize = 16 * 1024;
const MAX_ENTRY_COUNT: usize = 4_096;
const MAX_BLOB_BYTES: usize = 1024 * 1024;

/// 키 보관 등급.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyProtection {
    /// 평문 파일. 기본적으로 거부한다.
    K0Plaintext = 1,
    /// OS 보호 저장소. Windows에서는 DPAPI를 사용한다.
    K1OsProtected = 2,
    /// TPM/Secure Enclave. 현재 구현하지 않는다.
    K2HardwareBacked = 3,
}

impl KeyProtection {
    fn from_byte(value: u8) -> Result<Self, KeyringError> {
        match value {
            1 => Ok(Self::K0Plaintext),
            2 => Ok(Self::K1OsProtected),
            3 => Ok(Self::K2HardwareBacked),
            _ => Err(KeyringError::CorruptFile("알 수 없는 키 보관 등급")),
        }
    }
}

/// K0 평문 파일 사용 정책.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaintextPolicy {
    /// K0을 거부한다.
    Reject,
    /// 호출자가 K0 사용을 명시적으로 허용했다.
    Allow,
}

impl Default for PlaintextPolicy {
    fn default() -> Self {
        Self::Reject
    }
}

/// 공개키 디렉터리에서 서명자의 상태를 구분한 결과.
///
/// 프로토콜의 기존 `VerifyOutcome`에는 폐기·quarantine 전용 값이 없으므로
/// 실제 서명 검증 결과에서는 세 상태 모두 `UnknownSigner`로 매핑한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDirectoryStatus {
    /// 등록된 키가 없다.
    Unregistered,
    /// 현재 검증 가능한 키가 하나 이상 있다.
    Active,
    /// 등록 이력은 있으나 모든 키가 폐기되었다.
    Revoked,
    /// 장치가 격리되어 키가 있어도 검증에 사용하지 않는다.
    Quarantined,
}

/// 개인키를 감싼 타입.
///
/// 원시 개인키 바이트 접근자를 공개하지 않는다. 서명과 공개키 추출만 허용한다.
pub struct SecretSigningKey(SigningKey);

impl SecretSigningKey {
    /// 기존 `SigningKey`를 비공개 보관 타입으로 감싼다.
    pub fn from_signing_key(key: SigningKey) -> Self {
        Self(key)
    }

    /// 이 개인키로 메시지에 서명한다.
    pub fn sign<M: gputeer_protocol::signing::Signable + ?Sized>(&self, message: &M) -> [u8; 64] {
        super::sign(&self.0, message)
    }

    /// 대응하는 공개키를 반환한다.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.0.verifying_key()
    }

    fn raw_bytes_for_storage(&self) -> [u8; 32] {
        self.0.to_bytes()
    }
}

impl fmt::Debug for SecretSigningKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretSigningKey(REDACTED)")
    }
}

impl fmt::Display for SecretSigningKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretSigningKey(REDACTED)")
    }
}

/// 영속 키링 오류.
///
/// 오류 내용에는 개인키 바이트를 포함하지 않는다.
#[derive(Debug)]
pub enum KeyringError {
    Io(std::io::Error),
    CorruptFile(&'static str),
    InvalidSignerId,
    DuplicateSigner,
    MissingSigner,
    InvalidState(&'static str),
    UnsupportedProtection(KeyProtection),
    UnsupportedPlatform,
    /// OS 보호 저장소는 있는데 그 호출이 실패했다.
    ///
    /// ★ `UnsupportedPlatform` 과 **구분한다**(`CLAUDE.md` §3 — 오류
    ///   메시지가 사실을 잘못 전하지 않게 한다). "이 플랫폼은 지원 안
    ///   한다" 와 "지원하는데 이번 호출이 실패했다" 는 고치는 방법이
    ///   전혀 다르다. 전자는 다른 등급을 골라야 하고, 후자는 권한이나
    ///   봉인 상태를 봐야 한다.
    OsProtectionFailed(String),
}

impl fmt::Display for KeyringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "키링 파일 I/O 오류: {error}"),
            Self::CorruptFile(reason) => write!(formatter, "키링 파일이 손상되었다: {reason}"),
            Self::InvalidSignerId => formatter.write_str("signer_id가 비어 있거나 너무 길다"),
            Self::DuplicateSigner => formatter.write_str("이미 등록된 signer_id다"),
            Self::MissingSigner => formatter.write_str("등록되지 않은 signer_id다"),
            Self::InvalidState(reason) => {
                write!(formatter, "키 상태 전이가 허용되지 않는다: {reason}")
            }
            Self::UnsupportedProtection(protection) => {
                write!(formatter, "지원하지 않는 키 보관 등급이다: {protection:?}")
            }
            Self::UnsupportedPlatform => {
                formatter.write_str("현재 플랫폼에서 OS 보호 키 저장소를 사용할 수 없다")
            }
            Self::OsProtectionFailed(detail) => {
                write!(formatter, "OS 보호 키 저장소 호출이 실패했다: {detail}")
            }
        }
    }
}

impl std::error::Error for KeyringError {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyState {
    Active,
    Revoked,
}

struct KeyVersion {
    public: VerifyingKey,
    private: Option<SecretSigningKey>,
    state: KeyState,
    valid_from_ms: u64,
    valid_until_ms: Option<u64>,
}

impl fmt::Debug for KeyVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KeyVersion")
            .field("public", &self.public)
            .field("has_private_key", &self.private.is_some())
            .field("state", &self.state_name())
            .field("valid_from_ms", &self.valid_from_ms)
            .field("valid_until_ms", &self.valid_until_ms)
            .finish()
    }
}

impl KeyVersion {
    fn state_name(&self) -> &'static str {
        match self.state {
            KeyState::Active => "active",
            KeyState::Revoked => "revoked",
        }
    }
}

/// 파일 기반 키링.
///
/// 변경 후에는 `save()`를 호출해야 파일에 반영된다.
pub struct PersistentKeyring {
    path: PathBuf,
    protection: KeyProtection,
    quarantined: bool,
    keys: BTreeMap<String, Vec<KeyVersion>>,
}

impl fmt::Debug for PersistentKeyring {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PersistentKeyring")
            .field("protection", &self.protection)
            .field("quarantined", &self.quarantined)
            .field("signer_count", &self.keys.len())
            .finish()
    }
}

impl PersistentKeyring {
    /// 새 키링을 만든다.
    pub fn new(
        path: impl Into<PathBuf>,
        protection: KeyProtection,
        plaintext_policy: PlaintextPolicy,
    ) -> Result<Self, KeyringError> {
        ensure_protection(protection, plaintext_policy)?;

        Ok(Self {
            path: path.into(),
            protection,
            quarantined: false,
            keys: BTreeMap::new(),
        })
    }

    /// 기존 키링 파일을 읽는다.
    pub fn load(
        path: impl AsRef<Path>,
        plaintext_policy: PlaintextPolicy,
    ) -> Result<Self, KeyringError> {
        let path = path.as_ref().to_path_buf();
        let bytes = fs::read(&path).map_err(KeyringError::Io)?;

        if bytes.len() < FILE_MAGIC.len() + 1 + 1 + 1 + 1 + 4 + 32 {
            return Err(KeyringError::CorruptFile("파일 길이가 너무 짧다"));
        }

        let checksum_offset = bytes
            .len()
            .checked_sub(32)
            .ok_or(KeyringError::CorruptFile("checksum 위치가 없다"))?;
        let body = &bytes[..checksum_offset];
        let checksum = &bytes[checksum_offset..];

        let expected = blake3::hash(body);
        if &expected.as_bytes()[..] != checksum {
            return Err(KeyringError::CorruptFile("checksum이 일치하지 않는다"));
        }

        let mut reader = Reader::new(body);

        if reader.take(FILE_MAGIC.len())? != FILE_MAGIC {
            return Err(KeyringError::CorruptFile("파일 magic이 다르다"));
        }

        let version = reader.u8()?;
        if version != FILE_VERSION {
            return Err(KeyringError::CorruptFile("지원하지 않는 파일 버전이다"));
        }

        let protection = KeyProtection::from_byte(reader.u8()?)?;
        ensure_protection(protection, plaintext_policy)?;

        let quarantined = match reader.u8()? {
            0 => false,
            1 => true,
            _ => return Err(KeyringError::CorruptFile("quarantine 플래그가 잘못되었다")),
        };

        if reader.u8()? != 0 {
            return Err(KeyringError::CorruptFile("예약 필드가 0이 아니다"));
        }

        let signer_count = reader.u32()? as usize;
        if signer_count > MAX_ENTRY_COUNT {
            return Err(KeyringError::CorruptFile("서명자 수가 상한을 넘었다"));
        }

        let mut keys = BTreeMap::new();

        for _ in 0..signer_count {
            let signer_id = reader.string()?;
            if signer_id.is_empty() || signer_id.len() > MAX_SIGNER_ID_BYTES {
                return Err(KeyringError::CorruptFile("signer_id 길이가 잘못되었다"));
            }

            let version_count = reader.u32()? as usize;
            if version_count == 0 || version_count > MAX_ENTRY_COUNT {
                return Err(KeyringError::CorruptFile("키 버전 수가 잘못되었다"));
            }

            let mut versions = Vec::with_capacity(version_count);

            for _ in 0..version_count {
                let valid_from_ms = reader.u64()?;
                let valid_until_raw = reader.u64()?;
                let valid_until_ms = if valid_until_raw == u64::MAX {
                    None
                } else {
                    if valid_until_raw <= valid_from_ms {
                        return Err(KeyringError::CorruptFile("키 유효기간이 역전되었다"));
                    }
                    Some(valid_until_raw)
                };

                let state = match reader.u8()? {
                    0 => KeyState::Active,
                    1 => KeyState::Revoked,
                    _ => return Err(KeyringError::CorruptFile("키 상태가 잘못되었다")),
                };

                let public_bytes: [u8; 32] = reader
                    .take(32)?
                    .try_into()
                    .map_err(|_| KeyringError::CorruptFile("공개키 길이가 잘못되었다"))?;

                let public = VerifyingKey::from_bytes(&public_bytes)
                    .map_err(|_| KeyringError::CorruptFile("공개키가 유효하지 않다"))?;

                let encrypted_private = reader.blob()?;
                let private = if encrypted_private.is_empty() {
                    None
                } else {
                    let raw_private = unprotect_private_key(protection, &signer_id, encrypted_private)?;

                    if raw_private.len() != 32 {
                        return Err(KeyringError::CorruptFile("개인키 길이가 잘못되었다"));
                    }

                    let private_bytes: [u8; 32] = raw_private
                        .try_into()
                        .map_err(|_| KeyringError::CorruptFile("개인키 길이가 잘못되었다"))?;

                    let private_key = SigningKey::from_bytes(&private_bytes);

                    if private_key.verifying_key() != public {
                        return Err(KeyringError::CorruptFile(
                            "개인키와 공개키가 서로 일치하지 않는다",
                        ));
                    }

                    Some(SecretSigningKey::from_signing_key(private_key))
                };

                versions.push(KeyVersion {
                    public,
                    private,
                    state,
                    valid_from_ms,
                    valid_until_ms,
                });
            }

            if keys.insert(signer_id, versions).is_some() {
                return Err(KeyringError::CorruptFile("signer_id가 중복되었다"));
            }
        }

        if reader.remaining() != 0 {
            return Err(KeyringError::CorruptFile(
                "예상하지 않은 데이터가 뒤에 남았다",
            ));
        }

        Ok(Self {
            path,
            protection,
            quarantined,
            keys,
        })
    }

    pub fn protection(&self) -> KeyProtection {
        self.protection
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 개인키를 새 서명자로 등록한다.
    pub fn insert_private(
        &mut self,
        signer_id: impl Into<String>,
        private: SecretSigningKey,
    ) -> Result<&mut Self, KeyringError> {
        let signer_id = signer_id.into();
        validate_signer_id(&signer_id)?;

        if self.keys.contains_key(&signer_id) {
            return Err(KeyringError::DuplicateSigner);
        }

        self.keys.insert(
            signer_id,
            vec![KeyVersion {
                public: private.verifying_key(),
                private: Some(private),
                state: KeyState::Active,
                valid_from_ms: 0,
                valid_until_ms: None,
            }],
        );

        Ok(self)
    }

    /// 공개키만 등록한다.
    ///
    /// 검증 전용 프로세스에서 사용할 수 있다. 이 항목으로는 서명할 수 없다.
    pub fn insert_public(
        &mut self,
        signer_id: impl Into<String>,
        public: VerifyingKey,
    ) -> Result<&mut Self, KeyringError> {
        let signer_id = signer_id.into();
        validate_signer_id(&signer_id)?;

        if self.keys.contains_key(&signer_id) {
            return Err(KeyringError::DuplicateSigner);
        }

        self.keys.insert(
            signer_id,
            vec![KeyVersion {
                public,
                private: None,
                state: KeyState::Active,
                valid_from_ms: 0,
                valid_until_ms: None,
            }],
        );

        Ok(self)
    }

    /// 키를 폐기한다.
    ///
    /// 폐기 후에는 과거에 정상 서명된 메시지도 현재 검증 기준으로 거부된다.
    pub fn revoke(&mut self, signer_id: &str) -> Result<bool, KeyringError> {
        let versions = self
            .keys
            .get_mut(signer_id)
            .ok_or(KeyringError::MissingSigner)?;

        let mut changed = false;
        for version in versions {
            if version.state != KeyState::Revoked {
                version.state = KeyState::Revoked;
                changed = true;
            }
        }

        Ok(changed)
    }

    /// 장치를 quarantine 상태로 만든다.
    pub fn quarantine(&mut self, signer_id: &str) -> Result<bool, KeyringError> {
        if !self.keys.contains_key(signer_id) {
            return Err(KeyringError::MissingSigner);
        }

        let changed = !self.quarantined;
        self.quarantined = true;
        Ok(changed)
    }

    /// quarantine을 해제한다.
    ///
    /// 이미 폐기된 키는 quarantine 해제 후에도 복구되지 않는다.
    pub fn release_quarantine(&mut self, signer_id: &str) -> Result<bool, KeyringError> {
        if !self.keys.contains_key(signer_id) {
            return Err(KeyringError::MissingSigner);
        }

        let changed = self.quarantined;
        self.quarantined = false;
        Ok(changed)
    }

    pub fn is_quarantined(&self) -> bool {
        self.quarantined
    }

    /// 새 키를 등록하고 기존 키를 24시간 grace 상태로 둔다.
    pub fn rotate(
        &mut self,
        signer_id: &str,
        new_private: SecretSigningKey,
        now_ms: u64,
    ) -> Result<&mut Self, KeyringError> {
        if self.status_at(signer_id, now_ms) != KeyDirectoryStatus::Active {
            return Err(KeyringError::InvalidState(
                "활성 키가 있는 서명자만 회전할 수 있다",
            ));
        }

        let new_public = new_private.verifying_key();
        let versions = self
            .keys
            .get_mut(signer_id)
            .ok_or(KeyringError::MissingSigner)?;

        if versions.iter().any(|version| version.public == new_public) {
            return Err(KeyringError::InvalidState("새 키가 기존 키와 같다"));
        }

        let grace_until = now_ms.saturating_add(ROTATION_GRACE_PERIOD_MS);

        for version in versions.iter_mut() {
            if version.state == KeyState::Active
                && version.valid_from_ms <= now_ms
                && version.valid_until_ms.map_or(true, |until| now_ms < until)
            {
                version.valid_until_ms = Some(grace_until);
            }
        }

        versions.push(KeyVersion {
            public: new_public,
            private: Some(new_private),
            state: KeyState::Active,
            valid_from_ms: now_ms,
            valid_until_ms: None,
        });

        Ok(self)
    }

    /// 특정 시각의 서명자 상태를 반환한다.
    pub fn status_at(&self, signer_id: &str, now_ms: u64) -> KeyDirectoryStatus {
        let Some(versions) = self.keys.get(signer_id) else {
            return KeyDirectoryStatus::Unregistered;
        };

        if self.quarantined {
            return KeyDirectoryStatus::Quarantined;
        }

        if versions
            .iter()
            .any(|version| version_is_valid(version, now_ms))
        {
            KeyDirectoryStatus::Active
        } else {
            KeyDirectoryStatus::Revoked
        }
    }

    /// 특정 시각을 기준으로 동작하는 검증용 디렉터리를 만든다.
    pub fn at(&self, now_ms: u64) -> KeyDirectoryView<'_> {
        KeyDirectoryView {
            keyring: self,
            now_ms,
        }
    }

    /// 현재 상태를 파일에 저장한다.
    pub fn save(&self) -> Result<(), KeyringError> {
        let mut body = Vec::new();

        body.extend_from_slice(FILE_MAGIC);
        body.push(FILE_VERSION);
        body.push(self.protection as u8);
        body.push(u8::from(self.quarantined));
        body.push(0);
        put_u32(&mut body, self.keys.len() as u32);

        for (signer_id, versions) in &self.keys {
            put_string(&mut body, signer_id)?;
            put_u32(&mut body, versions.len() as u32);

            for version in versions {
                put_u64(&mut body, version.valid_from_ms);
                put_u64(&mut body, version.valid_until_ms.unwrap_or(u64::MAX));
                body.push(match version.state {
                    KeyState::Active => 0,
                    KeyState::Revoked => 1,
                });
                body.extend_from_slice(version.public.as_bytes());

                let private_blob = match &version.private {
                    Some(private) => {
                        let raw = private.raw_bytes_for_storage();
                        protect_private_key(self.protection, signer_id, &raw)?
                    }
                    None => Vec::new(),
                };

                put_blob(&mut body, &private_blob)?;
            }
        }

        let checksum = blake3::hash(&body);
        body.extend_from_slice(checksum.as_bytes());

        fs::write(&self.path, &body).map_err(KeyringError::Io)?;

        // Unix의 파일 권한은 우발적인 다른 일반 사용자 접근을 줄일 뿐이다.
        // 관리자·동일 사용자 악성 프로세스를 막는다고 보장하지 않는다.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(&self.path)
                .map_err(KeyringError::Io)?
                .permissions();
            permissions.set_mode(0o600);
            fs::set_permissions(&self.path, permissions).map_err(KeyringError::Io)?;
        }

        Ok(())
    }
}

/// 특정 시각에 사용할 공개키 디렉터리 뷰.
pub struct KeyDirectoryView<'a> {
    keyring: &'a PersistentKeyring,
    now_ms: u64,
}

impl<'a> KeyDirectoryView<'a> {
    pub fn status(&self, signer_id: &str) -> KeyDirectoryStatus {
        self.keyring.status_at(signer_id, self.now_ms)
    }
}

impl KeyDirectory for KeyDirectoryView<'_> {
    fn lookup(&self, signer_id: &str) -> Option<VerifyingKey> {
        self.lookup_candidates(signer_id).into_iter().next()
    }

    fn lookup_candidates(&self, signer_id: &str) -> Vec<VerifyingKey> {
        if self.keyring.quarantined {
            return Vec::new();
        }

        self.keyring
            .keys
            .get(signer_id)
            .into_iter()
            .flat_map(|versions| versions.iter())
            .filter(|version| version_is_valid(version, self.now_ms))
            .map(|version| version.public)
            .collect()
    }

    /// 물러난 키 — 폐기됐거나 회전 grace period 가 끝난 것.
    ///
    /// ★ quarantine 중이면 **여기도 비운다.**
    ///   quarantine 은 "이 device 의 어떤 것도 신뢰하지 않는다" 이므로,
    ///   물러난 키를 근거로 오류 종류를 바꿔 주는 것조차 정보를 준다.
    fn lookup_retired(&self, signer_id: &str) -> Vec<VerifyingKey> {
        if self.keyring.quarantined {
            return Vec::new();
        }

        self.keyring
            .keys
            .get(signer_id)
            .into_iter()
            .flat_map(|versions| versions.iter())
            .filter(|version| !version_is_valid(version, self.now_ms))
            .map(|version| version.public)
            .collect()
    }
}

impl KeyDirectory for PersistentKeyring {
    fn lookup(&self, signer_id: &str) -> Option<VerifyingKey> {
        self.at(unix_now_ms()).lookup(signer_id)
    }

    fn lookup_candidates(&self, signer_id: &str) -> Vec<VerifyingKey> {
        self.at(unix_now_ms()).lookup_candidates(signer_id)
    }

    fn lookup_retired(&self, signer_id: &str) -> Vec<VerifyingKey> {
        self.at(unix_now_ms()).lookup_retired(signer_id)
    }
}

fn version_is_valid(version: &KeyVersion, now_ms: u64) -> bool {
    version.state == KeyState::Active
        && version.valid_from_ms <= now_ms
        && version.valid_until_ms.map_or(true, |until| now_ms < until)
}

fn validate_signer_id(signer_id: &str) -> Result<(), KeyringError> {
    if signer_id.is_empty() || signer_id.len() > MAX_SIGNER_ID_BYTES {
        return Err(KeyringError::InvalidSignerId);
    }
    Ok(())
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn ensure_protection(
    protection: KeyProtection,
    plaintext_policy: PlaintextPolicy,
) -> Result<(), KeyringError> {
    match protection {
        KeyProtection::K0Plaintext => {
            if plaintext_policy == PlaintextPolicy::Reject {
                Err(KeyringError::UnsupportedProtection(protection))
            } else {
                Ok(())
            }
        }
        KeyProtection::K1OsProtected => {
            #[cfg(any(windows, target_os = "linux"))]
            {
                Ok(())
            }
            #[cfg(not(any(windows, target_os = "linux")))]
            {
                Err(KeyringError::UnsupportedPlatform)
            }
        }
        KeyProtection::K2HardwareBacked => Err(KeyringError::UnsupportedProtection(protection)),
    }
}

/// 개인키를 이 등급으로 봉인한다.
///
/// # 왜 `signer_id` 가 필요한가
///
/// ★ 봉인 이름을 **signer 마다 다르게** 묶는다. 고정 이름을 쓰면 A 의
///   봉인 blob 을 B 자리에 바꿔치기해도 복호가 성공한다 — 파일을 쓸 수
///   있는 누군가가 어느 장치의 키를 다른 장치의 것으로 만들 수 있다.
///
///   `signing.md` 의 `domain_tag` 가 서명에서, `derive_replay_nonce` 가
///   nonce 에서 하는 일과 같은 종류의 분리다.
fn protect_private_key(
    protection: KeyProtection,
    signer_id: &str,
    private_key: &[u8; 32],
) -> Result<Vec<u8>, KeyringError> {
    match protection {
        KeyProtection::K0Plaintext => Ok(private_key.to_vec()),
        KeyProtection::K1OsProtected => os_protect(signer_id, private_key),
        KeyProtection::K2HardwareBacked => Err(KeyringError::UnsupportedProtection(protection)),
    }
}

fn unprotect_private_key(
    protection: KeyProtection,
    signer_id: &str,
    encrypted: &[u8],
) -> Result<Vec<u8>, KeyringError> {
    if encrypted.len() > MAX_BLOB_BYTES {
        return Err(KeyringError::CorruptFile("개인키 blob이 너무 크다"));
    }

    match protection {
        KeyProtection::K0Plaintext => Ok(encrypted.to_vec()),
        KeyProtection::K1OsProtected => os_unprotect(signer_id, encrypted),
        KeyProtection::K2HardwareBacked => Err(KeyringError::UnsupportedProtection(protection)),
    }
}

/// DPAPI 의 optional entropy 로 쓸 signer 별 바이트.
///
/// ★ 여기에 signer 를 묶으면 A 의 봉인 blob 을 B 자리에 놓아도 복호가
///   **실패한다.** 없으면 파일을 쓸 수 있는 누군가가 어느 장치의 키를
///   다른 장치의 것으로 만들 수 있다. Linux 쪽 `--name=` 과 같은 역할이다.
#[cfg(windows)]
fn dpapi_entropy(signer_id: &str) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gputeer/v1/keyring-dpapi-entropy");
    hasher.update(&(signer_id.len() as u64).to_be_bytes());
    hasher.update(signer_id.as_bytes());
    *hasher.finalize().as_bytes()
}

#[cfg(windows)]
fn dpapi_protect(signer_id: &str, bytes: &[u8]) -> Result<Vec<u8>, KeyringError> {
    use std::{ffi::c_void, ptr};

    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB},
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut entropy_bytes = dpapi_entropy(signer_id);
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: entropy_bytes.len() as u32,
        pbData: entropy_bytes.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();

    let ok = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            &entropy,
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };

    if ok == 0 || output.pbData.is_null() || output.cbData == 0 {
        return Err(KeyringError::Io(std::io::Error::last_os_error()));
    }

    let result = unsafe {
        let data = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as *mut c_void);
        data
    };

    Ok(result)
}

/// K1 을 이 플랫폼의 OS 보호 저장소로 보낸다.
///
/// ```text
/// Windows  DPAPI (CryptProtectData)        — **사용자** 경계
/// Linux    systemd-creds --with-key=host   — **기계·root** 경계
/// 그 외    UnsupportedPlatform
/// ```
///
/// ★ 둘은 같은 등급 이름을 쓰지만 **막는 대상이 다르다.** 아래
///   `linux_creds_protect` 문서를 보라. 같은 K1 이라고 해서 같은
///   보호를 받는다고 읽으면 안 된다.
fn os_protect(signer_id: &str, bytes: &[u8]) -> Result<Vec<u8>, KeyringError> {
    #[cfg(windows)]
    {
        // ★ DPAPI 도 signer 로 묶는다. `CryptProtectData` 의 optional
        //   entropy 자리에 넣으면 이름이 다른 blob 은 복호에 실패한다.
        dpapi_protect(signer_id, bytes)
    }
    #[cfg(target_os = "linux")]
    {
        linux_creds_protect(signer_id, bytes)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (signer_id, bytes);
        Err(KeyringError::UnsupportedPlatform)
    }
}

fn os_unprotect(signer_id: &str, bytes: &[u8]) -> Result<Vec<u8>, KeyringError> {
    #[cfg(windows)]
    {
        dpapi_unprotect(signer_id, bytes)
    }
    #[cfg(target_os = "linux")]
    {
        linux_creds_unprotect(signer_id, bytes)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (signer_id, bytes);
        Err(KeyringError::UnsupportedPlatform)
    }
}

/// `systemd-creds` 가 ciphertext 를 묶는 이름.
///
/// ★ 이름이 다르면 복호가 실패한다. 다른 용도로 만든 blob 을 키로
///   되읽는 것을 막는다 — `signing.md` 의 `domain_tag` 가 서명에서
///   하는 일과 같은 종류의 분리다.
/// 봉인 이름의 앞부분. 뒤에 signer 별 꼬리표가 붙는다.
#[cfg(target_os = "linux")]
const LINUX_CREDENTIAL_PREFIX: &str = "gputeer-device-private-key";

/// signer 마다 다른 봉인 이름.
///
/// ★ `signer_id` 를 그대로 붙이지 않는다. 이 값은 명령줄 인자로 나가고
///   (`--name=`), systemd 가 허용하는 문자 집합이 정해져 있다. 해시로
///   바꾸면 문자 집합·길이 문제가 한 번에 사라진다.
///
/// ★ `derive_cgroup_name` 과 같은 이유로 길이 접두사를 넣는다 —
///   성분 경계가 흐려지면 서로 다른 signer 가 같은 이름을 받는다.
#[cfg(target_os = "linux")]
fn linux_credential_name(signer_id: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gputeer/v1/keyring-credential-name");
    hasher.update(&(signer_id.len() as u64).to_be_bytes());
    hasher.update(signer_id.as_bytes());
    format!(
        "{LINUX_CREDENTIAL_PREFIX}-{}",
        &hasher.finalize().to_hex()[..32]
    )
}

/// Linux 의 K1 — `systemd-creds --with-key=host`.
///
/// # 이것이 막는 것과 못 막는 것
///
/// ★ **Windows DPAPI 와 경계가 다르다.** 같은 "K1" 이라는 이름을 쓰지만
///   같은 보호가 아니다.
///
/// ```text
///            막는다                                  못 막는다
/// DPAPI      같은 기계의 **다른 사용자**             같은 사용자, 관리자
/// host key   같은 기계의 **비-root 사용자**          root, 다른 기계로의 이동은
///                                                    막지만 root 면 그만
/// ```
///
/// `--with-key=host` 는 `/var/lib/systemd/credential.secret`(0600 root)
/// 로 봉인한다. 그래서:
///
/// ```text
/// 막는다     비-root 사용자의 읽기
///            키링 파일만 훔쳐 **다른 기계**에서 여는 것
/// 못 막는다  root
///            복호 뒤의 프로세스 메모리·크래시 덤프(Windows 와 동일)
///            디스크가 암호화돼 있지 않으면 디스크를 통째로 가져가는 것
/// ```
///
/// ★ 마지막 항목은 추측이 아니다 — `systemd-creds` 자신이 경고한다.
///   x600 WSL 실측에서 정확히 이 문구가 나왔다.
///
///   > Credential secret file '/var/lib/systemd/credential.secret' is not
///   > located on encrypted media, using anyway.
///
///   TPM 으로 봉인하면(`--with-key=tpm2`) 그것까지 막지만 그건 K2 이고,
///   이 조각은 K2 를 구현하지 않는다(`CLAUDE.md` §0.4 — 강제할 수 없는
///   것을 보장으로 선언하지 않는다).
///
/// # 운영 전제 — root 권한이 사실상 필요하다
///
/// ★ 2026-08-30 독립 검수 3라운드가 빠졌다고 지적했다.
///   `/var/lib/systemd/credential.secret` 은 0600 root 다. 즉 **Agent 가
///   그 파일을 읽을 수 있어야** K1 이 성립한다 — 비-root 로 도는 Agent 는
///   `systemd-creds` 가 설치돼 있어도 K1 이 실패한다.
///
///   그건 결함이 아니라 이 등급의 조건이다. 다만 "Linux 에 K1 이 있다"
///   가 "아무 Agent 나 쓸 수 있다" 로 읽히면 안 되므로 여기 적는다.
///
/// # 왜 라이브러리가 아니라 subprocess 인가
///
/// systemd 의 credential 형식을 직접 다루려면 그 포맷과 TPM 정책을
/// 재구현해야 한다. 남의 PC 에서 도는 코드에 암호 구현을 하나 더
/// 늘리는 것보다, OS 가 이미 관리하는 도구를 부르는 편이 낫다.
/// `dpapi_protect` 가 Win32 API 를 부르는 것과 같은 자리다.
#[cfg(target_os = "linux")]
fn linux_creds_protect(signer_id: &str, bytes: &[u8]) -> Result<Vec<u8>, KeyringError> {
    run_systemd_creds(&["encrypt", "--with-key=host"], signer_id, bytes)
}

#[cfg(target_os = "linux")]
fn linux_creds_unprotect(signer_id: &str, bytes: &[u8]) -> Result<Vec<u8>, KeyringError> {
    run_systemd_creds(&["decrypt"], signer_id, bytes)
}

/// `systemd-creds` 를 stdin -> stdout 으로 한 번 부른다.
///
/// ★ 개인키를 **명령줄 인자나 임시 파일로 넘기지 않는다.** 인자는
///   `/proc/<pid>/cmdline` 으로 같은 기계의 아무나 읽을 수 있고, 임시
///   파일은 지우기 전에 죽으면 남는다. stdin 은 그 둘 다 아니다.
///
/// ★ 그러나 **"누출이 없다" 는 아니다**(2026-08-30 독립 검수 3라운드).
///   정확히는 "명령줄·임시 파일 누출을 피했다" 다. 평문은 Agent 메모리
///   외에 커널 파이프와 `systemd-creds` 프로세스 메모리에도 한 번 더
///   존재한다. 그 둘은 이 코드가 없앨 수 없다.
#[cfg(target_os = "linux")]
fn run_systemd_creds(
    args: &[&str],
    signer_id: &str,
    input: &[u8],
) -> Result<Vec<u8>, KeyringError> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    /// 이만큼 안 끝나면 죽인다.
    ///
    /// ★ 상한이 없으면 helper 가 멈췄을 때 Agent 도 무기한 멈춘다
    ///   (2026-08-30 독립 검수 3라운드 지적). 이건 로컬 도구를 짧게
    ///   부르는 것이므로 넉넉히 잡아도 초 단위면 충분하다.
    const DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

    let mut child = Command::new("systemd-creds")
        .args(args)
        .arg(format!("--name={}", linux_credential_name(signer_id)))
        // `- -` 는 stdin 에서 읽어 stdout 으로 쓴다는 뜻이다.
        .arg("-")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        // ★ **없는 것**과 **있는데 못 띄운 것**을 구분한다(§3).
        //   초안은 전부 UnsupportedPlatform 이라, 권한 거부나 프로세스
        //   한도 초과도 "이 플랫폼은 지원 안 함" 으로 보고했다 — 고치는
        //   방법이 전혀 다른데 같은 말을 했다.
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => KeyringError::UnsupportedPlatform,
            _ => KeyringError::OsProtectionFailed(format!("systemd-creds 기동 실패: {error}")),
        })?;

    // ★ 아래 어느 단계에서 실패하든 자식을 **반드시 회수한다.**
    //   Rust 의 `Child` 는 drop 해도 자동으로 안 거둬서, 그냥 반환하면
    //   자식이 계속 돌거나 좀비로 남는다(같은 검수 지적).
    let reap = |child: &mut std::process::Child| {
        let _ = child.kill();
        let _ = child.wait();
    };

    let write_result = (|| -> std::io::Result<()> {
        let mut stdin = child.stdin.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stdin 을 열지 못했다")
        })?;
        stdin.write_all(input)?;
        // stdin 을 닫아야 상대가 EOF 를 본다.
        drop(stdin);
        Ok(())
    })();
    if let Err(error) = write_result {
        reap(&mut child);
        return Err(KeyringError::Io(error));
    }

    // 시간 상한 안에서 기다린다. 넘기면 죽이고 실패로 보고한다.
    let deadline = std::time::Instant::now() + DEADLINE;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    reap(&mut child);
                    return Err(KeyringError::OsProtectionFailed(format!(
                        "systemd-creds {} 가 {DEADLINE:?} 안에 끝나지 않았다",
                        args.join(" ")
                    )));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => {
                reap(&mut child);
                return Err(KeyringError::Io(error));
            }
        }
    };

    // 파이프에 남은 것을 읽는다. 자식은 이미 끝났으므로 블로킹하지 않는다.
    let mut stdout = Vec::new();
    let mut stderr = String::new();
    if let Some(mut handle) = child.stdout.take() {
        use std::io::Read as _;
        handle.read_to_end(&mut stdout).map_err(KeyringError::Io)?;
    }
    if let Some(mut handle) = child.stderr.take() {
        use std::io::Read as _;
        let _ = handle.read_to_string(&mut stderr);
    }

    if !status.success() {
        // ★ "없다" 가 아니라 "있는데 실패했다" 다. 진단에 필요한 만큼만
        //   stderr 를 싣는다 — 개인키는 stdin 으로만 갔고 stderr 에는
        //   systemd 의 진단 문구만 나온다.
        let detail: String = stderr
            .lines()
            .next()
            .unwrap_or("(stderr 없음)")
            .chars()
            .take(200)
            .collect();
        return Err(KeyringError::OsProtectionFailed(format!(
            "systemd-creds {} 실패({status}): {detail}",
            args.join(" ")
        )));
    }
    if stdout.is_empty() {
        // 성공했다는데 아무것도 안 나왔다. 빈 키를 통과시키지 않는다.
        return Err(KeyringError::CorruptFile("systemd-creds 출력이 비었다"));
    }
    Ok(stdout)
}

#[cfg(windows)]
fn dpapi_unprotect(signer_id: &str, bytes: &[u8]) -> Result<Vec<u8>, KeyringError> {
    use std::{ffi::c_void, ptr};

    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut entropy_bytes = dpapi_entropy(signer_id);
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: entropy_bytes.len() as u32,
        pbData: entropy_bytes.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let mut description = ptr::null_mut();

    let ok = unsafe {
        CryptUnprotectData(
            &input,
            &mut description,
            &entropy,
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };

    if ok == 0 || output.pbData.is_null() || output.cbData == 0 {
        return Err(KeyringError::Io(std::io::Error::last_os_error()));
    }

    if output.cbData as usize > MAX_BLOB_BYTES {
        unsafe {
            LocalFree(output.pbData as *mut c_void);
        }
        return Err(KeyringError::CorruptFile("DPAPI 결과가 너무 크다"));
    }

    let result = unsafe {
        let data = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as *mut c_void);

        if !description.is_null() {
            LocalFree(description as *mut c_void);
        }

        data
    };

    Ok(result)
}



struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], KeyringError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(KeyringError::CorruptFile("길이 계산이 넘쳤다"))?;

        if end > self.bytes.len() {
            return Err(KeyringError::CorruptFile("파일이 중간에 끝났다"));
        }

        let result = &self.bytes[self.position..end];
        self.position = end;
        Ok(result)
    }

    fn u8(&mut self) -> Result<u8, KeyringError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, KeyringError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(
            |_| KeyringError::CorruptFile("u32 길이가 잘못되었다"),
        )?))
    }

    fn u64(&mut self) -> Result<u64, KeyringError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().map_err(
            |_| KeyringError::CorruptFile("u64 길이가 잘못되었다"),
        )?))
    }

    fn blob(&mut self) -> Result<&'a [u8], KeyringError> {
        let length = self.u32()? as usize;
        if length > MAX_BLOB_BYTES {
            return Err(KeyringError::CorruptFile("blob이 너무 크다"));
        }
        self.take(length)
    }

    fn string(&mut self) -> Result<String, KeyringError> {
        let bytes = self.blob()?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| KeyringError::CorruptFile("signer_id가 UTF-8이 아니다"))
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_blob(output: &mut Vec<u8>, value: &[u8]) -> Result<(), KeyringError> {
    if value.len() > MAX_BLOB_BYTES || value.len() > u32::MAX as usize {
        return Err(KeyringError::InvalidState("저장할 blob이 너무 크다"));
    }

    put_u32(output, value.len() as u32);
    output.extend_from_slice(value);
    Ok(())
}

fn put_string(output: &mut Vec<u8>, value: &str) -> Result<(), KeyringError> {
    if value.len() > MAX_SIGNER_ID_BYTES {
        return Err(KeyringError::InvalidSignerId);
    }
    put_blob(output, value.as_bytes())
}
