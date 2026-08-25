# Watch cursor continuity kernel v1

- 범위: `proto/control.proto`의 기존 `Cursor`, `WatchEvent`, `WatchReset` 계약을 소비하는
  구독자 관점의 순수 연속성 판정
- 배치: `crates/protocol/src/watch_continuity.rs`
- 비범위: watch transport, 저장소, 재동기화 실행, snapshot 생성, 상태 전이, Raft 규칙 추가

## 1. 근거와 경계

`WatchRequest.from`은 없으면 현재 시점부터 구독하고, 있으면 해당 cursor 이후부터
이어받는다고 정의한다. `WatchEvent`는 event 또는 reset과 cursor를 담는다.
특히 `WatchReset` 주석은 snapshot 등으로 cursor가 무효화되었을 때 구독자가 **반드시
전체 재동기화**해야 하며 그 목적이 **조용한 누락 방지**라고 명시한다.

kernel은 이 세 기존 wire 타입의 직접 소비자이므로 schema/canonical/signing을 소유하는
`gputeer-protocol`에 둔다. 다른 크레이트에 wire 타입을 복제하거나 adapter 규칙을 만들지
않고, 새 protobuf 필드도 추가하지 않는다. 기존 공개 API는 그대로 두고 새 모듈과 export만
추가한다.

DoD-41 hard-filter, DoD-45 best-fit, DoD-54 effective-replica, DoD-55 GPU scope의
공통 스타일을 따른다.

- 호출자가 제공한 고정 입력만 읽는 순수 함수다.
- 누락되거나 규범으로 해소할 수 없는 사실은 typed 결과로 fail closed한다.
- 결과는 원인을 포함하는 비교 가능한 전체 report다.
- 순서가 집합 의미인 곳만 canonicalize하고, 도메인 의미가 있는 순서는 보존한다.

## 2. API와 판정

`evaluate_watch_continuity(previous, events) -> WatchContinuityReport`는 다음 outcome 중
하나를 반환한다.

| 입력/관계 | 결과 |
|---|---|
| 이전 cursor가 없고 첫 정상 event가 있음 | 첫 cursor를 현재 구독의 anchor로 수용 |
| 같은 term, `received.index == previous.index + 1` | `Continuous` |
| 같은 term, index가 2 이상 건너뜀 | `Gap`, 전체 재동기화 필요 |
| 같은 term, 같은 index | `DuplicateIndex`, 전체 재동기화 필요 |
| 같은 term, index 후퇴 | `IndexRegression`, 전체 재동기화 필요 |
| term 후퇴 | `TermRegression`, 전체 재동기화 필요 |
| `WatchReset` 존재 | `WatchReset`, 반드시 전체 재동기화 |
| kind 또는 cursor 누락 | `Indeterminate`, 전체 재동기화 필요 |
| term 증가 | `Indeterminate(TermAdvanced)`, 전체 재동기화 필요 |

안전하지 않은 report에는 이어받을 cursor를 제공하지 않는다. `WatchReset.resume_from`도
전체 재동기화 자체를 대신할 수 없으므로 kernel이 자동 수용하지 않는다.

## 3. 규범이 정하지 않은 것

`Cursor`에는 `index`와 `term`만 있고 term 변경 시 index가 계속 증가하는지, 다시 시작하는지,
또는 두 값 사이에 다른 제약이 있는지 정의되어 있지 않다. 따라서 term 증가를 Raft 관행으로
해석해 연속이라고 승인하지 않는다. `prev_entry_hash`, quorum configuration, commit
certificate, leader ID 같은 증거도 schema에 없으므로 입력이나 규칙으로 추가하지 않았다.

index/term의 0 값도 schema가 금지하지 않으므로 별도 malformed 규칙을 발명하지 않는다.
이 kernel은 제공된 subscription boundary 이후의 인접 수신 cursor만 판정하며, transport가
첫 event 이전에 이미 버렸는지는 기존 필드만으로 증명할 수 없다.

## 4. 순서 판단

정상 `WatchEvent`의 배열 순서는 수신 스트림 자체다. 이를 index로 정렬하면 실제 역행이나
재정렬을 숨길 수 있으므로 절대 canonicalize하지 않는다. 네 정상 event의 4! 순열 테스트는
원래 수신 순서 하나만 연속이고 나머지는 모두 재동기화로 닫히는지 확인한다.

반면 `WatchReset`은 위치와 무관하게 규범상 재동기화를 강제하는 지배 사실이다. reset 하나와
정상 event 셋의 4! 모든 순열에서 `WatchContinuityReport` 전체를 `assert_eq!`로 비교해 완전히
같은 `WatchReset` report인지 확인한다. 이것이 이 도메인에서 정직하게 순서 무관해야 하는
부분이다.

## 5. 구현 및 테스트 결과

추가 파일:

- `crates/protocol/src/watch_continuity.rs`: 순수 kernel, report/outcome, typed 원인
- `crates/protocol/tests/watch_cursor_continuity.rs`: 연속, anchor, gap, 중복/역행,
  reset 4! 순열, 정상 stream 4! 순열, 누락 필드, term 증가 fail-closed 테스트
- `crates/protocol/src/lib.rs`: 새 API re-export

검증:

- `cargo build -p gputeer-protocol`: PASS
- `cargo test -p gputeer-protocol`: PASS
- 신규 integration test: 8 passed

뮤테이션 검증:

1. production의 gap 반환 분기를 임시로 제거해 큰 index도 연속 처리하게 했다.
   `gap_requires_resynchronization`이 `Continuous` 대 `Gap` 불일치로 실패했다.
   분기를 원복한 뒤 같은 지정 테스트가 통과했다.
2. production의 term 증가 `Indeterminate` 분기를 임시로 제거했다.
   `advanced_term_is_indeterminate_without_a_normative_index_relation`이 `Continuous` 대
   `Indeterminate(TermAdvanced)` 불일치로 실패했다. 분기를 원복한 뒤 같은 지정 테스트가
   통과했다.

두 뮤테이션은 모두 production 코드에서 제거되었고 최종 전체 테스트로 원복 상태를 다시
확인한다.
