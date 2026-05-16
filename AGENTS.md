# wt — Git worktree workspace manager

## 버전 관리 기준 (Pre-1.0 SemVer)

| 범위 | 언제 올리나 | 예시 |
|------|------------|------|
| **patch** (0.1.x) | 버그 수정, 내부 로직 변경 | 포트 할당 방식 변경, 에러 메시지 수정 |
| **minor** (0.x.0) | 새 기능 추가, 설정 포맷 변경, 1.0 전 breaking change | 새 서브커맨드 추가, .wt.toml 스키마 변경, CLI 인터페이스 변경 |
| **major** (1.0.0 이후) | 1.0 이후 breaking change | 안정화 이후 기존 설정 호환 깨짐 |

아직 `wt`는 1.0에 도달하지 않은 도구로 본다. 사용자-facing CLI, config,
상태 파일 모델이 안정화되기 전까지는 breaking change도 `0.x.0` minor로 표현한다.

버전은 `Cargo.toml`의 `version` 필드에서 관리하되, 일반 개발 커밋에서는 올리지 않는다.
기본 개발 브랜치는 `develop`이고, `master`는 릴리즈된 코드만 담는 브랜치다.
릴리즈할 때 `develop`에서 release 브랜치를 만들고, 릴리즈 PR이 `master`로 merge되기 전에
그 릴리즈에 포함된 변경 중 가장 큰 범위에 맞춰 version을 한 번 올린다.

릴리즈 PR이 `master`에 merge되고 tag가 생성되면, version bump commit을 다시 `develop`에
merge해서 다음 릴리즈 기준점이 두 브랜치에서 어긋나지 않게 한다.
