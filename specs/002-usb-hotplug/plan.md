# Implementation Plan: Simple USB Hotplug Detection

**Branch**: `002-usb-hotplug` | **Date**: 2025-09-15 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/002-usb-hotplug/spec.md`

## Summary
개인 프로젝트니까 심플하게: 기존 폴링 대신 `nusb::watch_devices()` 사용해서 USB 디바이스 연결/해제를 이벤트로 받기. 복잡한 마이그레이션이나 추상화 없이 직접 구현.

## Technical Context
**Language/Version**: Rust 1.75+ (현재 코드베이스)
**Primary Dependencies**: nusb (이미 사용중), tokio (이미 사용중)
**Storage**: N/A (메모리만)
**Testing**: cargo test (기존 방식)
**Target Platform**: Linux/Windows (이미 지원)
**Project Type**: single (기존 서버)
**Performance Goals**: 폴링 오버헤드 제거, 빠른 감지
**Constraints**: 개인 프로젝트 - 심플하게, 오버 엔지니어링 금지
**Scale/Scope**: 1인 개발, 기본적인 핫플러그 대체

## Constitution Check (SIMPLIFIED)
**Simplicity**: ✓ 1개 프로젝트, 직접 구현, 기존 타입 사용, 패턴 없음
**Architecture**: ✓ 기존 모듈 수정 (개인 프로젝트니까 OK)
**Testing**: ✓ 기본 테스트만, 실용적으로
**Observability**: ✓ 기존 tracing 사용
**Versioning**: ✓ 기존 방식 따름

## Phase 0: 심플 리서치
기본적인 것만:
- `nusb::watch_devices()` 사용법
- 크로스 플랫폼 호환성
- tokio 통합 방법

**Output**: research.md (필수 정보만)

## Phase 1: 심플 설계
복잡한 것 없이:
- 기존 DeviceInfo, DiscoveryEvent 타입 그대로 사용
- 기본적인 contracts 사용 - 핫플러그 인터페이스 정의
- quickstart.md - 기본 테스트 스텝만

**Output**: quickstart.md (기본 테스트 방법)

## Phase 2: 심플 태스크 (10개 이하)
복잡한 추상화 없이:
1. nusb::watch_devices() 사용법 조사
2. 기본 실패 테스트 작성
3. DiscoveryCoordinator에 핫플러그 통합
4. 폴링 타이머 로직 제거
5. 실제 USB 디바이스로 테스트
6. 문서 업데이트

**전략**: 직선적이고 심플하게, 병렬 실행 복잡성 없음

## Progress Tracking
**Phase Status**:
- [x] Phase 0: 심플 리서치 완료
- [x] Phase 1: 기본 설계 완료
- [x] Phase 2: 심플 태스크 계획 완료

**Gate Status**:
- [x] 심플함 유지 (개인 프로젝트 집중)
- [x] 오버 엔지니어링 없음
- [x] 계획 완료 - /tasks 준비됨

---
*개인 프로젝트용 심플 접근법 - 오버 엔지니어링 금지*