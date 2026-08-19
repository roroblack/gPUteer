# 2026-08-20_0424_check_schema_ci_연결_v1

## 계획

`.github/workflows/canonical-schema-check.yml`을 추가해 `main` 대상
`push`·`pull_request`에서 canonical 참조 구현, schema 검사, Rust
워크스페이스 빌드·테스트, evidence 스키마 검사를 순서대로 실행한다.
Rust는 `Cargo.toml`의 `rust-version = "1.89"`에 맞추고, Python은
`protobuf`·`blake3`를 설치한다. Cargo 빌드는 `protoc-bin-vendored`를
사용하지만 `check_schema.py`가 PATH의 `protoc`를 직접 요구하므로 CI에
`protobuf-compiler`도 설치한다.

## 범위

- Ubuntu 최신 러너와 Rust 1.89, Python 3 설정
- `gputeer-runtime-windows`를 제외한 build/test 실행
- GitHub API 호출, 커밋·푸시, 기존 지침·evidence·history 문서 수정 없음

## 검증 방법

워크플로의 다섯 명령을 로컬에서 동일한 순서로 실행하고, PyYAML로
워크플로 YAML을 파싱한다. `act`가 설치되어 있으면 확인하되, 설치되어
있지 않으면 추가 설치하지 않는다.
