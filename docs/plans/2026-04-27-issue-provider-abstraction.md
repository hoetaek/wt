# Issue Provider 추상화 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `wt issue`가 `.wt.toml` 설정에 따라 Linear 또는 GitHub Issues를 이슈 트래커로 사용할 수 있게 한다.

**Architecture:** trait `IssueProvider`로 이슈 트래커를 추상화하고, `GithubIssueProvider`와 `LinearIssueProvider`가 구현한다. `commands/issue.rs`는 trait만 의존하며, `.wt.toml`의 `[issues] provider` 설정으로 런타임에 구현체를 선택한다.

**Tech Stack:** Rust, clap, serde, `gh` CLI, `linear` CLI

**Spec:** `docs/specs/2026-04-27-issue-provider-abstraction.md`

---

### Task 1: Config에 `[issues]` 섹션 추가

**Files:**
- Modify: `src/config.rs:1-13` (Config struct에 issues 필드 추가)
- Test: `src/config.rs` (기존 테스트 모듈 내)

- [ ] **Step 1: 파싱 테스트 작성**

`src/config.rs`의 `mod tests` 블록 끝에 추가:

```rust
#[test]
fn parses_issues_config_github() {
    let toml_str = r#"
[issues]
provider = "github"
gh_user = "hoetaek"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let issues = config.issues.unwrap();
    assert_eq!(issues.provider, IssueProviderType::Github);
    assert_eq!(issues.gh_user.as_deref(), Some("hoetaek"));
}

#[test]
fn parses_issues_config_linear() {
    let toml_str = r#"
[issues]
provider = "linear"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let issues = config.issues.unwrap();
    assert_eq!(issues.provider, IssueProviderType::Linear);
    assert!(issues.gh_user.is_none());
}

#[test]
fn issues_section_optional() {
    let toml_str = r#"
[worktree]
copy = [".env"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.issues.is_none());
}
```

- [ ] **Step 2: 테스트 실행 → 실패 확인**

Run: `cd ~/dotfiles/tools/wt && cargo test parses_issues_config -- --nocapture 2>&1 | tail -20`
Expected: 컴파일 에러 (`IssueProviderType` 미정의)

- [ ] **Step 3: IssuesConfig 구조체와 enum 추가**

`src/config.rs` 상단, `TestConfig` 구조체 뒤에 추가:

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

`Config` 구조체에 필드 추가:

```rust
pub struct Config {
    pub worktree: WorktreeConfig,
    pub setup: SetupConfig,
    pub herd: Option<HerdConfig>,
    pub workspace: Option<WorkspaceConfig>,
    pub test: Option<TestConfig>,
    pub issues: Option<IssuesConfig>,  // 추가
}
```

- [ ] **Step 4: 테스트 실행 → 통과 확인**

Run: `cd ~/dotfiles/tools/wt && cargo test parses_issues_config -- --nocapture`
Expected: 3개 테스트 모두 PASS

- [ ] **Step 5: 기존 테스트 전체 통과 확인**

Run: `cd ~/dotfiles/tools/wt && cargo test 2>&1 | tail -5`
Expected: 전체 PASS (issues는 optional이므로 기존 테스트 영향 없음)

- [ ] **Step 6: 커밋**

```bash
cd ~/dotfiles/tools/wt
git add src/config.rs
git commit -m "feat: config에 [issues] 섹션 추가 (provider = github | linear)"
```

---

### Task 2: trait `IssueProvider` 정의

**Files:**
- Create: `src/services/issues/mod.rs`
- Modify: `src/services/mod.rs:1-5` (module 선언 추가)

- [ ] **Step 1: `services/issues/mod.rs` 생성 — trait + 공용 타입 정의**

```rust
use anyhow::Result;

pub mod github;
pub mod linear;

pub struct IssueInfo {
    pub identifier: String,
    pub title: String,
    pub branch_name: Option<String>,
}

pub struct IssueListItem {
    pub identifier: String,
    pub title: String,
    pub display: String,
}

pub trait IssueProvider {
    fn get_issue(&self, id: &str) -> Result<IssueInfo>;
    fn list_issues(&self) -> Result<Vec<IssueListItem>>;
    fn ensure_branch(&self, id: &str, base: Option<&str>) -> Result<String>;
    fn on_start(&self, id: &str) -> Result<()>;
    fn on_clean(&self, id: &str, branch: &str) -> Result<()>;
}
```

- [ ] **Step 2: 빈 하위 모듈 파일 생성**

`src/services/issues/github.rs`:
```rust
// GithubIssueProvider — Task 4에서 구현
```

`src/services/issues/linear.rs`:
```rust
// LinearIssueProvider — Task 3에서 구현
```

- [ ] **Step 3: `services/mod.rs`에 모듈 등록**

`src/services/mod.rs`를 다음과 같이 변경:

```rust
pub mod cmux;
pub mod git;
pub mod github;
pub mod herd;
pub mod issues;
pub mod linear;
```

- [ ] **Step 4: 컴파일 확인**

Run: `cd ~/dotfiles/tools/wt && cargo check 2>&1 | tail -10`
Expected: 경고만 있고 에러 없음

- [ ] **Step 5: 커밋**

```bash
cd ~/dotfiles/tools/wt
git add src/services/issues/ src/services/mod.rs
git commit -m "feat: IssueProvider trait 및 공용 타입 정의"
```

---

### Task 3: `LinearIssueProvider` 구현

**Files:**
- Modify: `src/services/issues/linear.rs`
- Test: `src/services/issues/linear.rs` (파일 내 `mod tests`)

- [ ] **Step 1: 테스트 작성**

`src/services/issues/linear.rs`:

```rust
use anyhow::Result;
use crate::context::CommandRunner;
use crate::services::issues::{IssueInfo, IssueListItem, IssueProvider};
use crate::services::linear::LinearService;
use std::path::Path;

pub struct LinearIssueProvider<'a> {
    linear: LinearService<'a>,
}

impl<'a> LinearIssueProvider<'a> {
    pub fn new(runner: &'a dyn CommandRunner, cwd: Option<&'a Path>) -> Self {
        Self {
            linear: LinearService::new(runner, cwd),
        }
    }
}

impl IssueProvider for LinearIssueProvider<'_> {
    fn get_issue(&self, id: &str) -> Result<IssueInfo> {
        todo!()
    }

    fn list_issues(&self) -> Result<Vec<IssueListItem>> {
        todo!()
    }

    fn ensure_branch(&self, id: &str, _base: Option<&str>) -> Result<String> {
        todo!()
    }

    fn on_start(&self, id: &str) -> Result<()> {
        todo!()
    }

    fn on_clean(&self, _id: &str, _branch: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::MockRunner;

    #[test]
    fn get_issue_normalizes_numeric_id_to_tech_prefix() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"TECH-680","title":"위키 에디터","branchName":"hoetaek/tech-680-위키"}"#,
            true,
        );
        let provider = LinearIssueProvider::new(&runner, None);
        let issue = provider.get_issue("680").unwrap();
        assert_eq!(issue.identifier, "TECH-680");
        assert_eq!(issue.title, "위키 에디터");
        assert_eq!(issue.branch_name.as_deref(), Some("hoetaek/tech-680-위키"));
    }

    #[test]
    fn get_issue_passes_through_full_identifier() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"TECH-680","title":"위키 에디터","branchName":"hoetaek/tech-680-위키"}"#,
            true,
        );
        let provider = LinearIssueProvider::new(&runner, None);
        let issue = provider.get_issue("TECH-680").unwrap();
        assert_eq!(issue.identifier, "TECH-680");
    }

    #[test]
    fn list_issues_maps_to_display_format() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"[{"identifier":"TECH-1","title":"Issue 1","state":{"name":"Started"},"assignee":{"displayName":"hoetaek"}}]"#,
            true,
        );
        let provider = LinearIssueProvider::new(&runner, None);
        let items = provider.list_issues().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].identifier, "TECH-1");
        assert!(items[0].display.contains("TECH-1"));
        assert!(items[0].display.contains("hoetaek"));
    }

    #[test]
    fn ensure_branch_returns_branch_name() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"TECH-680","title":"위키 에디터","branchName":"hoetaek/tech-680-위키"}"#,
            true,
        );
        let provider = LinearIssueProvider::new(&runner, None);
        let branch = provider.ensure_branch("TECH-680", None).unwrap();
        assert_eq!(branch, "hoetaek/tech-680-위키");
    }

    #[test]
    fn ensure_branch_errors_when_no_branch_name() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"TECH-100","title":"Test","branchName":null}"#,
            true,
        );
        let provider = LinearIssueProvider::new(&runner, None);
        let result = provider.ensure_branch("TECH-100", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No branch name"));
    }

    #[test]
    fn on_start_updates_status() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        let provider = LinearIssueProvider::new(&runner, None);
        provider.on_start("TECH-680").unwrap();
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["issue", "update", "TECH-680", "--state", "In Progress"]);
    }

    #[test]
    fn on_clean_is_noop() {
        let runner = MockRunner::new();
        let provider = LinearIssueProvider::new(&runner, None);
        assert!(provider.on_clean("TECH-680", "hoetaek/tech-680").is_ok());
        assert!(runner.calls.lock().unwrap().is_empty());
    }
}
```

- [ ] **Step 2: 테스트 실행 → 실패 확인 (todo! 패닉)**

Run: `cd ~/dotfiles/tools/wt && cargo test services::issues::linear -- --nocapture 2>&1 | tail -20`
Expected: 컴파일은 성공하지만 테스트에서 `todo!()` 패닉

- [ ] **Step 3: 구현 채우기**

`todo!()`들을 다음으로 교체:

```rust
impl IssueProvider for LinearIssueProvider<'_> {
    fn get_issue(&self, id: &str) -> Result<IssueInfo> {
        let identifier = if id.chars().all(|c| c.is_ascii_digit()) {
            format!("TECH-{id}")
        } else {
            id.to_string()
        };
        let issue = self.linear.get_issue(&identifier)?;
        Ok(IssueInfo {
            identifier: issue.identifier,
            title: issue.title,
            branch_name: issue.branch_name,
        })
    }

    fn list_issues(&self) -> Result<Vec<IssueListItem>> {
        let issues = self.linear.list_issues()?;
        Ok(issues
            .into_iter()
            .map(|i| {
                let assignee = i
                    .assignee
                    .as_ref()
                    .map(|a| a.display_name.as_str())
                    .unwrap_or("-");
                IssueListItem {
                    display: format!("{} {} [{}]", i.identifier, i.title, assignee),
                    identifier: i.identifier,
                    title: i.title,
                }
            })
            .collect())
    }

    fn ensure_branch(&self, id: &str, _base: Option<&str>) -> Result<String> {
        let issue = self.linear.get_issue(id)?;
        issue.branch_name.ok_or_else(|| {
            crate::error::WtError::NoBranchName {
                identifier: id.to_string(),
            }
            .into()
        })
    }

    fn on_start(&self, id: &str) -> Result<()> {
        self.linear.update_status(id, "In Progress")
    }

    fn on_clean(&self, _id: &str, _branch: &str) -> Result<()> {
        Ok(())
    }
}
```

- [ ] **Step 4: 테스트 실행 → 통과 확인**

Run: `cd ~/dotfiles/tools/wt && cargo test services::issues::linear -- --nocapture`
Expected: 전체 PASS

- [ ] **Step 5: 커밋**

```bash
cd ~/dotfiles/tools/wt
git add src/services/issues/linear.rs
git commit -m "feat: LinearIssueProvider 구현 (기존 LinearService 래핑)"
```

---

### Task 4: `GithubIssueProvider` 구현

**Files:**
- Modify: `src/services/issues/github.rs`
- Test: `src/services/issues/github.rs` (파일 내 `mod tests`)

- [ ] **Step 1: 테스트 작성**

`src/services/issues/github.rs`:

```rust
use anyhow::{Result, bail};
use crate::context::CommandRunner;
use crate::services::issues::{IssueInfo, IssueListItem, IssueProvider};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct GhIssue {
    number: u32,
    title: String,
}

pub struct GithubIssueProvider<'a> {
    runner: &'a dyn CommandRunner,
    cwd: Option<&'a Path>,
    gh_user: Option<String>,
}

impl<'a> GithubIssueProvider<'a> {
    pub fn new(runner: &'a dyn CommandRunner, cwd: Option<&'a Path>, gh_user: Option<String>) -> Self {
        Self { runner, cwd, gh_user }
    }
}

impl IssueProvider for GithubIssueProvider<'_> {
    fn get_issue(&self, id: &str) -> Result<IssueInfo> {
        todo!()
    }

    fn list_issues(&self) -> Result<Vec<IssueListItem>> {
        todo!()
    }

    fn ensure_branch(&self, id: &str, base: Option<&str>) -> Result<String> {
        todo!()
    }

    fn on_start(&self, _id: &str) -> Result<()> {
        Ok(())
    }

    fn on_clean(&self, _id: &str, _branch: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::MockRunner;

    #[test]
    fn get_issue_parses_gh_json() {
        let mut runner = MockRunner::new();
        // gh issue view
        runner.add_response(r#"{"number":42,"title":"Add feature"}"#, true);
        // gh issue develop --list (기존 브랜치 있음)
        runner.add_response("42\thoetaek/42-add-feature\n", true);

        let provider = GithubIssueProvider::new(&runner, None, None);
        let issue = provider.get_issue("42").unwrap();
        assert_eq!(issue.identifier, "#42");
        assert_eq!(issue.title, "Add feature");
        assert_eq!(issue.branch_name.as_deref(), Some("hoetaek/42-add-feature"));
    }

    #[test]
    fn get_issue_no_existing_branch() {
        let mut runner = MockRunner::new();
        runner.add_response(r#"{"number":42,"title":"Add feature"}"#, true);
        // gh issue develop --list (비어있음)
        runner.add_response("", true);

        let provider = GithubIssueProvider::new(&runner, None, None);
        let issue = provider.get_issue("42").unwrap();
        assert_eq!(issue.identifier, "#42");
        assert!(issue.branch_name.is_none());
    }

    #[test]
    fn list_issues_with_gh_user_filter() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"[{"number":1,"title":"Issue 1"},{"number":2,"title":"Issue 2"}]"#,
            true,
        );

        let provider = GithubIssueProvider::new(&runner, None, Some("hoetaek".into()));
        let items = provider.list_issues().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].display, "#1 Issue 1");

        let calls = runner.calls.lock().unwrap();
        assert!(calls[0].1.contains(&"-a".to_string()));
        assert!(calls[0].1.contains(&"hoetaek".to_string()));
    }

    #[test]
    fn list_issues_without_gh_user() {
        let mut runner = MockRunner::new();
        runner.add_response(r#"[{"number":1,"title":"Issue 1"}]"#, true);

        let provider = GithubIssueProvider::new(&runner, None, None);
        let items = provider.list_issues().unwrap();
        assert_eq!(items.len(), 1);

        let calls = runner.calls.lock().unwrap();
        assert!(!calls[0].1.contains(&"-a".to_string()));
    }

    #[test]
    fn ensure_branch_returns_existing() {
        let mut runner = MockRunner::new();
        // gh issue develop --list → 기존 브랜치
        runner.add_response("42\thoetaek/42-add-feature\n", true);

        let provider = GithubIssueProvider::new(&runner, None, None);
        let branch = provider.ensure_branch("42", None).unwrap();
        assert_eq!(branch, "hoetaek/42-add-feature");
    }

    #[test]
    fn ensure_branch_creates_new_without_base() {
        let mut runner = MockRunner::new();
        // gh issue develop --list → 비어있음
        runner.add_response("", true);
        // gh issue develop 42 → 브랜치 생성 출력
        runner.add_response("hoetaek/42-add-feature", true);

        let provider = GithubIssueProvider::new(&runner, None, None);
        let branch = provider.ensure_branch("42", None).unwrap();
        assert_eq!(branch, "hoetaek/42-add-feature");

        let calls = runner.calls.lock().unwrap();
        let create_call = &calls[1];
        assert_eq!(create_call.1, vec!["issue", "develop", "42"]);
    }

    #[test]
    fn ensure_branch_creates_new_with_base() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        runner.add_response("hoetaek/42-add-feature", true);

        let provider = GithubIssueProvider::new(&runner, None, None);
        let branch = provider.ensure_branch("42", Some("develop")).unwrap();
        assert_eq!(branch, "hoetaek/42-add-feature");

        let calls = runner.calls.lock().unwrap();
        let create_call = &calls[1];
        assert_eq!(create_call.1, vec!["issue", "develop", "--base", "develop", "42"]);
    }

    #[test]
    fn on_start_is_noop() {
        let runner = MockRunner::new();
        let provider = GithubIssueProvider::new(&runner, None, None);
        assert!(provider.on_start("42").is_ok());
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn on_clean_is_noop() {
        let runner = MockRunner::new();
        let provider = GithubIssueProvider::new(&runner, None, None);
        assert!(provider.on_clean("42", "hoetaek/42-feature").is_ok());
        assert!(runner.calls.lock().unwrap().is_empty());
    }
}
```

- [ ] **Step 2: 테스트 실행 → 실패 확인**

Run: `cd ~/dotfiles/tools/wt && cargo test services::issues::github -- --nocapture 2>&1 | tail -20`
Expected: `todo!()` 패닉

- [ ] **Step 3: 구현 채우기**

`todo!()`들을 다음으로 교체:

```rust
impl IssueProvider for GithubIssueProvider<'_> {
    fn get_issue(&self, id: &str) -> Result<IssueInfo> {
        let out = self.runner.run(
            "gh",
            &["issue", "view", id, "--json", "number,title"],
            self.cwd,
        )?;
        if !out.success {
            bail!("Failed to fetch issue #{id}");
        }
        let gh_issue: GhIssue = serde_json::from_str(&out.stdout)?;

        // 기존 브랜치 확인
        let list_out = self.runner.run(
            "gh",
            &["issue", "develop", "--list", id],
            self.cwd,
        )?;
        let branch_name = list_out
            .stdout
            .lines()
            .next()
            .and_then(|line| line.split('\t').nth(1))
            .filter(|s| !s.is_empty())
            .map(String::from);

        Ok(IssueInfo {
            identifier: format!("#{}", gh_issue.number),
            title: gh_issue.title,
            branch_name,
        })
    }

    fn list_issues(&self) -> Result<Vec<IssueListItem>> {
        let mut args = vec!["issue", "list", "--json", "number,title", "--state", "open"];
        let gh_user_str;
        if let Some(ref user) = self.gh_user {
            gh_user_str = user.clone();
            args.extend_from_slice(&["-a", &gh_user_str]);
        }
        let out = self.runner.run("gh", &args, self.cwd)?;
        if !out.success {
            bail!("Failed to fetch issue list");
        }
        let issues: Vec<GhIssue> = serde_json::from_str(&out.stdout)?;
        Ok(issues
            .into_iter()
            .map(|i| IssueListItem {
                display: format!("#{} {}", i.number, i.title),
                identifier: i.number.to_string(),
                title: i.title,
            })
            .collect())
    }

    fn ensure_branch(&self, id: &str, base: Option<&str>) -> Result<String> {
        // 1. 기존 브랜치 확인
        let list_out = self.runner.run(
            "gh",
            &["issue", "develop", "--list", id],
            self.cwd,
        )?;
        if let Some(branch) = list_out
            .stdout
            .lines()
            .next()
            .and_then(|line| line.split('\t').nth(1))
            .filter(|s| !s.is_empty())
        {
            return Ok(branch.to_string());
        }

        // 2. 새 브랜치 생성
        let mut args = vec!["issue", "develop"];
        if let Some(b) = base {
            args.extend_from_slice(&["--base", b]);
        }
        args.push(id);

        let out = self.runner.run("gh", &args, self.cwd)?;
        if !out.success {
            bail!("Failed to create branch for issue #{id}");
        }
        let branch = out.stdout.trim().to_string();
        if branch.is_empty() {
            bail!("gh issue develop returned empty branch name for #{id}");
        }
        Ok(branch)
    }

    fn on_start(&self, _id: &str) -> Result<()> {
        Ok(())
    }

    fn on_clean(&self, _id: &str, _branch: &str) -> Result<()> {
        Ok(())
    }
}
```

- [ ] **Step 4: 테스트 실행 → 통과 확인**

Run: `cd ~/dotfiles/tools/wt && cargo test services::issues::github -- --nocapture`
Expected: 전체 PASS

- [ ] **Step 5: 커밋**

```bash
cd ~/dotfiles/tools/wt
git add src/services/issues/github.rs
git commit -m "feat: GithubIssueProvider 구현 (gh issue develop 기반)"
```

---

### Task 5: `commands/issue.rs`를 trait 기반으로 리팩터

**Files:**
- Modify: `src/commands/issue.rs`
- Test: `src/commands/issue.rs` (기존 테스트 업데이트)

- [ ] **Step 1: `build_provider` 함수 추가 및 import 변경**

`src/commands/issue.rs` 상단의 import를 변경:

기존:
```rust
use crate::services::linear::LinearService;
```

변경:
```rust
use crate::config::IssueProviderType;
use crate::services::issues::{IssueProvider, IssueInfo};
use crate::services::issues::github::GithubIssueProvider;
use crate::services::issues::linear::LinearIssueProvider;
```

파일 끝(테스트 모듈 직전)에 `build_provider` 함수 추가:

```rust
pub fn build_provider<'a>(ctx: &'a Ctx) -> Result<Box<dyn IssueProvider + 'a>> {
    let issues_config = ctx
        .config
        .issues
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!(
            "No [issues] section in .wt.toml. Set provider = \"linear\" or \"github\""
        ))?;
    match issues_config.provider {
        IssueProviderType::Linear => Ok(Box::new(
            LinearIssueProvider::new(ctx.runner.as_ref(), Some(&ctx.repo_root)),
        )),
        IssueProviderType::Github => Ok(Box::new(
            GithubIssueProvider::new(
                ctx.runner.as_ref(),
                Some(&ctx.repo_root),
                issues_config.gh_user.clone(),
            ),
        )),
    }
}
```

- [ ] **Step 2: `run` 함수를 trait 기반으로 변경**

`run` 함수 본문을 변경. 핵심 차이:

1. `LinearService::new()` → `build_provider(ctx)?`
2. 이슈 resolve 시 `provider.get_issue()` + `provider.ensure_branch()` 사용
3. `linear.update_status()` → `provider.on_start()`

```rust
pub fn run(ctx: &Ctx, number: Option<u32>, base_raw: &Option<String>) -> Result<()> {
    let provider = build_provider(ctx)?;
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));

    // 1. Resolve issue
    let (identifier, title) = if let Some(num) = number {
        let issue = provider.get_issue(&num.to_string())?;
        (issue.identifier, issue.title)
    } else {
        let issues = provider.list_issues()?;
        if issues.is_empty() {
            bail!("No issues found");
        }

        let items: Vec<String> = issues.iter().map(|i| i.display.clone()).collect();
        let idx = ctx.ui.select("Select an issue", &items)?;
        let selected = &issues[idx];
        (selected.identifier.clone(), selected.title.clone())
    };

    ctx.ui.print_step(&format!("{identifier}: {title}"));

    // Resolve base for ensure_branch
    let base_mode = BaseMode::from_raw(base_raw);
    let base_for_ensure = match &base_mode {
        BaseMode::Explicit(b) => Some(b.as_str()),
        _ => None,
    };

    // Ensure branch exists (provider-specific: Linear reads, GH may create)
    let raw_id = identifier.trim_start_matches('#');
    let branch_name = provider.ensure_branch(raw_id, base_for_ensure)?;

    let names = WorktreeNames::new(
        &branch_name,
        &ctx.parent_dir,
        &ctx.repo_name,
        Some(&title),
        ctx.config.herd.as_ref().map(|h| h.site_name.as_str()),
    );

    // 2. Check if branch is already checked out elsewhere
    let existing_path = git.checked_out_path(&branch_name)?;
    if let Some(ref existing) = existing_path {
        if *existing == ctx.invocation_root {
            ctx.ui
                .print_warning("이미 이 브랜치에 있습니다. 다른 브랜치로 전환 후 다시 시도하세요.");
            return Ok(());
        }
        if *existing != names.path {
            ctx.ui.print_step(&format!(
                "Branch already checked out at: {}",
                existing.display()
            ));
            setup::run_setup(ctx, existing, &names, Some(&title), "issue", None)?;
            return Ok(());
        }
    }

    // 3. Handle existing worktree directory
    if names.path.exists() {
        ctx.ui.print_warning(&format!(
            "Worktree {} already exists.",
            names.path.display()
        ));
        let items = vec![
            "Delete and recreate".into(),
            "Open existing".into(),
            "Abort".into(),
        ];
        let choice = ctx.ui.select("Worktree already exists", &items)?;
        match choice {
            0 => {
                ctx.ui.print_step("Removing existing worktree...");
                git.worktree_remove_force(&names.path).ok();
                if names.path.exists() {
                    std::fs::remove_dir_all(&names.path)?;
                }
            }
            1 => {
                setup::run_setup(ctx, &names.path, &names, Some(&title), "issue", None)?;
                return Ok(());
            }
            _ => return Err(WtError::Cancelled.into()),
        }
    }

    // 4. Create worktree
    git.fetch()?;
    let create_type = create_worktree(ctx, &git, &branch_name, &names.path, base_raw)?;

    // 5. Update issue status for new branches
    if create_type == CreateType::New {
        if let Err(e) = provider.on_start(raw_id) {
            ctx.ui
                .print_warning(&format!("Failed to update issue status: {e}"));
        }
    }

    // 6. Setup
    setup::run_setup(ctx, &names.path, &names, Some(&title), "issue", None)?;

    Ok(())
}
```

`create_worktree` 함수는 기존과 동일하게 유지 (변경 없음).

- [ ] **Step 3: 기존 테스트를 config.issues 설정 포함하도록 업데이트**

테스트에서 `Ctx` 생성 시 Config에 issues 섹션 추가가 필요하다. 테스트 헬퍼 수정:

```rust
fn make_ctx_with_linear(runner: MockRunner, ui: MockUi) -> Ctx {
    let mut config = Config::default();
    config.issues = Some(crate::config::IssuesConfig {
        provider: crate::config::IssueProviderType::Linear,
        gh_user: None,
    });
    Ctx::new(
        PathBuf::from("/tmp/test-repo"),
        PathBuf::from("/tmp/test-repo"),
        config,
        Box::new(runner),
        Box::new(ui),
    )
}
```

기존 테스트에서 `Config::default()` 사용하는 부분을 `config.issues = Some(...)` 추가하도록 수정. MockRunner의 응답 순서도 `ensure_branch`가 `get_issue`를 한 번 더 호출하므로 그에 맞게 조정.

- [ ] **Step 4: 전체 테스트 실행 → 통과 확인**

Run: `cd ~/dotfiles/tools/wt && cargo test 2>&1 | tail -10`
Expected: 전체 PASS

- [ ] **Step 5: 커밋**

```bash
cd ~/dotfiles/tools/wt
git add src/commands/issue.rs
git commit -m "refactor: issue.rs를 IssueProvider trait 기반으로 전환"
```

---

### Task 6: `commands/clean.rs`에 `on_clean` 훅 추가

**Files:**
- Modify: `src/commands/clean.rs:1-9` (import 추가), `src/commands/clean.rs:44-68` (on_clean 호출 추가)

- [ ] **Step 1: import 추가 및 on_clean 호출 삽입**

`src/commands/clean.rs` 상단에 import 추가:

```rust
use crate::commands::issue::build_provider;
```

`clean.rs`의 worktree 제거 루프 내부, branch 삭제 전에 `on_clean` 호출 추가:

```rust
// 기존 Herd unlink 뒤, worktree remove 앞에 추가:
if let Ok(provider) = build_provider(ctx) {
    if let Err(e) = provider.on_clean(&entry.branch, &entry.branch) {
        ctx.ui.print_warning(&format!("  Issue cleanup: {e}"));
    }
}
```

- [ ] **Step 2: 컴파일 및 기존 테스트 확인**

Run: `cd ~/dotfiles/tools/wt && cargo test commands::clean -- --nocapture 2>&1 | tail -10`
Expected: 기존 clean 테스트 PASS (config.issues가 None이면 build_provider가 Err → .ok()로 무시)

- [ ] **Step 3: 커밋**

```bash
cd ~/dotfiles/tools/wt
git add src/commands/clean.rs
git commit -m "feat: clean에 IssueProvider.on_clean 훅 추가"
```

---

### Task 7: 전체 통합 확인 및 문서 업데이트

**Files:**
- Modify: `CLAUDE.md` (있다면 .wt.toml 설정 예시 추가)

- [ ] **Step 1: 전체 테스트 실행**

Run: `cd ~/dotfiles/tools/wt && cargo test 2>&1 | tail -10`
Expected: 전체 PASS

- [ ] **Step 2: 빌드 확인**

Run: `cd ~/dotfiles/tools/wt && cargo build --release 2>&1 | tail -5`
Expected: 빌드 성공

- [ ] **Step 3: clippy 확인**

Run: `cd ~/dotfiles/tools/wt && cargo clippy 2>&1 | tail -10`
Expected: 에러 없음

- [ ] **Step 4: 커밋**

```bash
cd ~/dotfiles/tools/wt
git add -A
git commit -m "chore: 통합 테스트 통과 확인 및 정리"
```
