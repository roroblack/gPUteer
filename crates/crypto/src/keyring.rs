//! 영속 키 관리 계층 — `signing.md` §11.
//!
//! 이 모듈이 보장하는 범위:
//!
//! - K0 평문 파일은 명시적으로 허용해야만 사용할 수 있다.
//! - Windows K1은 DPAPI로, Linux K1은 `systemd-creds --with-key=host`로
//!   개인키 바이트를 보호한다. **두 등급은 이름이 같지만 막는 경계가
//!   다르다** — Windows는 다른 *사용자*를, Linux는 *비-root*를 막는다.
//! - 키 폐기·quarantine·회전 상태를 공개키와 함께 **한 파일 안에서**
//!   보존한다. ★ 그 파일이 통째로 옛 버전으로 되돌려지는 것은 막지
//!   못한다 — 아래 비보장 목록 참조.
//! - 회전 중에는 24시간 동안 구 키와 신 키를 함께 검증한다.
//! - 손상된 파일은 checksum·길이·공개키/개인키 일치 검사를 통과하지 못한다.
//!
//! 이 모듈이 보장하지 않는 범위:
//!
//! - K0 파일에 대한 관리자·악성 코드 방어
//! - 복호된 뒤의 프로세스 메모리·크래시 덤프 보호(두 플랫폼 공통)
//! - TPM 2.0/Secure Enclave 기반 비수출 키 (= K2, 미구현)
//! - **신선성·롤백 방지.** K1은 *변조 탐지*만 제공한다. 정상적으로
//!   봉인됐던 **과거 파일을 통째로 되돌리면** 검증을 그대로 통과하며,
//!   그때 되살아나는 것은 폐기된 개인키만이 아니다.
//!
//!   ```text
//!   되살아나는 것   폐기(revoke)된 키
//!                   quarantine 설정·해제 상태
//!                   그 뒤 새로 등록된 키가 사라짐
//!                   회전 이력과 유효 시작·종료 시각
//!                   공개키 디렉터리의 최신 신뢰 상태
//!   ```
//!
//!   막으려면 파일 밖의 단조 상태(TPM monotonic counter 등)가 필요하고
//!   이 모듈에 없다. `CLAUDE.md` §0.4 대로 반쯤 동작하는 방어를 만들지
//!   않고 **못 막는다고 적는다.**
//! - **K1 경계 안쪽의 공격자.** Windows의 같은 사용자, Linux의 root는
//!   알려진 entropy/이름으로 직접 봉인을 만들 수 있다 — 처음부터 이
//!   등급의 정의다.
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
/// 파일 형식 버전.
///
/// ★ 2026-08-30 에 1 -> 2 로 올렸다(독립 검수 4라운드 지적).
///
///   v1 은 꼬리에 **키 없는** BLAKE3 체크섬을 붙였다. 파일을 쓸 수 있는
///   누구나 다시 계산할 수 있으므로 위조를 전혀 막지 못했다 — 개인키를
///   signer 에 묶어도, **개인키 blob 을 비워** 공개키 전용 엔트리로
///   만들면 복호를 건너뛰어 그 방어가 통째로 우회됐다.
///
///   v2 는 그 체크섬을 OS 보호 저장소로 **봉인**한다. 위조하려면 봉인을
///   만들 수 있어야 하고, 그건 OS 비밀(Windows 사용자 DPAPI / Linux
///   root credential.secret)이 있어야 한다.
///
/// ★ **v1 파일은 못 읽는다.** legacy fallback 을 두면 원래 이식 공격이
///   되살아나므로 두지 않는다. 이 저장소는 아직 배포된 적이 없어
///   마이그레이션 대상이 없다.
const FILE_VERSION: u8 = 2;
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

        // 꼬리: `u32(LE) 봉인 길이 || 봉인된 체크섬`.
        let tail_len_at = bytes
            .len()
            .checked_sub(4)
            .ok_or(KeyringError::CorruptFile("checksum 길이 필드가 없다"))?;
        let sealed_len = u32::from_le_bytes(
            bytes[tail_len_at..]
                .try_into()
                .map_err(|_| KeyringError::CorruptFile("checksum 길이 필드가 잘못되었다"))?,
        ) as usize;
        if sealed_len == 0 || sealed_len > MAX_BLOB_BYTES {
            return Err(KeyringError::CorruptFile("checksum 길이가 범위를 벗어났다"));
        }
        let sealed_at = tail_len_at
            .checked_sub(sealed_len)
            .ok_or(KeyringError::CorruptFile("checksum 위치가 없다"))?;
        let body = &bytes[..sealed_at];
        let sealed = &bytes[sealed_at..tail_len_at];

        // ★ 보관 등급을 body 에서 먼저 읽는다. 아직 검증 전이라 신뢰하지
        //   않지만, **어떤 방식으로 검증할지**를 정하려면 필요하다.
        //   K0 로 낮춰 적어 평문 검증을 유도하는 강등 공격은
        //   `ensure_protection(..., PlaintextPolicy::Reject)` 가 막는다 —
        //   그래서 그 검사를 여기서 **먼저** 한다.
        let claimed_protection = KeyProtection::from_byte(
            *bytes
                .get(FILE_MAGIC.len() + 1)
                .ok_or(KeyringError::CorruptFile("보관 등급 위치가 없다"))?,
        )?;
        ensure_protection(claimed_protection, plaintext_policy)?;
        verify_sealed_checksum(claimed_protection, body, sealed)?;

        let mut reader = Reader::new(body);

        if reader.take(FILE_MAGIC.len())? != FILE_MAGIC {
            return Err(KeyringError::CorruptFile("파일 magic이 다르다"));
        }

        let version = reader.u8()?;
        if version != FILE_VERSION {
            // ★ v1 을 조용히 받아 주지 않는다 — 그 형식의 체크섬은 키가
            //   없어 위조를 막지 못했다. 되살리면 공개키 전용 엔트리로
            //   신원을 바꿔치기할 수 있다.
            return Err(KeyringError::CorruptFile(
                "지원하지 않는 파일 버전이다 — v1 은 봉인되지 않은 체크섬을 써서 위조를 막지 못한다",
            ));
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

        let sealed = seal_checksum(self.protection, &body)?;
        body.extend_from_slice(&sealed);
        put_u32(&mut body, sealed.len() as u32);

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
/// 파일 전체 무결성 표식을 봉인할 때 쓰는 이름.
///
/// ★ signer 별 이름과 **다르다.** 같은 이름을 쓰면 어떤 signer 의 개인키
///   봉인 blob 을 파일 MAC 자리에 놓을 수 있다.
const FILE_MAC_LABEL: &str = "gputeer-keyring-file-mac";

/// 본문의 무결성 표식을 만든다.
///
/// # 왜 체크섬을 봉인하는가
///
/// ★ 2026-08-30 독립 검수 4라운드가 우회를 찾았다. 개인키를 signer 에
///   묶어도, 공격자가 **개인키 blob 을 비워** 공개키 전용 엔트리로
///   만들면 복호 자체가 일어나지 않아 그 방어를 건너뛴다.
///
/// ```text
/// alice 엔트리를 (bob 공개키, 빈 private blob) 으로 바꾼다
///   -> 복호 없음 -> signer 묶기가 작동하지 않는다
///   -> lookup("alice") 가 bob 공개키를 돌려준다
///   -> bob 의 서명이 alice 의 것으로 받아들여진다
/// ```
///
/// 근본 원인은 체크섬이 **키 없는** BLAKE3 라는 것이다 — 파일을 쓸 수
/// 있으면 누구나 다시 계산한다. 봉인하면 위조에 OS 비밀이 필요해진다.
///
/// K0 는 봉인이 없는 등급이므로 평문 체크섬 그대로다 — 그 등급이 무엇을
/// 보호하지 않는지는 이미 이름에 있다.
///
/// # ★ 롤백은 막지 못한다 — 명시적 비보장이다
///
/// 2026-08-30 독립 검수 5라운드 지적. 이 봉인은 **본문 변조**를 막지만
/// **과거의 정상 파일을 통째로 되돌리는 것**은 막지 못한다.
///
/// ```text
/// 공격자가 예전 v2 파일을 복사해 둔다
///   -> 그 파일은 그때 정상적으로 봉인된 것이다
///   -> 나중에 되돌려 놓으면 봉인 검증을 그대로 통과한다
///   -> 폐기·회전된 키가 되살아난다
/// ```
///
/// 막으려면 파일 밖의 **단조 카운터**가 필요하다 — 세대 번호를 봉인
/// 안에 넣어도, 기대값을 어디에 둘지가 같은 문제다. TPM 의 monotonic
/// counter 나 별도 권위 저장소가 있어야 하고 둘 다 이 조각 밖이다.
///
/// `CLAUDE.md` §0.4 는 강제할 수 없는 것을 보장으로 선언하지 말라고
/// 한다. 그래서 반쯤 동작하는 카운터를 만들지 않고 **못 막는다고
/// 적는다.**
///
/// # ★ 같은 사용자(Windows)·root(Linux)는 봉인을 만들 수 있다
///
/// 같은 검수의 정정. K1 의 경계는 처음부터 그렇게 정의돼 있다 —
/// Windows DPAPI 는 **다른 사용자**를, Linux host key 는 **비-root** 를
/// 막는다. 그 경계 안쪽의 공격자는 알려진 entropy/이름으로 직접
/// 봉인을 만들 수 있다.
///
/// 즉 "파일을 쓸 수 있는 공격자는 봉인을 만들 수 없다" 는 **과한
/// 일반화**다. 정확히는 "그 경계 **밖**의 공격자는 못 만든다" 다.
fn seal_checksum(
    protection: KeyProtection,
    body: &[u8],
) -> Result<Vec<u8>, KeyringError> {
    let digest = blake3::hash(body);
    match protection {
        KeyProtection::K0Plaintext => Ok(digest.as_bytes().to_vec()),
        KeyProtection::K1OsProtected => os_protect(FILE_MAC_LABEL, digest.as_bytes()),
        KeyProtection::K2HardwareBacked => Err(KeyringError::UnsupportedProtection(protection)),
    }
}

fn verify_sealed_checksum(
    protection: KeyProtection,
    body: &[u8],
    sealed: &[u8],
) -> Result<(), KeyringError> {
    let expected = blake3::hash(body);
    let actual = match protection {
        KeyProtection::K0Plaintext => sealed.to_vec(),
        KeyProtection::K1OsProtected => os_unprotect(FILE_MAC_LABEL, sealed)?,
        KeyProtection::K2HardwareBacked => {
            return Err(KeyringError::UnsupportedProtection(protection))
        }
    };
    if actual.as_slice() != expected.as_bytes() {
        return Err(KeyringError::CorruptFile("checksum이 일치하지 않는다"));
    }
    Ok(())
}

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
    // ★ `process_group` 은 Unix 확장 트레이트의 메서드다. Windows 는 이
    //   함수를 아예 컴파일하지 않으므로(cfg linux) 개발 기계 빌드가
    //   빠진 import 를 못 잡았다 — x600 빌드에서 드러났다.
    use std::os::unix::process::CommandExt as _;
    use std::process::{Command, Stdio};

    /// 이만큼 안 끝나면 죽인다.
    ///
    /// ★ 상한이 없으면 helper 가 멈췄을 때 Agent 도 무기한 멈춘다.
    ///   이건 로컬 도구를 짧게 부르는 것이므로 초 단위면 충분하다.
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
        // ★ 자식을 **자기 프로세스 그룹**에 넣는다(setpgid). 그래야
        //   후손까지 한 번에 끝낼 수 있다.
        //
        //   ★ 이전 주석은 "setsid() 한다" 고 썼는데 **틀렸다** —
        //     `process_group(0)` 은 새 세션이 아니라 새 프로세스 그룹만
        //     만든다(2026-08-30 독립 검수 6라운드 지적). 그룹 종료
        //     목적에는 충분하지만 문서는 정확해야 한다.
        .process_group(0)
        .spawn()
        // ★ **없는 것**과 **있는데 못 띄운 것**을 구분한다(§3).
        //   전부 `UnsupportedPlatform` 이면 권한 거부나 프로세스 한도
        //   초과도 "이 플랫폼은 지원 안 함" 으로 보고된다 — 고치는
        //   방법이 전혀 다른데 같은 말을 하는 것이다.
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => KeyringError::UnsupportedPlatform,
            _ => KeyringError::OsProtectionFailed(format!("systemd-creds 기동 실패: {error}")),
        })?;

    let pgid = child.id() as i32;

    // ★ 파이프를 **읽는 스레드가 채널로 직접 보낸다.**
    //
    //   이전 설계는 `JoinHandle` 을 기다리는 **보조 스레드를 하나 더**
    //   띄워 시간 상한을 걸었다. 시간 초과 시 reader 와 보조 스레드가
    //   둘 다 남아, 실패 한 번에 최대 3개가 쌓이고 반복 호출하면
    //   선형으로 누적됐다(2026-08-30 독립 검수 6라운드 지적).
    //
    //   채널로 보내면 보조 스레드가 필요 없다 — 시간 초과 시 남는 것은
    //   reader 스레드뿐이고, 그마저도 아래에서 프로세스 그룹을 죽여
    //   파이프가 닫히면 스스로 끝난다.
    let (out_tx, out_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let (err_tx, err_rx) = std::sync::mpsc::channel::<String>();
    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();
    std::thread::spawn(move || {
        use std::io::Read as _;
        let mut buffer = Vec::new();
        if let Some(mut handle) = stdout_handle {
            let _ = handle.read_to_end(&mut buffer);
        }
        let _ = out_tx.send(buffer);
    });
    std::thread::spawn(move || {
        use std::io::Read as _;
        let mut text = String::new();
        if let Some(mut handle) = stderr_handle {
            let _ = handle.read_to_string(&mut text);
        }
        let _ = err_tx.send(text);
    });

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
        kill_process_group(pgid);
        let _ = child.wait();
        return Err(KeyringError::Io(error));
    }

    // 시간 상한 안에서 자식 종료를 기다린다.
    let deadline = std::time::Instant::now() + DEADLINE;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    timed_out = true;
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => {
                kill_process_group(pgid);
                let _ = child.wait();
                return Err(KeyringError::Io(error));
            }
        }
    };

    // ★ 그룹 종료를 **항상** 한다 — 자식이 정상 종료한 경우에도.
    //
    //   이전 설계는 오류 경로에서만 그룹을 죽였다. 그런데 이 수정이
    //   겨냥한 상황은 정확히 **자식은 정상 종료했는데 후손이 파이프를
    //   물고 있는 경우**다 — 그 경로에서는 그룹 종료가 아예 안 불렸다
    //   (2026-08-30 독립 검수 6라운드 지적).
    //
    //   자식이 이미 죽었고 후손도 없으면 `killpg` 는 `ESRCH` 로 실패한다
    //   — 그건 정상이며 아래에서 구분해 다룬다.
    let kill_note = kill_process_group(pgid);
    let _ = child.wait();

    // 이제 파이프가 닫혔으므로 읽기 스레드가 곧 끝난다. 그래도 상한을
    // 건다 — "죽였으니 끝날 것" 을 가정하지 않는다.
    let stdout = out_rx.recv_timeout(DEADLINE).map_err(|_| {
        KeyringError::OsProtectionFailed(format!(
            "systemd-creds stdout 을 상한 안에 읽지 못했다 — 후손이 파이프를 물고 있을 수 있다{kill_note}"
        ))
    })?;
    let stderr = err_rx.recv_timeout(DEADLINE).unwrap_or_default();

    if timed_out {
        return Err(KeyringError::OsProtectionFailed(format!(
            "systemd-creds {} 가 {DEADLINE:?} 안에 끝나지 않았다{kill_note}",
            args.join(" ")
        )));
    }
    let status = status.expect("timed_out 이 false 면 status 가 있다");
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

/// 프로세스 그룹 전체에 `SIGKILL` 을 보낸다.
///
/// # 왜 `kill` 명령이 아니라 `killpg` 인가
///
/// ★ 이전 구현은 `kill -KILL -<pgid>` 를 subprocess 로 불렀고 **결과를
///   통째로 버렸다**(2026-08-30 독립 검수 6라운드 지적). 실행 파일 부재·
///   PATH 문제·인자 해석 차이·비정상 종료가 전부 조용히 무시됐다.
///   그 뒤의 `child.kill()` 은 직접 자식만 죽이므로 대안이 아니다.
///
/// `killpg(2)` 를 직접 부르고 결과를 본다.
///
/// # 반환값
///
/// 진단에 붙일 문구. 정상(죽였거나 이미 없음)이면 빈 문자열이고,
/// 그 밖의 실패면 이유를 담는다 — 나중에 읽기가 시간 초과했을 때
/// "그룹을 못 죽여서" 인지 알 수 있어야 한다.
#[cfg(target_os = "linux")]
fn kill_process_group(pgid: i32) -> String {
    // SAFETY: `killpg` 는 정수 두 개만 받는다. 잘못된 pgid 는 -1 과
    // errno 로 보고되며 메모리 안전성과 무관하다.
    let result = unsafe { libc::killpg(pgid, libc::SIGKILL) };
    if result == 0 {
        return String::new();
    }
    let error = std::io::Error::last_os_error();
    // ESRCH = 그런 그룹이 없다. 자식이 이미 끝났고 후손도 없다는 뜻이라
    // 정상이다.
    if error.raw_os_error() == Some(libc::ESRCH) {
        return String::new();
    }
    format!(" (프로세스 그룹 {pgid} 종료 실패: {error})")
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
