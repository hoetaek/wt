use crate::commands::profile_match;
use crate::config::Config;
use crate::context::Ctx;
use crate::names::WorktreeNames;
use crate::services::git::{GitService, WorktreeEntry};
use crate::setup;
use anyhow::Result;
use console::{Term, measure_text_width};
use serde::Serialize;
use std::io::Write;

const MIN_BRANCH_WIDTH: usize = 10;
const MIN_PARENT_WIDTH: usize = 6;
const MIN_SITE_WIDTH: usize = 12;
const MIN_PATH_WIDTH: usize = 10;

pub fn run(ctx: &Ctx, wide: bool) -> Result<()> {
    let items = collect(ctx)?;
    if ctx.is_json() {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        serde_json::to_writer_pretty(&mut handle, &items)?;
        writeln!(handle)?;
    } else {
        print_table(&items, wide)?;
    }
    Ok(())
}

#[derive(Debug, Serialize, PartialEq)]
struct WorktreeRow {
    branch: String,
    path: String,
    current: bool,
    dirty: bool,
    parent: Option<String>,
    ahead: Option<u32>,
    behind: Option<u32>,
    site_url: Option<String>,
}

fn collect(ctx: &Ctx) -> Result<Vec<WorktreeRow>> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));
    let current = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root))
        .current_branch()
        .ok();
    let entries = git.worktree_list()?;

    entries
        .iter()
        .map(|entry| build_row(ctx, &git, entry, current.as_deref()))
        .collect()
}

fn build_row(
    ctx: &Ctx,
    git: &GitService,
    entry: &WorktreeEntry,
    current: Option<&str>,
) -> Result<WorktreeRow> {
    let parent = git.get_branch_parent(&entry.branch)?;
    let (ahead, behind) = parent
        .as_deref()
        .and_then(|parent| ahead_behind(ctx, &entry.branch, parent))
        .unwrap_or((None, None));

    Ok(WorktreeRow {
        branch: entry.branch.clone(),
        path: entry.path.display().to_string(),
        current: current == Some(entry.branch.as_str()),
        dirty: is_dirty(ctx, entry),
        parent,
        ahead,
        behind,
        site_url: site_url(ctx, entry)?,
    })
}

fn is_dirty(ctx: &Ctx, entry: &WorktreeEntry) -> bool {
    ctx.runner
        .run("git", &["status", "--porcelain"], Some(&entry.path))
        .map(|out| out.success && !out.stdout.trim().is_empty())
        .unwrap_or(false)
}

fn ahead_behind(ctx: &Ctx, branch: &str, parent: &str) -> Option<(Option<u32>, Option<u32>)> {
    let range = format!("{parent}...{branch}");
    let out = ctx
        .runner
        .run(
            "git",
            &["rev-list", "--left-right", "--count", &range],
            Some(&ctx.repo_root),
        )
        .ok()?;
    if !out.success {
        return None;
    }
    let mut parts = out.stdout.split_whitespace();
    let behind = parts.next()?.parse::<u32>().ok()?;
    let ahead = parts.next()?.parse::<u32>().ok()?;
    Some((Some(ahead), Some(behind)))
}

fn site_url(ctx: &Ctx, entry: &WorktreeEntry) -> Result<Option<String>> {
    let profile_config = profile_match::load_profile_config_for_branch(ctx, &entry.branch)?;
    let config = profile_config.as_ref().unwrap_or(&ctx.config);
    site_url_with_config(ctx, config, entry)
}

fn site_url_with_config(
    ctx: &Ctx,
    config: &Config,
    entry: &WorktreeEntry,
) -> Result<Option<String>> {
    if !config.has_site() {
        return Ok(None);
    }

    let names = WorktreeNames::new(
        &entry.branch,
        &ctx.parent_dir,
        &ctx.repo_name,
        None,
        Some(""),
    );
    let mut vars = setup::build_template_vars(ctx, &entry.path, &names, None);
    setup::apply_site_template_vars(config, &mut vars);
    Ok(vars.remove("site_url"))
}

fn print_table(items: &[WorktreeRow], wide: bool) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let max_width = if wide { None } else { Some(terminal_width()) };
    write!(out, "{}", render_table(items, max_width))?;
    Ok(())
}

fn render_table(items: &[WorktreeRow], max_width: Option<usize>) -> String {
    let has_site = items.iter().any(|item| item.site_url.is_some());
    let show_site = has_site && site_column_fits(max_width);
    let headers = table_headers(show_site);
    let mut rows = table_rows(items, show_site);
    let mut widths = column_widths(&headers, &rows);
    if let Some(max_width) = max_width {
        shrink_to_fit(&headers, &mut rows, &mut widths, show_site, max_width);
    }

    let mut out = String::new();
    out.push_str(&separator('┌', '┬', '┐', &widths));
    out.push_str(&row(&headers, &widths));
    out.push_str(&separator('├', '┼', '┤', &widths));
    for cells in rows {
        out.push_str(&row(&cells, &widths));
    }
    out.push_str(&separator('└', '┴', '┘', &widths));
    out
}

fn site_column_fits(max_width: Option<usize>) -> bool {
    match max_width {
        Some(max_width) => min_table_width(true) <= max_width,
        None => true,
    }
}

fn table_headers(show_site: bool) -> Vec<String> {
    let mut headers = vec![
        "CUR".into(),
        "BRANCH".into(),
        "STATE".into(),
        "PARENT".into(),
        "SYNC".into(),
    ];
    if show_site {
        headers.push("SITE".into());
    }
    headers.push("PATH".into());
    headers
}

fn min_table_width(show_site: bool) -> usize {
    let widths = if show_site {
        vec![
            measure_text_width("CUR"),
            MIN_BRANCH_WIDTH,
            measure_text_width("STATE"),
            MIN_PARENT_WIDTH,
            measure_text_width("SYNC").max(measure_text_width("+0/-0")),
            MIN_SITE_WIDTH,
            MIN_PATH_WIDTH,
        ]
    } else {
        vec![
            measure_text_width("CUR"),
            MIN_BRANCH_WIDTH,
            measure_text_width("STATE"),
            MIN_PARENT_WIDTH,
            measure_text_width("SYNC").max(measure_text_width("+0/-0")),
            MIN_PATH_WIDTH,
        ]
    };
    table_width(&widths)
}

fn table_rows(items: &[WorktreeRow], show_site: bool) -> Vec<Vec<String>> {
    items
        .iter()
        .map(|item| {
            let mut cells = vec![
                if item.current { "●" } else { "" }.into(),
                item.branch.clone(),
                if item.dirty { "dirty" } else { "clean" }.into(),
                item.parent.clone().unwrap_or_else(|| "-".into()),
                sync_label(item),
            ];
            if show_site {
                cells.push(item.site_url.clone().unwrap_or_else(|| "-".into()));
            }
            cells.push(item.path.clone());
            cells
        })
        .collect()
}

fn sync_label(item: &WorktreeRow) -> String {
    match (item.ahead, item.behind) {
        (Some(0), Some(0)) => "even".into(),
        (Some(ahead), Some(behind)) => format!("+{ahead}/-{behind}"),
        _ => "-".into(),
    }
}

fn column_widths(headers: &[String], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths = headers
        .iter()
        .map(|header| measure_text_width(header))
        .collect::<Vec<_>>();
    for row in rows {
        for (idx, cell) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(measure_text_width(cell));
        }
    }
    widths
}

fn shrink_to_fit(
    headers: &[String],
    rows: &mut [Vec<String>],
    widths: &mut [usize],
    show_site: bool,
    max_width: usize,
) {
    let Some(mut excess) = table_width(widths).checked_sub(max_width) else {
        return;
    };

    for spec in shrink_plan(show_site) {
        if excess == 0 {
            break;
        }
        if widths[spec.index] <= spec.min_width {
            continue;
        }

        let shrink_by = (widths[spec.index] - spec.min_width).min(excess);
        let target_width = widths[spec.index] - shrink_by;
        widths[spec.index] = target_width.max(measure_text_width(&headers[spec.index]));
        for row in rows.iter_mut() {
            row[spec.index] = match spec.mode {
                TruncateMode::End => truncate_end(&row[spec.index], widths[spec.index]),
                TruncateMode::Middle => truncate_middle(&row[spec.index], widths[spec.index]),
            };
        }
        excess = table_width(widths).saturating_sub(max_width);
    }
}

#[derive(Clone, Copy)]
struct ShrinkSpec {
    index: usize,
    min_width: usize,
    mode: TruncateMode,
}

#[derive(Clone, Copy)]
enum TruncateMode {
    End,
    Middle,
}

fn shrink_plan(show_site: bool) -> Vec<ShrinkSpec> {
    let mut plan = Vec::new();
    let path_index = if show_site { 6 } else { 5 };
    plan.push(ShrinkSpec {
        index: path_index,
        min_width: MIN_PATH_WIDTH,
        mode: TruncateMode::Middle,
    });
    plan.push(ShrinkSpec {
        index: 1,
        min_width: MIN_BRANCH_WIDTH,
        mode: TruncateMode::End,
    });
    if show_site {
        plan.push(ShrinkSpec {
            index: 5,
            min_width: MIN_SITE_WIDTH,
            mode: TruncateMode::Middle,
        });
    }
    plan.push(ShrinkSpec {
        index: 3,
        min_width: MIN_PARENT_WIDTH,
        mode: TruncateMode::End,
    });
    plan
}

fn table_width(widths: &[usize]) -> usize {
    widths.iter().sum::<usize>() + widths.len() * 3 + 1
}

fn terminal_width() -> usize {
    let (_, cols) = Term::stdout().size();
    usize::from(cols)
}

fn truncate_end(value: &str, max_width: usize) -> String {
    if measure_text_width(value) <= max_width {
        return value.to_string();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let mut out = String::new();
    let keep_width = max_width - 3;
    for ch in value.chars() {
        let next_width = measure_text_width(&out) + measure_text_width(&ch.to_string());
        if next_width > keep_width {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}

fn truncate_middle(value: &str, max_width: usize) -> String {
    if measure_text_width(value) <= max_width {
        return value.to_string();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let keep_width = max_width - 3;
    let start_width = keep_width / 2 + keep_width % 2;
    let end_width = keep_width / 2;
    let start = take_prefix_width(value, start_width);
    let end = take_suffix_width(value, end_width);
    format!("{start}...{end}")
}

fn take_prefix_width(value: &str, max_width: usize) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        let next_width = measure_text_width(&out) + measure_text_width(&ch.to_string());
        if next_width > max_width {
            break;
        }
        out.push(ch);
    }
    out
}

fn take_suffix_width(value: &str, max_width: usize) -> String {
    let mut chars = Vec::new();
    let mut width = 0;
    for ch in value.chars().rev() {
        let ch_width = measure_text_width(&ch.to_string());
        if width + ch_width > max_width {
            break;
        }
        chars.push(ch);
        width += ch_width;
    }
    chars.into_iter().rev().collect()
}

fn separator(left: char, middle: char, right: char, widths: &[usize]) -> String {
    let mut line = String::new();
    line.push(left);
    for (idx, width) in widths.iter().enumerate() {
        if idx > 0 {
            line.push(middle);
        }
        line.push_str(&"─".repeat(width + 2));
    }
    line.push(right);
    line.push('\n');
    line
}

fn row(cells: &[String], widths: &[usize]) -> String {
    let mut line = String::new();
    line.push('│');
    for (cell, width) in cells.iter().zip(widths) {
        line.push(' ');
        line.push_str(cell);
        line.push_str(&" ".repeat(width.saturating_sub(measure_text_width(cell))));
        line.push(' ');
        line.push('│');
    }
    line.push('\n');
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CmdOutput, CommandRunner, Ctx};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    struct SharedRunner {
        inner: Arc<MockRunner>,
    }

    impl CommandRunner for SharedRunner {
        fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
            self.inner.run(cmd, args, cwd)
        }

        fn has_command(&self, cmd: &str) -> bool {
            self.inner.has_command(cmd)
        }
    }

    #[test]
    fn collect_reports_dirty_and_ahead_behind_state() {
        let mut runner = MockRunner::new();
        runner.add_response("feature", true); // current branch
        runner.add_response(
            "worktree /tmp/repo\nHEAD abc\nbranch refs/heads/main\n\nworktree /tmp/repo-feature\nHEAD def\nbranch refs/heads/feature\n\n",
            true,
        );
        runner.add_response("", false); // main parent
        runner.add_response("", true); // main status
        runner.add_response("main", true); // feature parent
        runner.add_response("1 2", true); // feature ahead/behind
        runner.add_response(" M src/lib.rs", true); // feature status
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo-feature"),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        let rows = collect(&ctx).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].branch, "feature");
        assert!(rows[1].current);
        assert!(rows[1].dirty);
        assert_eq!(rows[1].parent.as_deref(), Some("main"));
        assert_eq!(rows[1].ahead, Some(2));
        assert_eq!(rows[1].behind, Some(1));
    }

    #[test]
    fn site_url_uses_matching_profile_config_from_branch_suffix() {
        let repo = tempfile::tempdir().unwrap();
        let profile_dir = repo.path().join(".git/wt/profiles/codex");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(
            profile_dir.join("profile.toml"),
            r#"
[site]
provider = "herd"
url = "https://profile-{{branch_slug}}.test"
"#,
        )
        .unwrap();

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let entry = WorktreeEntry {
            branch: "feature/cms-codex".into(),
            path: repo.path().join("repo-cms-codex"),
        };

        assert_eq!(
            site_url(&ctx, &entry).unwrap().as_deref(),
            Some("https://profile-cms-codex.test")
        );
    }

    #[test]
    fn render_table_uses_boxed_columns_and_inline_site_url() {
        let rows = vec![
            WorktreeRow {
                branch: "main".into(),
                path: "/tmp/repo".into(),
                current: false,
                dirty: false,
                parent: None,
                ahead: None,
                behind: None,
                site_url: None,
            },
            WorktreeRow {
                branch: "feature/profile-table".into(),
                path: "/tmp/repo-feature".into(),
                current: true,
                dirty: true,
                parent: Some("main".into()),
                ahead: Some(2),
                behind: Some(1),
                site_url: Some("https://feature.test".into()),
            },
        ];

        let rendered = render_table(&rows, None);

        assert!(rendered.contains("┌"));
        assert!(rendered.contains("│ CUR │ BRANCH"));
        assert!(rendered.contains("│ SITE"));
        assert!(rendered.contains("│ ●   │ feature/profile-table"));
        assert!(rendered.contains("│ dirty"));
        assert!(rendered.contains("│ +2/-1"));
        assert!(rendered.contains("│ https://feature.test"));
        assert!(!rendered.contains("site:"));
    }

    #[test]
    fn render_table_omits_site_column_without_site_urls() {
        let rows = vec![WorktreeRow {
            branch: "main".into(),
            path: "/tmp/repo".into(),
            current: true,
            dirty: false,
            parent: None,
            ahead: Some(0),
            behind: Some(0),
            site_url: None,
        }];

        let rendered = render_table(&rows, None);

        assert!(rendered.contains("│ CUR │ BRANCH │ STATE │ PARENT │ SYNC │ PATH"));
        assert!(rendered.contains("│ even"));
        assert!(!rendered.contains("SITE"));
    }

    #[test]
    fn render_table_truncates_to_fit_width_without_wrapping() {
        let rows = vec![WorktreeRow {
            branch: "feature/very-long-profile-table-branch-name".into(),
            path: "/Users/alice/projects/sample-app/worktrees/sample-app-feature-very-long-profile-table-branch-name".into(),
            current: true,
            dirty: false,
            parent: Some("origin/main-with-a-long-name".into()),
            ahead: Some(12),
            behind: Some(3),
            site_url: Some("https://sample-app-feature-very-long-profile-table-branch-name.test".into()),
        }];

        let rendered = render_table(&rows, Some(80));

        assert!(rendered.contains("..."));
        assert!(!rendered.contains("feature/very-long-profile-table-branch-name"));
        assert!(!rendered.contains("/Users/alice/projects/sample-app/worktrees"));
        for line in rendered.lines() {
            assert!(
                measure_text_width(line) <= 80,
                "line is too wide ({}): {line}",
                measure_text_width(line)
            );
        }
    }

    #[test]
    fn render_table_wide_keeps_full_values() {
        let rows = vec![WorktreeRow {
            branch: "feature/very-long-profile-table-branch-name".into(),
            path: "/tmp/sample-app-feature-very-long-profile-table-branch-name".into(),
            current: true,
            dirty: false,
            parent: Some("main".into()),
            ahead: Some(0),
            behind: Some(0),
            site_url: None,
        }];

        let rendered = render_table(&rows, None);

        assert!(rendered.contains("feature/very-long-profile-table-branch-name"));
        assert!(rendered.contains("/tmp/sample-app-feature-very-long-profile-table-branch-name"));
    }
}
