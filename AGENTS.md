# wt — Git worktree workspace manager

## 버전 관리 기준 (Pre-1.0 SemVer)

| 범위 | 언제 올리나 | 예시 |
|------|------------|------|
| **patch** (0.1.x) | 버그 수정, 내부 로직 변경 | 포트 할당 방식 변경, 에러 메시지 수정 |
| **minor** (0.x.0) | 새 기능 추가, 설정 포맷 변경, 1.0 전 breaking change | 새 서브커맨드 추가, .wt.toml 스키마 변경, CLI 인터페이스 변경 |
| **major** (1.0.0 이후) | 1.0 이후 breaking change | 안정화 이후 기존 설정 호환 깨짐 |

아직 `wt`는 1.0에 도달하지 않은 도구로 본다. 사용자-facing CLI, config,
상태 파일 모델이 안정화되기 전까지는 breaking change도 `0.x.0` minor로 표현한다.

버전은 `Cargo.toml`의 `version` 필드에서 관리한다.

## 릴리즈 전략

기본 개발 브랜치는 `develop`이고, 일반 개발 커밋에서는 버전을 올리지 않는다.
기능, 버그 수정, 문서 정리는 `develop`으로 통합한다.

`master`는 릴리즈된 코드만 담는 보호 브랜치다. 직접 push, force push, branch 삭제는
허용하지 않는다. `master`로 들어가는 변경은 pull request를 거쳐야 하며, 최신 base 기준의
필수 체크 `Rust`, `Security audit`, `cargo-deny`가 통과해야 한다. PR conversation은
merge 전에 모두 resolve되어야 하고, linear history를 유지한다. Review-agent inline comment는
답글을 남긴 직후 바로 resolve하지 않고, follow-up 응답을 확인한 뒤 해결 또는 비조치 동의가
확인될 때 resolve한다. Thread-specific addressed marker는 그 follow-up 확인으로 볼 수 있지만,
PR body나 review-request comment의 tool-specific reaction/marker는 review 상태 신호로
확인하되 thread/comment/check 확인을 대체하지 않는다. 예:

- CodeRabbit inline comment는 후속 응답 확인 전 resolve하지 않는다.
- Codex reaction은 기록할 상태 신호로만 취급한다.

릴리즈할 때는 최신 `develop`에서 `release/vX.Y.Z` 브랜치를 만들고, 그 릴리즈에 포함된
변경 중 가장 큰 범위에 맞춰 `Cargo.toml`의 version을 한 번 올린다. release 브랜치에서
검증을 통과시킨 뒤 `master` 대상으로 릴리즈 PR을 만들고, merge 후 `vX.Y.Z` tag와 GitHub
release를 생성한다.

릴리즈 PR이 `master`에 merge되고 tag가 생성되면, version bump commit을 다시 `develop`에
merge해서 다음 릴리즈 기준점이 두 브랜치에서 어긋나지 않게 한다. CI job 이름이나 보호
규칙을 바꾸면 이 문서와 GitHub branch protection rule을 함께 갱신한다.
