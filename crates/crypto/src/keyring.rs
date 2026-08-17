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
    fmt,
    fs,
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
    pub fn sign<M: gputeer_protocol::signing::Signable + ?Sized>(
        &self,
        message: &M,
    ) -> [u8; 64] {
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
}

impl fmt::Display for KeyringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "키링 파일 I/O 오류: {error}"),
            Self::CorruptFile(reason) => write!(formatter, "키링 파일이 손상되었다: {reason}"),
            Self::InvalidSignerId => formatter.write_str("signer_id가 비어 있거나 너무 길다"),
            Self::DuplicateSigner => formatter.write_str("이미 등록된 signer_id다"),
            Self::MissingSigner => formatter.write_str("등록되지 않은 signer_id다"),
            Self::InvalidState(reason) => write!(formatter, "키 상태 전이가 허용되지 않는다: {reason}"),
            Self::UnsupportedProtection(protection) => {
                write!(formatter, "지원하지 않는 키 보관 등급이다: {protection:?}")
            }
            Self::UnsupportedPlatform => {
                formatter.write_str("현재 플랫폼에서 OS 보호 키 저장소를 사용할 수 없다")
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
                    let raw_private = unprotect_private_key(protection, encrypted_private)?;

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
            return Err(KeyringError::CorruptFile("예상하지 않은 데이터가 뒤에 남았다"));
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
                put_u64(
                    &mut body,
                    version.valid_until_ms.unwrap_or(u64::MAX),
                );
                body.push(match version.state {
                    KeyState::Active => 0,
                    KeyState::Revoked => 1,
                });
                body.extend_from_slice(version.public.as_bytes());

                let private_blob = match &version.private {
                    Some(private) => {
                        let raw = private.raw_bytes_for_storage();
                        protect_private_key(self.protection, &raw)?
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
            #[cfg(not(windows))]
            {
                Err(KeyringError::UnsupportedPlatform)
            }

            #[cfg(windows)]
            {
                Ok(())
            }
        }
        KeyProtection::K2HardwareBacked => {
            Err(KeyringError::UnsupportedProtection(protection))
        }
    }
}

fn protect_private_key(
    protection: KeyProtection,
    private_key: &[u8; 32],
) -> Result<Vec<u8>, KeyringError> {
    match protection {
        KeyProtection::K0Plaintext => Ok(private_key.to_vec()),
        KeyProtection::K1OsProtected => dpapi_protect(private_key),
        KeyProtection::K2HardwareBacked => {
            Err(KeyringError::UnsupportedProtection(protection))
        }
    }
}

fn unprotect_private_key(
    protection: KeyProtection,
    encrypted: &[u8],
) -> Result<Vec<u8>, KeyringError> {
    if encrypted.len() > MAX_BLOB_BYTES {
        return Err(KeyringError::CorruptFile("개인키 blob이 너무 크다"));
    }

    match protection {
        KeyProtection::K0Plaintext => Ok(encrypted.to_vec()),
        KeyProtection::K1OsProtected => dpapi_unprotect(encrypted),
        KeyProtection::K2HardwareBacked => {
            Err(KeyringError::UnsupportedProtection(protection))
        }
    }
}

#[cfg(windows)]
fn dpapi_protect(bytes: &[u8]) -> Result<Vec<u8>, KeyringError> {
    use std::{ffi::c_void, ptr};

    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
        },
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();

    let ok = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            ptr::null(),
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

#[cfg(not(windows))]
fn dpapi_protect(_: &[u8]) -> Result<Vec<u8>, KeyringError> {
    Err(KeyringError::UnsupportedPlatform)
}

#[cfg(windows)]
fn dpapi_unprotect(bytes: &[u8]) -> Result<Vec<u8>, KeyringError> {
    use std::{ffi::c_void, ptr};

    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
        },
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let mut description = ptr::null_mut();

    let ok = unsafe {
        CryptUnprotectData(
            &input,
            &mut description,
            ptr::null(),
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

#[cfg(not(windows))]
fn dpapi_unprotect(_: &[u8]) -> Result<Vec<u8>, KeyringError> {
    Err(KeyringError::UnsupportedPlatform)
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
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| KeyringError::CorruptFile("u32 길이가 잘못되었다"))?,
        ))
    }

    fn u64(&mut self) -> Result<u64, KeyringError> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| KeyringError::CorruptFile("u64 길이가 잘못되었다"))?,
        ))
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
