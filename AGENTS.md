# wt — Git worktree workspace manager

## 버전 관리 기준 (Pre-1.0 SemVer)

| 범위 | 언제 올리나 | 예시 |
|------|------------|------|
| **patch** (0.1.x) | 버그 수정, 내부 로직 변경 | 포트 할당 방식 변경, 에러 메시지 수정 |
| **minor** (0.x.0) | 새 기능 추가, 설정 포맷 변경, 1.0 전 breaking change | 새 서브커맨드 추가, .wt.toml 스키마 변경, CLI 인터페이스 변경 |
| **major** (1.0.0 이후) | 1.0 이후 breaking change | 안정화 이후 기존 설정 호환 깨짐 |

아직 `wt`는 1.0에 도달하지 않은 도구로 본다. 사용자-facing CLI, config,
상태 파일 모델이 안정화되기 전까지는 breaking change도 `0.x.0` minor로 표현한다.

버전은 `Cargo.toml`의 `version` 필드에서 관리. 해당하는 변경이 있으면 커밋 시 함께 올린다.
