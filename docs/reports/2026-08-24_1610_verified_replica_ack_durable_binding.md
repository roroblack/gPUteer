# verified ReplicaAck durable checkpoint/root binding 구현

- 계획: `docs/plans/2026-08-24_1556_verified_replica_ack_durable_binding_v1.md`의 저장 슬라이스
- 스트림: Coordinator
- 시각: 2026-08-24 16:10 KST

## 수행

- `CoordinatorReplicaAckStore`와 `coordinator_replica_acks`를 추가했다.
- 공개 save API를 `&Verified<pb::ReplicaAck>`로 제한하고 load는 raw binding으로 유지했다.
- 모든 durable 대조와 insert를 한 `BEGIN IMMEDIATE` transaction 안에서 수행한다.
- DoD-52 CheckpointManifest의 validated crate-private helper로 checkpoint 존재와 exact
  BLAKE3-256 root를 대조한다.
- immutable observation replay, BLAKE3 complete-body hash, u64 big-endian time,
  corruption/rollback fail-closed 테스트를 추가했다.
- 상태 전이, effective replica count, membership/failure-domain 판정은 추가하지 않았다.

## 검증

- `cargo test -p gputeer-coordinator replica_ack_store --no-fail-fast`: 7 passed, 0 failed.
- `cargo test -p gputeer-coordinator --no-fail-fast`: 121 unit, 4 integration,
  1 compile-fail doctest passed; 0 failed.
- `cargo build --workspace --exclude gputeer-runtime-windows`: 성공.
- `cargo test --workspace --exclude gputeer-runtime-windows`: 성공, 0 failed,
  기존 ignored test 1건.
- `python scripts/verify_evidence.py`: schema 위반 0.
- canonical vector 재생성 대조: 48개 일치.
- `git diff --check`: whitespace error 없음.

뮤테이션은 (1) DoD-52 anchor root guard 제거, (2) load body-hash guard 제거를 실제 적용했다.
각각 지정 negative test가 실패했고 원복 뒤 재통과했다. 상세 실측은 계획 문서 §8에 기록했다.

## 자체 재검토와 제한

- PK 일부인 `acked_at` BLOB을 손상시킨 행은 원래 key 기반 get으로 찾을 수 없다는 테스트 설계
  결함을 발견했다. 해당 케이스를 전체 list의 fail-closed 검사로 바꿨다.
- BLAKE3-256 외 algorithm과 32바이트가 아닌 root를 모두 거부하는지 다시 확인했다.
- `rustfmt` component가 설치돼 있지 않아 cargo fmt는 실행하지 못했다. build/test는 모두 통과했다.
- 사용자 지시에 따라 신규 DoD evidence와 history는 만들거나 수정하지 않았다. 따라서 이 리포트는
  독립 검수된 DoD PASS evidence를 주장하지 않는다.
- membership, failure-domain, freshness, current liveness, effective count와 checkpoint 전이는 후속이며
  이번 저장 성공이 유효 replica나 `MIRRORED`를 뜻하지 않는다.
