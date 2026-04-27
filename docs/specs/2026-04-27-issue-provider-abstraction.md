# wt issue: Issue Provider 추상화

## 배경

`wt issue`가 Linear 전용으로 구현되어 있어 GitHub Issues 기반 프로젝트에서 사용할 수 없다. `.wt.toml` 설정으로 프로젝트별 이슈 트래커를 선택할 수 있게 한다.

## 설계 결정

- **접근 방식**: trait `IssueProvider`로 추상화. Config 설정으로 Linear/GH 분기
- **대안 검토**: if/else 분기(비대해짐), 커맨드 분리(공통 로직 추출 번거로움) 배제
- **근거**: 기존 서비스 패턴(HerdService, CmuxService)과 일관. issue.rs의 핵심 로직(worktree 생성, 충돌 처리)은 provider 무관

## trait 설계

```rust
pub struct IssueInfo {
    pub identifier: String,          // "TECH-680" 또는 "#42"
    pub title: String,
    pub branch_name: Option<String>, // 이미 존재하는 브랜치
}

pub struct IssueListItem {
    pub identifier: String,
    pub title: String,
    pub display: String,             // fuzzy select 표시 문자열
}

pub trait IssueProvider {
    /// 특정 이슈 조회
    fn get_issue(&self, id: &str) -> Result<IssueInfo>;

    /// 작업 가능한 이슈 목록
    fn list_issues(&self) -> Result<Vec<IssueListItem>>;

    /// 이슈의 브랜치를 확보 (없으면 생성)
    fn ensure_branch(&self, id: &str, base: Option<&str>) -> Result<String>;

    /// worktree 생성 후 호출 (상태 변경 등)
    fn on_start(&self, id: &str) -> Result<()>;

    /// worktree 정리 시 호출
    fn on_clean(&self, id: &str, branch: &str) -> Result<()>;
}
```

## Config 변경

`.wt.toml`에 `[issues]` 섹션 추가:

```toml
# Linear 프로젝트
[issues]
provider = "linear"

# GitHub Issues 프로젝트
[issues]
provider = "github"
gh_user = "hoetaek"    # gh issue list -a 에 사용 (optional)
```

`config.rs`:

```rust
#[derive(Debug, Deserialize, PartialEq)]
pub struct IssuesConfig {
    pub provider: IssueProviderType,
    pub gh_user: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum IssueProviderType {
    Linear,
    Github,
}
```

- `Config`에 `pub issues: Option<IssuesConfig>` 추가
- `issues` 섹션 없이 `wt issue` 실행 시 에러 메시지 안내

## Provider 구현

### GithubIssueProvider

| 메서드 | 구현 |
|--------|------|
| `get_issue` | `gh issue view {id} --json number,title` |
| `list_issues` | `gh issue list [-a {gh_user}] --json number,title --state open` |
| `ensure_branch` | `gh issue develop --list {id}` → 있으면 리턴, 없으면 `gh issue develop {id} [--base ...]` 실행 후 브랜치명 리턴 |
| `on_start` | no-op (GH는 develop로 브랜치 연결 시 자동 처리) |
| `on_clean` | no-op (브랜치 삭제 시 GH가 자동 unlink) |

### LinearIssueProvider

기존 `services/linear.rs`의 `LinearService`를 래핑:

| 메서드 | 구현 |
|--------|------|
| `get_issue` | `LinearService::get_issue()` 호출. id "680" → "TECH-680" 변환 |
| `list_issues` | `LinearService::list_issues()` 호출 |
| `ensure_branch` | `get_issue()` → `branch_name.ok_or(NoBranchName)`. base 무시 |
| `on_start` | `LinearService::update_status(id, "In Progress")` |
| `on_clean` | no-op |

## 파일 구조

```
services/
  issues/
    mod.rs          # trait 정의 + IssueInfo/IssueListItem
    github.rs       # GithubIssueProvider
    linear.rs       # LinearIssueProvider
  git.rs            # 기존 유지
  github.rs         # 기존 PR 전용 유지
  herd.rs           # 기존 유지
  linear.rs         # 기존 유지 (LinearIssueProvider가 내부에서 사용)
  cmux.rs           # 기존 유지
```

## commands/issue.rs 변경

- `LinearService` 직접 호출 → `build_provider(ctx)` → `Box<dyn IssueProvider>`
- `create_worktree()` 로직 유지. `ensure_branch`가 브랜치명 확보, `create_worktree`가 local/remote/new 분기 처리
- `linear.update_status()` → `provider.on_start()`

```rust
fn build_provider(ctx: &Ctx) -> Result<Box<dyn IssueProvider + '_>> {
    let config = ctx.config.issues.as_ref()
        .ok_or_else(|| anyhow!("No [issues] section in .wt.toml"))?;
    match config.provider {
        IssueProviderType::Linear => Ok(Box::new(LinearIssueProvider::new(ctx))),
        IssueProviderType::Github => Ok(Box::new(GithubIssueProvider::new(ctx))),
    }
}
```

## ensure_branch와 create_worktree 상호작용

`--base` 플래그가 두 곳에 전달된다:

1. `ensure_branch(id, base)` — GH에서 `gh issue develop --base`에 사용
2. `create_worktree(branch, base)` — local/remote 없을 때 git worktree의 base로 사용

GH: `ensure_branch`가 remote에 브랜치를 생성하므로 → `create_worktree`에서 remote 경로를 탐.
Linear: `ensure_branch`가 base를 무시하므로 → `create_worktree`까지 base가 내려감.
자연스럽게 맞아떨어진다.

## commands/clean.rs 변경

기존 herd unlink 뒤에 `on_clean` 훅 추가:

```rust
if let Ok(provider) = build_provider(ctx) {
    provider.on_clean(&identifier, &branch).ok();
}
```

provider 없어도(설정 없음) 에러 무시하고 진행.

## 테스트

- **trait 구현 테스트**: MockRunner로 `gh`/`linear` CLI 응답 모킹. 기존 패턴과 동일
  - GH: get_issue JSON 파싱, ensure_branch의 --list 있는/없는 경우, base 전달
  - Linear: 기존 테스트 래핑 검증
- **config 테스트**: `[issues]` 파싱, provider enum 변환, 섹션 없을 때 동작
- **commands/issue.rs**: MockIssueProvider로 issue.rs 로직 독립 테스트
