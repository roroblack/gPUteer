# gputeer

**gPUteer 구현 저장소.**

개인 PC · 연구실 서버 · 클라우드 GPU 를 하나의 사설 Compute Pool 로 묶고,
노드 장애 시 지속 보존된 상태로 **다른 GPU 에서 작업을 이어가는** 오케스트레이션 플랫폼.

## 작업 전에 반드시 읽을 것

```text
1. RULE.md      프로세스 규칙   (필독)
2. CLAUDE.md    도메인 안전 원칙 (필독)
3. docs/README.md               문서 지도
```

**어떤 문서가 적용되는지 확인하지 못하면 파일 변경을 시작하지 않는다.**

## 구조

```text
RULE.md              프로세스 규칙
CLAUDE.md            도메인 안전 원칙 · 현재 상태
proto/               와이어 스키마 (규범)
docs/protocol/       서명 · 상태 전이 (규범)
docs/                문서 지도는 docs/README.md
crates/              Rust 구현
apps/console/        React + Tauri UI
python/gputeer_ml/   ML 어댑터
tools/canonical/     canonical 인코딩 참조 구현
tests/               contract · integration · chaos · security · compatibility · vectors
scripts/             검증 스크립트
legacy/              대체된 코드 보존소
```

## 기준선

`../gputeer_master_plan_FINAL.md` — **읽기 전용. 이 저장소에서 수정하지 않는다.**

구현이 기준선과 어긋나면 `docs/plans/` 에 사유를 적는다.
기준선을 바꿔야 하는 결정은 `RULE.md` §8 절차를 따른다.

## 검증

```bash
python tools/canonical/reference_canonical.py --self-test
python tools/canonical/reference_canonical.py --verify tests/vectors/canonical_v1.json
python scripts/verify_evidence.py
cargo test --workspace          # 구현 착수 후
```

## 현재 상태

**구현 미착수.** P0 스파이크부터 시작한다 — 기준선 §32, `CLAUDE.md` §5.
