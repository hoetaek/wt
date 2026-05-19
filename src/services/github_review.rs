use crate::context::CommandRunner;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

const PR_LIST_FIELDS: &str = "number,state,updatedAt";
const PR_VIEW_FIELDS: &str = "number,title,url,state,isDraft,headRefName,headRefOid,baseRefName,mergeable,mergeStateStatus,latestReviews,reviewDecision,reviewRequests,reactionGroups,comments,statusCheckRollup";
const REVIEW_THREAD_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      reviewThreads(first: 100) {
        totalCount
        nodes {
          id
          isResolved
          isOutdated
          comments(first: 50) {
            totalCount
            nodes {
              author {
                login
              }
              body
              url
              createdAt
            }
          }
        }
      }
    }
  }
}
"#;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PullRequestReviewEvidence {
    pub pr: Option<PullRequestReviewPr>,
    pub verdict: PullRequestReviewVerdict,
    pub checks: Vec<PullRequestReviewCheck>,
    pub reviews: Vec<PullRequestSubmittedReview>,
    pub threads: Vec<PullRequestReviewThread>,
    pub reactions: Vec<PullRequestReaction>,
    pub comments: Vec<PullRequestCommentSignal>,
    pub review_requests: Vec<PullRequestReviewRequest>,
    pub suggested_triggers: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PullRequestReviewPr {
    pub number: u32,
    pub title: String,
    pub url: Option<String>,
    pub state: String,
    pub is_draft: bool,
    pub head_ref_name: String,
    pub head_ref_oid: String,
    pub base_ref_name: String,
    pub mergeable: Option<String>,
    pub merge_state_status: Option<String>,
    pub review_decision: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestReviewVerdict {
    Passed,
    Blocked,
    Pending,
    Warning,
    Unavailable,
}

impl PullRequestReviewVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Blocked => "blocked",
            Self::Pending => "pending",
            Self::Warning => "warning",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PullRequestReviewCheck {
    pub name: String,
    pub status: Option<String>,
    pub conclusion: Option<String>,
    pub url: Option<String>,
    pub verdict: PullRequestReviewVerdict,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PullRequestSubmittedReview {
    pub author: String,
    pub state: String,
    pub commit_id: Option<String>,
    pub submitted_at: Option<String>,
    pub url: Option<String>,
    pub covers_head: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PullRequestReviewThread {
    pub id: String,
    pub is_resolved: bool,
    pub is_outdated: bool,
    pub comments: Vec<PullRequestThreadComment>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PullRequestThreadComment {
    pub author: String,
    pub body: String,
    pub url: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PullRequestReaction {
    pub content: String,
    pub users: Vec<String>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PullRequestCommentSignal {
    pub author: String,
    pub body: String,
    pub url: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PullRequestReviewRequest {
    pub reviewer: String,
}

pub struct GithubReviewService<'a> {
    runner: &'a dyn CommandRunner,
    cwd: Option<&'a Path>,
}

impl<'a> GithubReviewService<'a> {
    pub fn new(runner: &'a dyn CommandRunner, cwd: Option<&'a Path>) -> Self {
        Self { runner, cwd }
    }

    pub fn review_for_branch(&self, branch: &str) -> Result<PullRequestReviewEvidence> {
        let candidates = self.find_prs_for_branch(branch)?;
        let Some(selected) = candidates
            .iter()
            .find(|candidate| candidate.state.eq_ignore_ascii_case("OPEN"))
            .or_else(|| candidates.first())
        else {
            return Ok(no_pull_request_evidence(branch));
        };

        let mut evidence = self.review_for_number(selected.number)?;
        if candidates.len() > 1 {
            evidence.warnings.push(format!(
                "Multiple pull requests matched branch {branch}; inspected PR #{}",
                selected.number
            ));
            classify_pull_request_review(&mut evidence);
        }
        Ok(evidence)
    }

    fn find_prs_for_branch(&self, branch: &str) -> Result<Vec<PullRequestListItem>> {
        let out = self.runner.run(
            "gh",
            &[
                "pr",
                "list",
                "--state",
                "all",
                "--head",
                branch,
                "--limit",
                "10",
                "--json",
                PR_LIST_FIELDS,
            ],
            self.cwd,
        )?;
        if !out.success {
            bail!(
                "Failed to find pull request for branch {branch}: {}",
                command_detail(&out)
            );
        }
        serde_json::from_str(&out.stdout)
            .with_context(|| format!("Failed to parse pull request lookup for branch {branch}"))
    }

    fn review_for_number(&self, number: u32) -> Result<PullRequestReviewEvidence> {
        let view = self.fetch_pr_view(number)?;
        let pr = pr_from_view(&view)
            .with_context(|| format!("Failed to normalize pull request #{number}"))?;
        let mut warnings = Vec::new();
        let checks = checks_from_view(&view);
        let comments = comments_from_view(&view);
        let reactions = reactions_from_view(&view);
        let review_requests = review_requests_from_view(&view);

        let repo = match self.fetch_repo_slug() {
            Ok(repo) => Some(repo),
            Err(err) => {
                warnings.push(format!(
                    "Could not fetch repository identity for PR evidence: {err:#}"
                ));
                None
            }
        };

        let mut reviews = latest_reviews_from_view(&view, &pr.head_ref_oid);
        let mut threads = Vec::new();
        if let Some(repo) = repo.as_ref() {
            match self.fetch_submitted_reviews(repo, number, &pr.head_ref_oid) {
                Ok((rest_reviews, rest_warnings)) => {
                    if !rest_reviews.is_empty() {
                        reviews = rest_reviews;
                    }
                    warnings.extend(rest_warnings);
                }
                Err(err) => warnings.push(format!(
                    "Could not fetch submitted pull request reviews: {err:#}"
                )),
            }

            match self.fetch_review_threads(repo, number) {
                Ok((fetched_threads, thread_warnings)) => {
                    threads = fetched_threads;
                    warnings.extend(thread_warnings);
                }
                Err(err) => warnings.push(format!(
                    "Could not fetch pull request review threads: {err:#}"
                )),
            }
        }

        let mut evidence = PullRequestReviewEvidence {
            pr: Some(pr),
            verdict: PullRequestReviewVerdict::Unavailable,
            checks,
            reviews,
            threads,
            reactions,
            comments,
            review_requests,
            suggested_triggers: Vec::new(),
            warnings,
        };
        classify_pull_request_review(&mut evidence);
        Ok(evidence)
    }

    fn fetch_pr_view(&self, number: u32) -> Result<Value> {
        let out = self.runner.run(
            "gh",
            &["pr", "view", &number.to_string(), "--json", PR_VIEW_FIELDS],
            self.cwd,
        )?;
        if !out.success {
            bail!("Failed to fetch PR #{number}: {}", command_detail(&out));
        }
        serde_json::from_str(&out.stdout)
            .with_context(|| format!("Failed to parse pull request #{number}"))
    }

    fn fetch_repo_slug(&self) -> Result<RepoSlug> {
        let out = self
            .runner
            .run("gh", &["repo", "view", "--json", "owner,name"], self.cwd)?;
        if !out.success {
            bail!(
                "Failed to fetch repository identity: {}",
                command_detail(&out)
            );
        }
        let value: Value =
            serde_json::from_str(&out.stdout).context("Failed to parse repository identity")?;
        let owner = value
            .get("owner")
            .and_then(|owner| owner.get("login"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if owner.is_empty() || name.is_empty() {
            bail!("Repository identity did not include owner.login and name");
        }
        Ok(RepoSlug { owner, name })
    }

    fn fetch_submitted_reviews(
        &self,
        repo: &RepoSlug,
        number: u32,
        head_ref_oid: &str,
    ) -> Result<(Vec<PullRequestSubmittedReview>, Vec<String>)> {
        let endpoint = format!(
            "repos/{}/{}/pulls/{number}/reviews?per_page=100",
            repo.owner, repo.name
        );
        let out = self.runner.run("gh", &["api", &endpoint], self.cwd)?;
        if !out.success {
            bail!(
                "Failed to fetch submitted reviews: {}",
                command_detail(&out)
            );
        }
        let values: Vec<Value> =
            serde_json::from_str(&out.stdout).context("Failed to parse submitted reviews")?;
        let mut warnings = Vec::new();
        if values.len() >= 100 {
            warnings.push("Submitted review pagination may be incomplete at 100 reviews".into());
        }
        let reviews = values
            .iter()
            .map(|value| submitted_review_from_rest(value, head_ref_oid))
            .filter(|review| !review.author.is_empty())
            .collect();
        Ok((reviews, warnings))
    }

    fn fetch_review_threads(
        &self,
        repo: &RepoSlug,
        number: u32,
    ) -> Result<(Vec<PullRequestReviewThread>, Vec<String>)> {
        let owner_arg = format!("owner={}", repo.owner);
        let name_arg = format!("name={}", repo.name);
        let number_arg = format!("number={number}");
        let query_arg = format!("query={REVIEW_THREAD_QUERY}");
        let out = self.runner.run(
            "gh",
            &[
                "api",
                "graphql",
                "-F",
                &owner_arg,
                "-F",
                &name_arg,
                "-F",
                &number_arg,
                "-f",
                &query_arg,
            ],
            self.cwd,
        )?;
        if !out.success {
            bail!("Failed to fetch review threads: {}", command_detail(&out));
        }
        let value: Value =
            serde_json::from_str(&out.stdout).context("Failed to parse review thread response")?;
        Ok(threads_from_graphql(&value))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestListItem {
    number: u32,
    state: String,
}

#[derive(Debug, Clone)]
struct RepoSlug {
    owner: String,
    name: String,
}

pub fn no_pull_request_evidence(branch: &str) -> PullRequestReviewEvidence {
    PullRequestReviewEvidence {
        pr: None,
        verdict: PullRequestReviewVerdict::Unavailable,
        checks: Vec::new(),
        reviews: Vec::new(),
        threads: Vec::new(),
        reactions: Vec::new(),
        comments: Vec::new(),
        review_requests: Vec::new(),
        suggested_triggers: Vec::new(),
        warnings: vec![format!(
            "No associated pull request detected for branch {branch}"
        )],
    }
}

pub fn classify_pull_request_review(evidence: &mut PullRequestReviewEvidence) {
    evidence.suggested_triggers.clear();
    let Some(pr) = evidence.pr.as_ref() else {
        evidence.verdict = PullRequestReviewVerdict::Unavailable;
        return;
    };

    let mut blocked = false;
    let mut pending = false;
    let mut warning = !evidence.warnings.is_empty();
    let mut unavailable = evidence
        .warnings
        .iter()
        .any(|message| message.starts_with("Could not fetch"));

    if pr.is_draft {
        pending = true;
        push_warning_once(
            &mut evidence.warnings,
            "Draft pull request is pending for landing",
        );
    }

    for check in &evidence.checks {
        match check.verdict {
            PullRequestReviewVerdict::Blocked => blocked = true,
            PullRequestReviewVerdict::Pending => pending = true,
            PullRequestReviewVerdict::Warning | PullRequestReviewVerdict::Unavailable => {
                warning = true
            }
            PullRequestReviewVerdict::Passed => {}
        }
    }

    for review in &evidence.reviews {
        if review.state.eq_ignore_ascii_case("CHANGES_REQUESTED") {
            blocked = true;
        }
    }

    let has_head_review = evidence
        .reviews
        .iter()
        .any(|review| review.covers_head && is_review_agent(&review.author));
    let has_stale_review = evidence
        .reviews
        .iter()
        .any(|review| !review.covers_head && is_review_agent(&review.author));
    if !has_head_review {
        pending = true;
        if has_stale_review {
            push_warning_once(
                &mut evidence.warnings,
                "Bot review evidence exists, but no bot review commit matches the current PR head",
            );
        } else if !unavailable {
            push_warning_once(
                &mut evidence.warnings,
                "No head-synchronized bot review evidence was observed",
            );
        }
        evidence.suggested_triggers = vec!["@coderabbitai review".into(), "@codex review".into()];
    }

    for thread in &evidence.threads {
        if thread.is_resolved {
            continue;
        }
        if thread_confirms_issue_remains(thread) || thread_is_actionable(thread) {
            blocked = true;
        } else {
            warning = true;
            if thread.is_outdated {
                push_warning_once(
                    &mut evidence.warnings,
                    "Outdated unresolved review thread observed; outdated context is not fix confirmation",
                );
            } else {
                push_warning_once(
                    &mut evidence.warnings,
                    "Unresolved review thread observed without enough actionable language to block automatically",
                );
            }
        }
    }

    for comment in &evidence.comments {
        if !is_unknown_bot(&comment.author) {
            continue;
        }
        if text_is_actionable(&comment.body) || text_says_issue_remains(&comment.body) {
            blocked = true;
        } else {
            warning = true;
            push_warning_once(
                &mut evidence.warnings,
                "Unknown bot pull request comment observed as warning-only evidence",
            );
        }
    }

    unavailable = unavailable
        || evidence
            .warnings
            .iter()
            .any(|message| message.starts_with("Could not fetch"));
    warning = warning || !evidence.warnings.is_empty();
    evidence.verdict = if blocked {
        PullRequestReviewVerdict::Blocked
    } else if unavailable {
        PullRequestReviewVerdict::Unavailable
    } else if pending {
        PullRequestReviewVerdict::Pending
    } else if warning {
        PullRequestReviewVerdict::Warning
    } else {
        PullRequestReviewVerdict::Passed
    };
}

fn pr_from_view(value: &Value) -> Result<PullRequestReviewPr> {
    let number = value
        .get("number")
        .and_then(Value::as_u64)
        .context("PR view did not include number")? as u32;
    let title = string_field(value, "title");
    let state = string_field(value, "state");
    let head_ref_name = string_field(value, "headRefName");
    let head_ref_oid = string_field(value, "headRefOid");
    let base_ref_name = string_field(value, "baseRefName");
    if head_ref_oid.is_empty() {
        bail!("PR view did not include headRefOid");
    }
    Ok(PullRequestReviewPr {
        number,
        title,
        url: optional_string_field(value, "url"),
        state,
        is_draft: value
            .get("isDraft")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        head_ref_name,
        head_ref_oid,
        base_ref_name,
        mergeable: optional_string_field(value, "mergeable"),
        merge_state_status: optional_string_field(value, "mergeStateStatus"),
        review_decision: optional_string_field(value, "reviewDecision"),
    })
}

fn checks_from_view(value: &Value) -> Vec<PullRequestReviewCheck> {
    value
        .get("statusCheckRollup")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(check_from_value)
        .filter(|check| !check.name.is_empty())
        .collect()
}

fn check_from_value(value: &Value) -> PullRequestReviewCheck {
    let name = first_string(value, &["name", "context", "workflowName"]).unwrap_or_default();
    let status = first_string(value, &["status", "state"]);
    let conclusion = optional_string_field(value, "conclusion");
    let url = first_string(value, &["detailsUrl", "targetUrl", "url"]);
    let verdict = classify_check(status.as_deref(), conclusion.as_deref());
    PullRequestReviewCheck {
        name,
        status,
        conclusion,
        url,
        verdict,
    }
}

fn classify_check(status: Option<&str>, conclusion: Option<&str>) -> PullRequestReviewVerdict {
    let signal = conclusion
        .or(status)
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(
        signal.as_str(),
        "SUCCESS" | "PASSED" | "NEUTRAL" | "SKIPPED" | "COMPLETED"
    ) {
        PullRequestReviewVerdict::Passed
    } else if matches!(
        signal.as_str(),
        "FAILURE" | "FAILED" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED"
    ) {
        PullRequestReviewVerdict::Blocked
    } else if matches!(
        signal.as_str(),
        "PENDING" | "QUEUED" | "IN_PROGRESS" | "REQUESTED" | "WAITING" | "EXPECTED"
    ) {
        PullRequestReviewVerdict::Pending
    } else {
        PullRequestReviewVerdict::Warning
    }
}

fn latest_reviews_from_view(value: &Value, head_ref_oid: &str) -> Vec<PullRequestSubmittedReview> {
    value
        .get("latestReviews")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|review| submitted_review_from_latest(review, head_ref_oid))
        .filter(|review| !review.author.is_empty())
        .collect()
}

fn submitted_review_from_latest(value: &Value, head_ref_oid: &str) -> PullRequestSubmittedReview {
    let commit_id = optional_string_field(value, "commitId")
        .or_else(|| optional_string_field(value, "commit_id"));
    PullRequestSubmittedReview {
        author: login_from_actor(value),
        state: string_field(value, "state"),
        covers_head: commit_id
            .as_deref()
            .is_some_and(|commit| commit == head_ref_oid),
        commit_id,
        submitted_at: first_string(value, &["submittedAt", "submitted_at"]),
        url: optional_string_field(value, "url"),
    }
}

fn submitted_review_from_rest(value: &Value, head_ref_oid: &str) -> PullRequestSubmittedReview {
    let commit_id = optional_string_field(value, "commit_id")
        .or_else(|| optional_string_field(value, "commitId"));
    PullRequestSubmittedReview {
        author: login_from_actor(value),
        state: string_field(value, "state"),
        covers_head: commit_id
            .as_deref()
            .is_some_and(|commit| commit == head_ref_oid),
        commit_id,
        submitted_at: first_string(value, &["submitted_at", "submittedAt"]),
        url: first_string(value, &["html_url", "url"]),
    }
}

fn comments_from_view(value: &Value) -> Vec<PullRequestCommentSignal> {
    value
        .get("comments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|comment| PullRequestCommentSignal {
            author: login_from_actor(comment),
            body: string_field(comment, "body"),
            url: optional_string_field(comment, "url"),
            created_at: first_string(comment, &["createdAt", "created_at"]),
        })
        .filter(|comment| !comment.author.is_empty() || !comment.body.is_empty())
        .collect()
}

fn reactions_from_view(value: &Value) -> Vec<PullRequestReaction> {
    value
        .get("reactionGroups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(reaction_from_value)
        .filter(|reaction| !reaction.content.is_empty() && reaction.count > 0)
        .collect()
}

fn reaction_from_value(value: &Value) -> PullRequestReaction {
    let content = string_field(value, "content");
    let users_value = value.get("users").unwrap_or(&Value::Null);
    let users = users_value
        .get("nodes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(login_from_actor)
        .filter(|login| !login.is_empty())
        .collect::<Vec<_>>();
    let count = users_value
        .get("totalCount")
        .and_then(Value::as_u64)
        .map(|count| count as usize)
        .unwrap_or(users.len());
    PullRequestReaction {
        content,
        users,
        count,
    }
}

fn review_requests_from_view(value: &Value) -> Vec<PullRequestReviewRequest> {
    value
        .get("reviewRequests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|request| PullRequestReviewRequest {
            reviewer: login_from_actor(request),
        })
        .filter(|request| !request.reviewer.is_empty())
        .collect()
}

fn threads_from_graphql(value: &Value) -> (Vec<PullRequestReviewThread>, Vec<String>) {
    let review_threads = value
        .pointer("/data/repository/pullRequest/reviewThreads")
        .unwrap_or(&Value::Null);
    let total_count = review_threads
        .get("totalCount")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let nodes = review_threads
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut warnings = Vec::new();
    if total_count > nodes.len() {
        warnings.push(format!(
            "Review thread pagination incomplete: fetched {} of {total_count} threads",
            nodes.len()
        ));
    }
    let threads = nodes
        .iter()
        .map(|node| thread_from_graphql_node(node, &mut warnings))
        .collect();
    (threads, warnings)
}

fn thread_from_graphql_node(value: &Value, warnings: &mut Vec<String>) -> PullRequestReviewThread {
    let comments_value = value.get("comments").unwrap_or(&Value::Null);
    let total_comments = comments_value
        .get("totalCount")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let comment_nodes = comments_value
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let id = string_field(value, "id");
    if total_comments > comment_nodes.len() {
        warnings.push(format!(
            "Review thread {id} comment pagination incomplete: fetched {} of {total_comments} comments",
            comment_nodes.len()
        ));
    }
    PullRequestReviewThread {
        id,
        is_resolved: value
            .get("isResolved")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        is_outdated: value
            .get("isOutdated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        comments: comment_nodes
            .iter()
            .map(|comment| PullRequestThreadComment {
                author: login_from_actor(comment),
                body: string_field(comment, "body"),
                url: optional_string_field(comment, "url"),
                created_at: first_string(comment, &["createdAt", "created_at"]),
            })
            .collect(),
    }
}

fn thread_confirms_issue_remains(thread: &PullRequestReviewThread) -> bool {
    thread
        .comments
        .iter()
        .any(|comment| is_review_agent(&comment.author) && text_says_issue_remains(&comment.body))
}

fn thread_is_actionable(thread: &PullRequestReviewThread) -> bool {
    thread
        .comments
        .iter()
        .any(|comment| text_is_actionable(&comment.body))
}

fn text_says_issue_remains(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    [
        "issue remains",
        "still an issue",
        "still needs",
        "not addressed",
        "not fixed",
        "still failing",
        "still unresolved",
    ]
    .iter()
    .any(|needle| body.contains(needle))
}

fn text_is_actionable(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    [
        "must ",
        "should ",
        "please ",
        "fix ",
        "needs ",
        "change ",
        "bug",
        "broken",
        "failing",
        "error",
        "risk",
        "regression",
        "security",
        "do not merge",
    ]
    .iter()
    .any(|needle| body.contains(needle))
}

fn is_review_agent(login: &str) -> bool {
    let login = login.to_ascii_lowercase();
    login.contains("coderabbit") || login.contains("codex") || login.contains("chatgpt")
}

fn is_unknown_bot(login: &str) -> bool {
    let login = login.to_ascii_lowercase();
    (login.ends_with("[bot]") || login.contains("bot")) && !is_review_agent(&login)
}

fn login_from_actor(value: &Value) -> String {
    value
        .get("author")
        .and_then(|actor| actor.get("login"))
        .or_else(|| value.get("user").and_then(|actor| actor.get("login")))
        .or_else(|| {
            value
                .get("requestedReviewer")
                .and_then(|actor| actor.get("login"))
        })
        .or_else(|| value.get("login"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| optional_string_field(value, key))
        .filter(|value| !value.is_empty())
}

fn optional_string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn string_field(value: &Value, key: &str) -> String {
    optional_string_field(value, key).unwrap_or_default()
}

fn push_warning_once(warnings: &mut Vec<String>, warning: &str) {
    if !warnings.iter().any(|existing| existing == warning) {
        warnings.push(warning.into());
    }
}

fn command_detail(out: &crate::context::CmdOutput) -> &str {
    if out.stderr.trim().is_empty() {
        out.stdout.trim()
    } else {
        out.stderr.trim()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::MockRunner;

    fn base_evidence() -> PullRequestReviewEvidence {
        PullRequestReviewEvidence {
            pr: Some(PullRequestReviewPr {
                number: 42,
                title: "Ship feature".into(),
                url: Some("https://github.com/acme/widgets/pull/42".into()),
                state: "OPEN".into(),
                is_draft: false,
                head_ref_name: "feature".into(),
                head_ref_oid: "head".into(),
                base_ref_name: "main".into(),
                mergeable: Some("MERGEABLE".into()),
                merge_state_status: Some("CLEAN".into()),
                review_decision: None,
            }),
            verdict: PullRequestReviewVerdict::Unavailable,
            checks: vec![PullRequestReviewCheck {
                name: "CI".into(),
                status: Some("COMPLETED".into()),
                conclusion: Some("SUCCESS".into()),
                url: None,
                verdict: PullRequestReviewVerdict::Passed,
            }],
            reviews: vec![PullRequestSubmittedReview {
                author: "coderabbitai".into(),
                state: "COMMENTED".into(),
                commit_id: Some("head".into()),
                submitted_at: None,
                url: None,
                covers_head: true,
            }],
            threads: Vec::new(),
            reactions: Vec::new(),
            comments: Vec::new(),
            review_requests: Vec::new(),
            suggested_triggers: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn classify_passed_when_checks_and_head_review_are_clean() {
        let mut evidence = base_evidence();
        classify_pull_request_review(&mut evidence);
        assert_eq!(evidence.verdict, PullRequestReviewVerdict::Passed);
    }

    #[test]
    fn classify_blocked_for_failed_check() {
        let mut evidence = base_evidence();
        evidence.checks[0].conclusion = Some("FAILURE".into());
        evidence.checks[0].verdict = PullRequestReviewVerdict::Blocked;
        classify_pull_request_review(&mut evidence);
        assert_eq!(evidence.verdict, PullRequestReviewVerdict::Blocked);
    }

    #[test]
    fn classify_pending_for_draft_pr() {
        let mut evidence = base_evidence();
        evidence.pr.as_mut().unwrap().is_draft = true;
        classify_pull_request_review(&mut evidence);
        assert_eq!(evidence.verdict, PullRequestReviewVerdict::Pending);
    }

    #[test]
    fn classify_warning_for_unknown_non_actionable_bot_signal() {
        let mut evidence = base_evidence();
        evidence.comments.push(PullRequestCommentSignal {
            author: "review-helper[bot]".into(),
            body: "I scanned this pull request.".into(),
            url: None,
            created_at: None,
        });
        classify_pull_request_review(&mut evidence);
        assert_eq!(evidence.verdict, PullRequestReviewVerdict::Warning);
        assert!(evidence.warnings.iter().any(|warning| {
            warning.contains("Unknown bot pull request comment observed as warning-only evidence")
        }));
    }

    #[test]
    fn stale_bot_review_is_pending_not_passed() {
        let mut evidence = base_evidence();
        evidence.reviews[0].commit_id = Some("old".into());
        evidence.reviews[0].covers_head = false;
        classify_pull_request_review(&mut evidence);
        assert_eq!(evidence.verdict, PullRequestReviewVerdict::Pending);
        assert_eq!(
            evidence.suggested_triggers,
            vec!["@coderabbitai review", "@codex review"]
        );
    }

    #[test]
    fn outdated_thread_is_warning_unless_actionable_or_confirmed_remaining() {
        let mut evidence = base_evidence();
        evidence.threads.push(PullRequestReviewThread {
            id: "thread-1".into(),
            is_resolved: false,
            is_outdated: true,
            comments: vec![PullRequestThreadComment {
                author: "coderabbitai".into(),
                body: "Earlier observation for this line.".into(),
                url: None,
                created_at: None,
            }],
        });
        classify_pull_request_review(&mut evidence);
        assert_eq!(evidence.verdict, PullRequestReviewVerdict::Warning);

        evidence.threads[0].comments[0].body = "The issue remains and is not fixed.".into();
        classify_pull_request_review(&mut evidence);
        assert_eq!(evidence.verdict, PullRequestReviewVerdict::Blocked);
    }

    #[test]
    fn review_for_branch_returns_no_pr_evidence_for_empty_lookup() {
        let mut runner = MockRunner::new();
        runner.add_response("[]", true);
        let service = GithubReviewService::new(&runner, None);
        let evidence = service.review_for_branch("feature").unwrap();
        assert_eq!(evidence.pr, None);
        assert_eq!(evidence.verdict, PullRequestReviewVerdict::Unavailable);
    }

    #[test]
    fn review_for_branch_fails_when_pr_lookup_fails() {
        let mut runner = MockRunner::new();
        runner.add_response_with_stderr("", "auth required", false);
        let service = GithubReviewService::new(&runner, None);
        let err = service.review_for_branch("feature").unwrap_err();
        assert!(format!("{err:#}").contains("Failed to find pull request"));
    }
}
