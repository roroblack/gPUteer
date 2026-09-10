//! hex 문자열을 바이트로 — **구조적으로 패닉하지 않는다.**
//!
//! # 왜 이 모듈이 있나
//!
//! ★★ 2026-09-10 결함 ⑬. 같은 모양의 hex 해석이 저장소에 **일곱 곳** 있었다
//!   (`cli` 의 submit·issue_grant·stage_job·import_manifest·import_inventory,
//!   `coordinator`·`agent` 의 `hex_decode`). 전부 이렇게 생겼다:
//!
//!   ```text
//!   if hex.len() != 64 { 거부 }
//!   for i in 0..32 { u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16) }
//!   ```
//!
//!   `len()` 은 **바이트** 수이고 슬라이스도 **바이트**로 자른다. 한글 한
//!   글자(3바이트) + '1' 61개 = 정확히 64바이트라 길이 검사를 통과하고,
//!   첫 `[0..2]` 가 문자 경계를 갈라 **패닉**한다. 사용자 입력이 닿는
//!   자리였다(명령줄 seed·키 파일·inventory 문서).
//!
//!   독립 검수가 `submit` 에서 찾았고 그곳만 고쳤다. 재검수가 `issue-grant`
//!   에서 **같은 패닉**을 다시 찾았고, grep 해 보니 다섯 곳이 더 있었다.
//!   ★ 한 곳에만 고치면 나머지가 남는다 — 이 결함이 일곱 곳에 생긴 방식이
//!     바로 그 복사였다. 그래서 도우미를 **한 곳**에 둔다.
//!
//! # 어떻게 패닉을 없앴나
//!
//! 문자열을 **자르지 않는다.** 바이트 배열을 한 바이트씩 읽어 니블로 바꾼다.
//! 자르는 연산이 없으니 문자 경계를 가를 수가 없다 — 검사 순서에 기대는
//! 방어(먼저 ASCII 인지 보고 나서 자른다)보다 한 단계 강하다.
//!
//! ★ 옛 코드와 다른 점 하나 — `u8::from_str_radix` 는 **부호**를 받는다
//!   (`"+f"` 가 15 로 읽힌다). 이 모듈은 `0-9a-fA-F` 만 받는다. 좁히는
//!   방향이라 정상 입력은 그대로 통과한다.

/// hex 해석 실패. 어느 칸이 틀렸는지 담는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HexError {
    /// 바이트 길이가 기대와 다르다.
    Length { expected: usize, actual: usize },
    /// 가변 길이 해석에서 바이트 길이가 홀수다.
    OddLength { actual: usize },
    /// hex 가 아닌 바이트가 있다(처음 만난 것).
    NotHex { byte: u8, index: usize },
}

impl std::fmt::Display for HexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Length { expected, actual } => {
                write!(f, "hex 는 {expected}자리여야 한다(받은 바이트 {actual})")
            }
            Self::OddLength { actual } => {
                write!(f, "hex 문자열 길이가 홀수다(바이트 {actual})")
            }
            Self::NotHex { byte, index } => {
                write!(f, "{index}번째 바이트 0x{byte:02x} 는 hex 가 아니다")
            }
        }
    }
}

impl std::error::Error for HexError {}

/// 정확히 `N` 바이트를 뜻하는 `2N` 자리 hex 를 읽는다.
pub fn decode_fixed<const N: usize>(hex: &str) -> Result<[u8; N], HexError> {
    let bytes = hex.as_bytes();
    if bytes.len() != N * 2 {
        return Err(HexError::Length {
            expected: N * 2,
            actual: bytes.len(),
        });
    }
    let mut out = [0u8; N];
    for (index, slot) in out.iter_mut().enumerate() {
        *slot = pair(bytes, index * 2)?;
    }
    Ok(out)
}

/// 길이가 짝수인 hex 를 읽는다. 길이 제약은 부르는 쪽이 건다.
pub fn decode_even(hex: &str) -> Result<Vec<u8>, HexError> {
    let bytes = hex.as_bytes();
    if bytes.len() % 2 != 0 {
        return Err(HexError::OddLength {
            actual: bytes.len(),
        });
    }
    (0..bytes.len() / 2)
        .map(|index| pair(bytes, index * 2))
        .collect()
}

fn pair(bytes: &[u8], at: usize) -> Result<u8, HexError> {
    Ok((nibble(bytes[at], at)? << 4) | nibble(bytes[at + 1], at + 1)?)
}

fn nibble(byte: u8, index: usize) -> Result<u8, HexError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(HexError::NotHex { byte, index }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★★ 결함 ⑬ 의 입력 그대로 — 길이는 맞는데 첫 글자가 3바이트다.
    #[test]
    fn a_multibyte_character_that_fits_the_length_is_an_error_not_a_panic() {
        let input = format!("가{}", "1".repeat(61));
        assert_eq!(input.len(), 64, "전제: 바이트 길이는 맞아야 한다");
        assert_eq!(
            decode_fixed::<32>(&input),
            Err(HexError::NotHex {
                byte: 0xea,
                index: 0
            })
        );
        // 가변 길이 쪽도 같은 입력으로 패닉하지 않는다.
        assert!(matches!(
            decode_even(&input),
            Err(HexError::NotHex { index: 0, .. })
        ));
    }

    /// 문자가 중간에 끼어도 같다 — 첫 칸만 보는 구현을 잡는다.
    #[test]
    fn a_multibyte_character_in_the_middle_is_found_where_it_is() {
        let input = format!("{}가{}", "0".repeat(31), "1".repeat(30));
        assert_eq!(input.len(), 64);
        assert!(matches!(
            decode_fixed::<32>(&input),
            Err(HexError::NotHex { index: 31, .. })
        ));
    }

    /// 옛 `from_str_radix` 는 부호를 받았다. 여기서는 거부한다.
    #[test]
    fn a_sign_is_not_a_hex_digit() {
        assert!(matches!(
            decode_fixed::<1>("+f"),
            Err(HexError::NotHex { byte: b'+', index: 0 })
        ));
    }

    #[test]
    fn length_and_odd_length_are_named() {
        assert_eq!(
            decode_fixed::<2>("abc"),
            Err(HexError::Length {
                expected: 4,
                actual: 3
            })
        );
        assert_eq!(decode_even("abc"), Err(HexError::OddLength { actual: 3 }));
    }

    /// 정상 입력 — 대소문자 둘 다, 값은 손으로 적는다.
    #[test]
    fn valid_hex_decodes_in_either_case() {
        assert_eq!(decode_fixed::<4>("00ff10Ab"), Ok([0x00, 0xff, 0x10, 0xab]));
        assert_eq!(decode_even("DEADbeef"), Ok(vec![0xde, 0xad, 0xbe, 0xef]));
        assert_eq!(decode_even(""), Ok(vec![]));
    }
}
