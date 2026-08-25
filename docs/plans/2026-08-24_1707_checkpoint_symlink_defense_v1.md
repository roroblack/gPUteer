# Checkpoint symlink defense v1

- 작성/실측: 2026-08-25 (Windows 개발기)
- 범위: `gputeer-checkpoint`의 파일 내용 읽기 경로에 Windows reparse-point 방어 연결
- 비범위: `write_once`를 포함한 쓰기 경로, 디렉터리 열거/GC 삭제 정책, Linux 방어 활성화

## 설계와 계층 결정

`crates/checkpoint/src/platform.rs`에 다음 플랫폼 추상화를 둔다.

```rust
pub(crate) fn open_beneath_for_read(
    root: &Path,
    relative: &Path,
) -> io::Result<File>
```

호출자는 반환된 `File`만 읽어야 하며, 검증된 경로 문자열을 다시 열면 안 된다.
공통 `read_beneath(root, relative)`가 이 핸들을 끝까지 읽는다. 절대 경로,
`..`, `.`, 빈 경로는 모든 플랫폼에서 먼저 거부한다.

- Windows: 타깃 의존성 `gputeer-runtime-windows`의 `open_beneath`에 위임한다.
  이 함수가 각 경로 구성요소의 reparse point를 연 핸들 기준으로 거부한다.
  기존 함수가 마지막 파일에 `OPEN_ALWAYS`를 쓰므로, 읽기 API의 정상적인
  `NotFound`가 파일 생성으로 바뀌지 않도록 `symlink_metadata`로 존재 여부를
  먼저 확인한다. 링크 방어 판정 자체는 이 선행 검사에 의존하지 않는다.
- Linux: `libc::SYS_openat2`와
  `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS`를 사용하는
  구현을 작성했다. root fd도 `O_NOFOLLOW | O_DIRECTORY | O_PATH`로 연다.
  그러나 이 구현은 아직 호출하지 않는다.
- 기타 플랫폼: 상대 경로 문법만 제한한 뒤 평범한 `File::open`을 사용한다.
  링크 방어는 없다.

의존 방향은 다음과 같다.

```text
gputeer-checkpoint --cfg(windows)--> gputeer-runtime-windows
                                      --> gputeer-runtime-policy
```

`runtime-windows`에는 `checkpoint` 의존성이 없으므로 순환이 없다. Windows
의존성과 Linux의 `libc` 의존성을 각각 target-specific dependency로 두어 다른
플랫폼이 불필요한 `runtime-windows`를 빌드하지 않게 했다.

## 실제 연결한 읽기 경로

파일 내용을 읽어 체크포인트 선택·검증 결과에 영향을 주는 지점을 범위로 잡았다.

1. `CheckpointManifest::verify_files`: 매니페스트가 가리키는 체크포인트 데이터.
2. `load_valid_manifest`: 재개 후보의 `manifest.json`.
3. `gc_partial`: 보존할 등록 파일을 판정하기 위한 `manifest.json`.
4. `state_recorded`, `publication_failed`: durability 상태 및 공개 실패 마커.
5. `read_pointer`: `LATEST` 포인터.

`atomic.rs`의 `write_once` 내부 두 `fs::read`는 이미 존재하는 대상과 새 데이터가
같은지 비교하는 쓰기 동작의 일부다. 요청대로 건드리지 않았다. `read_dir`,
`symlink_metadata` 등 디렉터리 열거와 GC 삭제 정책도 이번 파일 내용 읽기 연결
범위 밖이다.

## Linux의 명시적인 미연결 상태

Linux 개발 타깃은 의도적으로 평범한 `File::open`을 계속 사용하므로 symlink를
따라가며 현재 무방비다. 다음 공개 상태값으로 이 사실을 코드에 드러낸다.

```rust
pub const LINUX_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE: bool = false;
pub const CHECKPOINT_READ_LINK_DEFENSE_ACTIVE: bool = /* Windows만 true */;
```

`platform::tests::rollout_status_is_windows_only`는 Linux 값이 `false`임을 직접
단언한다. 나중에 Linux 구현을 연결하면서 상태만 바꾸면 이 테스트가 실패하므로,
테스트와 문서를 함께 갱신해야 한다.

이 개발기는 설치된 Rust 타깃이 `x86_64-pc-windows-msvc` 하나뿐이다. Linux
구현은 Windows에서 작성됐고 Linux 컴파일 및 실제 `openat2` 동작을 검증하지
못했다. Linux 타깃 의존 트리 확인도 미캐시 크레이트 다운로드 단계에서 네트워크
차단으로 완료되지 않았다. 따라서 Linux 구현이 동작한다고 주장하지 않는다.

## 검증 결과

### 정상 테스트

- `cargo test -p gputeer-checkpoint`: PASS.
  - 새 상태 고정 단위 테스트 1건 PASS.
  - 새 Windows junction 통합 테스트 2건 PASS.
  - 기존 checkpoint 테스트 전부 PASS.
- `cargo test -p gputeer-runtime-windows`: PASS.
  - 단위 테스트 4건, `artifact_beneath` 3건, `commit_cap` 2건 PASS.

실행 환경에서 `cargo`가 `PATH`에 없어 실제 명령은
`C:\Users\playdata2\.cargo\bin\cargo.exe` 절대 경로로 같은 인자를 주어
실행했다.

### Junction 실측

`mklink /J`로 두 경우를 실제 생성했다. 테스트는 junction을 통한 평범한
`std::fs::read`가 외부 파일을 읽을 수 있음을 먼저 확인해 비공허성을 확보한다.

1. 체크포인트 안 `payload` junction이 외부 데이터 디렉터리를 가리키는 경우:
   `CheckpointManifest::verify_files`가 오류로 거부했다.
2. 체크포인트 root의 후보 디렉터리 자체가 외부 체크포인트를 가리키는 junction인
   경우: `find_resume_point`가 외부 매니페스트를 후보로 채택하지 않았다.

### 뮤테이션 검증과 원복

1. Windows 구현의 `gputeer_runtime_windows::open_beneath` 위임을 일시적으로
   `File::open(root.join(relative))`로 바꿨다.
   `checkpoint_data_read_through_junction_is_rejected`가 예상대로 실패했고 외부
   데이터를 읽었다는 assertion이 발동했다. 원복 뒤 같은 테스트가 PASS했다.
2. `LINUX_CHECKPOINT_READ_LINK_DEFENSE_ACTIVE`를 일시적으로 `true`로 바꿨다.
   `platform::tests::rollout_status_is_windows_only`가 예상대로 실패했다. 원복 뒤
   같은 테스트가 PASS했다.

두 뮤테이션은 모두 코드에서 원복했고 재검증했다. 테스트가 만든 junction과 파일은
각 `tempfile` 임시 디렉터리 안에만 있었고 테스트 종료 시 제거됐다.


---

## ★ 2026-08-24 감독자 수정 — 읽기가 파일을 만들던 문제

초기 구현은 `runtime-windows` 의 `open_beneath` 를 읽기 경로에 그대로
썼다. 그런데 그 함수는 최종 대상에 `OPEN_ALWAYS` 를 쓴다 — **없으면
만든다.** 쓰기 경로용 계약이다. 그대로 읽기에 쓰면 "없는 체크포인트를
읽어본다" 는 정상 동작이 빈 파일을 만들어 버린다.

초기 구현은 이를 `std::fs::symlink_metadata()` 사전 존재 검사로
막으려 했다. **그 자체가 TOCTOU 다.** 검사와 열기 사이에 파일이
사라지면 여전히 만들어진다. TOCTOU 방어를 목적으로 하는 모듈에
TOCTOU 임시방편을 붙이는 것은 자기모순이라 채택하지 않았다.

대신 `runtime-windows` 에 읽기 전용 변형을 추가했다.

```rust
pub fn open_beneath_read_only(root: &Path, relative: &Path) -> io::Result<File>
// -> open_beneath_with(root, relative, GENERIC_READ, OPEN_EXISTING)
```

`open_beneath` 와 본체를 공유하되(`open_beneath_with`) 접근 권한과
disposition 만 다르다. 존재 여부 판정을 **커널에 맡기므로** 확인/사용
사이의 창이 없다. checkpoint 쪽 사전 검사는 제거했다.

회귀 테스트 2건을 `crates/checkpoint/tests/symlink_defense.rs` 에
추가했다.

- `reading_a_missing_file_must_not_create_it` — 없는 파일을 읽으면
  `NotFound` 이고 파일이 생기지 않는다.
- `the_write_variant_still_creates_by_contract` — 대조군. 쓰기용
  `open_beneath` 는 여전히 만든다. 읽기 전용 변형을 추가하면서 기존
  계약을 깨지 않았음을 고정한다.

`gputeer-checkpoint`·`gputeer-runtime-windows` 테스트 통과.


---

## ★ 검수 2라운드와 그 뒤에 드러난 결함들

독립 검수가 `CHANGES_REQUESTED` 를 냈다. 이름 기반 사전 검사 두 곳이
남아 있다는 지적이었다. 그 지적을 처리하는 과정과, 그 뒤 반복 실행에서
드러난 것들을 그대로 남긴다. 결과만 적으면 다음 사람이 같은 함정을
다시 밟는다.

### (1) `writer.rs` 의 `exists()` — 제거

`read_beneath` 실패가 이미 `Ok(None)` 으로 매핑되므로 완전한 중복이었다.
제거했다.

### (2) `atomic.rs` 의 `manifest_exists` — **제거하면 안 되는 것이었다**

검수 지적을 그대로 받아 변수를 없앴다가 컴파일 오류로 발견했다. 이것은
사전 검사가 아니라 **GC 판정 입력**이다.

```rust
let should_remove = !manifest_exists || (is_tmp && !is_registered);
```

매니페스트가 없으면 **전부 삭제**한다. 변수를 없애면 "있지만 손상된"
매니페스트가 "없음" 과 같아져 **데이터 파일까지 지워진다.** 유실 버그다.

대신 `retry_tolerating_race` 의 `Ok(None)` 이 정확히 NotFound 라는 점을
이용해 **한 번의 읽기**에서 존재 여부와 내용을 함께 얻도록 재구성했다.
TOCTOU 는 없애고 "없음 vs 손상" 구분은 보존했다.

부수 효과로 더 안전해졌다. 매니페스트가 reparse point 나 디렉터리면
예전에는 `symlink_metadata` 의 `is_file()` 이 거짓이 되어 조용히 "없음"
으로 떨어지고 **전체 삭제**로 갔다. 이제는 오류로 전파된다. 링크된
매니페스트로 GC 를 유도해 데이터를 지우게 만드는 시나리오가 막혔다.

### (3) `fs::read` -> Win32 열기 치환에서 std 가 해주던 것들

전체 워크스페이스 실행에서 `write_failure` 가 **1회** 실패한 것이
시작이었다. 단독 실행은 통과해서 넘어갈 뻔했으나 반복 실행으로 재현했다.
**10회 중 4회** 실패였다.

원인은 하나가 아니라 셋이었고, 하나를 고칠 때마다 다음이 드러났다.

| 조치 | 실패율 |
|---|---:|
| (없음) | 4/10 |
| `ERROR_NOT_FOUND` 정규화 — 열기 실패 경로 | 3/12 |
| `FILE_SHARE_DELETE` 추가 | 1/15 |
| `ERROR_NOT_FOUND` 정규화 — 정보 조회 실패 경로 | **0/25** |

- **오류 코드 정규화.** Rust 는 `ERROR_FILE_NOT_FOUND`(2)·
  `ERROR_PATH_NOT_FOUND`(3) 만 `ErrorKind::NotFound` 로 매핑하고
  **`ERROR_NOT_FOUND`(1168)** 는 원시 오류로 둔다. 체크포인트 GC 는
  "없음" 을 정상 경합으로 다루므로, 1168 이 새어 나가면 정상 경합이
  치명적 오류가 된다. **두 지점** 모두 정규화해야 했다 — `CreateFileW`
  실패와, 핸들을 연 뒤 `GetFileInformationByHandle` 실패다. 후자는
  `FILE_SHARE_DELETE` 를 허용한 뒤에야 드러났다(여는 데 성공하고 그
  직후 지워지는 경합).
- **공유 플래그.** std 의 `File::open` 은
  `READ | WRITE | DELETE` 를 쓴다. `DELETE` 를 빼면 우리가 읽는 동안
  다른 쪽의 삭제가 `ERROR_SHARING_VIOLATION`(32) 이 된다. 읽기만 하는
  쪽이 남의 삭제를 막을 이유가 없고, 공유 모드는 reparse 탐지와
  무관하다.
- **생성 동작.** `OPEN_ALWAYS` 는 없으면 만든다(쓰기 계약).
  읽기에는 `OPEN_EXISTING` 이어야 한다. 이건 검수 전에 잡았다.

**교훈**: `std::fs` 를 커스텀 Win32 열기로 바꾸는 것은 단순 치환이
아니다. std 가 조용히 해주던 오류 정규화·공유 플래그·생성 동작을 전부
직접 맞춰야 한다. 다음에 같은 치환을 한다면 이 세 가지를 먼저 확인해라.

**그리고 낮은 확률의 실패를 적은 횟수로 검증하지 마라.** 1/15 짜리를
10회만 돌리면 절반 확률로 "고쳐졌다" 는 잘못된 결론이 나온다.

### 최종 검증

- `write_failure` 25회 반복 실패 0건
- `cargo test --workspace --exclude gputeer-runtime-windows` 통과
- `cargo test -p gputeer-runtime-windows` 통과
- `check_docs.py` 오류 0
