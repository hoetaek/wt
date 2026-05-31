use crate::context::{Ctx, PromptOption, PromptRow};
use crate::scaffold::{ALL_DOC_KINDS, DocKind};
use crate::task::safe_task_key;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScaffoldFlags {
    pub idea: bool,
    pub spec: bool,
    pub task: bool,
    pub workflow: bool,
    pub retrospect: bool,
    pub all: bool,
    pub force: bool,
}

#[derive(Debug, Serialize)]
struct ScaffoldReport {
    feature: String,
    created: Vec<String>,
    skipped: Vec<String>,
}

struct PlannedDocument {
    path: PathBuf,
    content: String,
}

pub fn run(ctx: &Ctx, feature: &str, flags: ScaffoldFlags) -> Result<()> {
    let report = execute(ctx, feature, flags)?;
    if ctx.is_json() {
        write_json(&report)?;
    } else {
        print_text(ctx, &report);
    }
    Ok(())
}

fn execute(ctx: &Ctx, feature: &str, flags: ScaffoldFlags) -> Result<ScaffoldReport> {
    let slug = validate_feature_slug(feature)?;
    let kinds = selected_kinds(ctx, flags)?;
    if kinds.is_empty() {
        return Ok(ScaffoldReport {
            feature: slug,
            created: Vec::new(),
            skipped: Vec::new(),
        });
    }

    ensure_no_legacy_scaffold_roots(ctx, &kinds)?;
    let planned = planned_documents(ctx, &slug, &kinds)?;
    let legacy_conflicts = legacy_spec_conflicts(ctx, &slug, &kinds);
    if !legacy_conflicts.is_empty() {
        let conflicts = legacy_conflicts
            .iter()
            .map(|path| ctx.storage_root.display_path(path))
            .collect::<Vec<_>>();
        bail!(
            "Legacy spec files from an older numbering already exist; rename or remove them before creating nine-gate spec files:\n{}",
            conflicts.join("\n")
        );
    }

    let conflicts = planned
        .iter()
        .filter(|document| document.path.exists())
        .map(|document| ctx.storage_root.display_path(&document.path))
        .collect::<Vec<_>>();

    if !conflicts.is_empty() && !flags.force {
        bail!(
            "Scaffold target files already exist; pass --force to overwrite:\n{}",
            conflicts.join("\n")
        );
    }

    let mut created = Vec::new();
    for document in planned {
        write_document(ctx, &document.path, &document.content, flags.force)?;
        created.push(ctx.storage_root.display_path(&document.path));
    }

    Ok(ScaffoldReport {
        feature: slug,
        created,
        skipped: Vec::new(),
    })
}

fn validate_feature_slug(feature: &str) -> Result<String> {
    let slug = feature.trim();
    if slug.is_empty() {
        bail!("Feature slug cannot be empty");
    }

    let normalized = safe_task_key(slug);
    if normalized != slug || normalized == "task" {
        bail!(
            "Invalid feature slug `{feature}`. Use a kebab-case slug containing only ASCII letters, numbers, '-' or '_'"
        );
    }

    Ok(slug.to_string())
}

fn selected_kinds(ctx: &Ctx, flags: ScaffoldFlags) -> Result<Vec<DocKind>> {
    if flags.all {
        return Ok(ALL_DOC_KINDS.to_vec());
    }

    let mut kinds = Vec::new();
    push_flagged(&mut kinds, flags.idea, DocKind::Idea);
    push_flagged(&mut kinds, flags.spec, DocKind::Spec);
    push_flagged(&mut kinds, flags.task, DocKind::Task);
    push_flagged(&mut kinds, flags.workflow, DocKind::Workflow);
    push_flagged(&mut kinds, flags.retrospect, DocKind::Retrospect);
    if !kinds.is_empty() {
        return Ok(kinds);
    }

    if !ctx.ui.can_prompt() {
        bail!(
            "No scaffold document kinds selected; pass --idea, --spec, --task, --workflow, --retrospect, or --all"
        );
    }

    let rows = ALL_DOC_KINDS
        .iter()
        .enumerate()
        .map(|(index, kind)| PromptRow::Option(PromptOption::new(kind.label()).value_index(index)))
        .collect::<Vec<_>>();
    let selections = ctx.ui.multi_select_rows("Scaffold documents", &rows)?;
    let mut selected = Vec::new();
    for index in selections {
        let Some(kind) = ALL_DOC_KINDS.get(index).copied() else {
            bail!("Invalid scaffold selection index {index}");
        };
        if !selected.contains(&kind) {
            selected.push(kind);
        }
    }
    Ok(selected)
}

fn push_flagged(kinds: &mut Vec<DocKind>, selected: bool, kind: DocKind) {
    if selected {
        kinds.push(kind);
    }
}

fn ensure_no_legacy_scaffold_roots(ctx: &Ctx, kinds: &[DocKind]) -> Result<()> {
    for kind in kinds {
        let legacy = match kind {
            DocKind::Idea => ctx.storage_root.detect_legacy_ideas(&ctx.repo_root),
            DocKind::Spec | DocKind::Retrospect => {
                ctx.storage_root.detect_legacy_specs(&ctx.repo_root)
            }
            DocKind::Task => ctx.storage_root.detect_legacy_tasks(&ctx.repo_root),
            DocKind::Workflow => ctx.storage_root.detect_legacy_workflows(&ctx.repo_root),
        };
        if let Some(legacy) = legacy {
            bail!("{}", legacy.error_message_for(kind.legacy_state_name()));
        }
    }
    Ok(())
}

fn planned_documents(ctx: &Ctx, slug: &str, kinds: &[DocKind]) -> Result<Vec<PlannedDocument>> {
    let mut planned = Vec::new();
    for kind in kinds {
        let paths = kind.paths(&ctx.storage_root, slug);
        let rendered = kind.render(slug);
        if paths.len() != rendered.len() {
            bail!("Internal scaffold renderer mismatch for {}", kind.label());
        }
        planned.extend(
            paths
                .into_iter()
                .zip(rendered)
                .map(|(path, (_relative_path, content))| PlannedDocument { path, content }),
        );
    }
    Ok(planned)
}

fn legacy_spec_conflicts(ctx: &Ctx, slug: &str, kinds: &[DocKind]) -> Vec<PathBuf> {
    if !kinds
        .iter()
        .any(|kind| matches!(kind, DocKind::Spec | DocKind::Retrospect))
    {
        return Vec::new();
    }

    let spec_dir = ctx.storage_root.specs_dir().join(slug);
    [
        // Unnumbered pre-work-sequence files.
        "requirements.md",
        "design.md",
        "tasks.md",
        "workflow.md",
        "mid-process-discoveries.md",
        // Pre-9-gate numbered files (older work-sequence numbering).
        "01-Learn/03-context.md",
        "02-Example/04+05-requirements.md",
        "02-Example/04+05+06-requirements.md",
        "02-Example/06-wireframe.md",
        "03-Architect/07-design.md",
        "03-Architect/08-tasks.md",
        "03-Architect/09-execution.md",
        "04-Feedback/10-review.md",
        "04-Feedback/11-retrospect.md",
    ]
    .into_iter()
    .map(|name| spec_dir.join(name))
    .filter(|path| path.exists())
    .collect()
}

fn write_document(ctx: &Ctx, path: &Path, content: &str, force: bool) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create scaffold directory: {}",
                ctx.storage_root.display_path(parent)
            )
        })?;
    }

    let mut options = fs::OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }

    let display_path = ctx.storage_root.display_path(path);
    let mut file = options
        .open(path)
        .with_context(|| format!("Failed to write scaffold file: {display_path}"))?;
    file.write_all(content.as_bytes())
        .with_context(|| format!("Failed to write scaffold file: {display_path}"))?;
    Ok(())
}

fn print_text(ctx: &Ctx, report: &ScaffoldReport) {
    if report.created.is_empty() && report.skipped.is_empty() {
        ctx.ui
            .print_plain("No scaffold documents selected; nothing created.");
        return;
    }

    for path in &report.created {
        ctx.ui.print_plain(&format!("created {path}"));
    }
}

fn write_json(report: &ScaffoldReport) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, report)?;
    writeln!(handle)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CtxOptions, OutputMode};

    fn ctx(root: &Path, ui: MockUi, output_mode: OutputMode) -> Ctx {
        Ctx::new_with_options(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
            CtxOptions {
                output_mode,
                ..CtxOptions::default()
            },
        )
    }

    fn text_ctx(root: &Path) -> Ctx {
        ctx(root, MockUi::new(), OutputMode::Text)
    }

    fn flags(kinds: &[DocKind]) -> ScaffoldFlags {
        let mut flags = ScaffoldFlags::default();
        for kind in kinds {
            match kind {
                DocKind::Idea => flags.idea = true,
                DocKind::Spec => flags.spec = true,
                DocKind::Task => flags.task = true,
                DocKind::Workflow => flags.workflow = true,
                DocKind::Retrospect => flags.retrospect = true,
            }
        }
        flags
    }

    fn relative_files(ctx: &Ctx) -> Vec<String> {
        let mut files = Vec::new();
        collect_files(
            ctx.storage_root.personal_root(),
            ctx.storage_root.personal_root(),
            &mut files,
        );
        files.sort();
        files
    }

    fn collect_files(root: &Path, current: &Path, files: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(current) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect_files(root, &path, files);
            } else if path.is_file() {
                files.push(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }

    #[test]
    fn rejects_invalid_feature_slugs() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = text_ctx(dir.path());
        for feature in ["", "foo bar", "foo/bar"] {
            let err = run(&ctx, feature, flags(&[DocKind::Idea]))
                .unwrap_err()
                .to_string();
            assert!(err.contains("Feature slug") || err.contains("Invalid feature slug"));
        }
    }

    #[test]
    fn creates_idea_and_task_from_flags() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = text_ctx(dir.path());

        run(&ctx, "foo", flags(&[DocKind::Idea, DocKind::Task])).unwrap();

        assert_eq!(
            relative_files(&ctx),
            vec![
                "execution/tasks/foo.toml".to_string(),
                "planning/ideas/foo.md".to_string()
            ]
        );
        assert_eq!(
            fs::read_to_string(ctx.storage_root.ideas_dir().join("foo.md")).unwrap(),
            DocKind::Idea.render("foo")[0].1
        );
        assert_eq!(
            fs::read_to_string(ctx.storage_root.tasks_dir().join("foo.toml")).unwrap(),
            DocKind::Task.render("foo")[0].1
        );
    }

    #[test]
    fn creates_spec_directory_and_files() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = text_ctx(dir.path());

        run(&ctx, "foo", flags(&[DocKind::Spec])).unwrap();

        let spec_dir = ctx.storage_root.specs_dir().join("foo");
        assert!(spec_dir.is_dir());
        assert_eq!(
            relative_files(&ctx),
            vec![
                "planning/specs/foo/00-status.md".to_string(),
                "planning/specs/foo/01-Learn/01-intent.md".to_string(),
                "planning/specs/foo/01-Learn/02-unknowns.md".to_string(),
                "planning/specs/foo/02-Example/03-criteria.md".to_string(),
                "planning/specs/foo/02-Example/04-wireframe.md".to_string(),
                "planning/specs/foo/03-Architect/05-design.md".to_string(),
                "planning/specs/foo/03-Architect/06-tasks.md".to_string()
            ]
        );
        assert_eq!(
            fs::read_to_string(spec_dir.join("00-status.md")).unwrap(),
            DocKind::Spec.render("foo")[0].1
        );
        assert_eq!(
            fs::read_to_string(spec_dir.join("01-Learn/01-intent.md")).unwrap(),
            DocKind::Spec.render("foo")[1].1
        );
        assert_eq!(
            fs::read_to_string(spec_dir.join("02-Example/03-criteria.md")).unwrap(),
            DocKind::Spec.render("foo")[3].1
        );
        assert_eq!(
            fs::read_to_string(spec_dir.join("02-Example/04-wireframe.md")).unwrap(),
            DocKind::Spec.render("foo")[4].1
        );
        assert_eq!(
            fs::read_to_string(spec_dir.join("03-Architect/05-design.md")).unwrap(),
            DocKind::Spec.render("foo")[5].1
        );
        assert_eq!(
            fs::read_to_string(spec_dir.join("03-Architect/06-tasks.md")).unwrap(),
            DocKind::Spec.render("foo")[6].1
        );
    }

    #[test]
    fn spec_scaffold_rejects_legacy_unnumbered_spec_files() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = text_ctx(dir.path());
        let spec_dir = ctx.storage_root.specs_dir().join("foo");
        fs::create_dir_all(&spec_dir).unwrap();
        fs::write(spec_dir.join("requirements.md"), "legacy").unwrap();

        let err = run(&ctx, "foo", flags(&[DocKind::Spec])).unwrap_err();

        assert!(
            format!("{err:#}").contains("Legacy spec files from an older numbering already exist"),
            "{err:#}"
        );
        assert!(!spec_dir.join("01-Learn/01-intent.md").exists());
    }

    #[test]
    fn retrospect_scaffold_uses_spec_local_path() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = text_ctx(dir.path());

        run(&ctx, "foo", flags(&[DocKind::Retrospect])).unwrap();

        assert_eq!(
            relative_files(&ctx),
            vec!["planning/specs/foo/04-Feedback/09-retrospect.md".to_string()]
        );
        assert_eq!(
            fs::read_to_string(
                ctx.storage_root
                    .specs_dir()
                    .join("foo/04-Feedback/09-retrospect.md")
            )
            .unwrap(),
            DocKind::Retrospect.render("foo")[0].1
        );
    }

    #[test]
    fn all_flag_creates_every_kind() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = text_ctx(dir.path());

        run(
            &ctx,
            "foo",
            ScaffoldFlags {
                all: true,
                ..ScaffoldFlags::default()
            },
        )
        .unwrap();

        assert_eq!(
            relative_files(&ctx),
            vec![
                "execution/tasks/foo.toml".to_string(),
                "execution/workflows/foo.toml".to_string(),
                "planning/ideas/foo.md".to_string(),
                "planning/specs/foo/00-status.md".to_string(),
                "planning/specs/foo/01-Learn/01-intent.md".to_string(),
                "planning/specs/foo/01-Learn/02-unknowns.md".to_string(),
                "planning/specs/foo/02-Example/03-criteria.md".to_string(),
                "planning/specs/foo/02-Example/04-wireframe.md".to_string(),
                "planning/specs/foo/03-Architect/05-design.md".to_string(),
                "planning/specs/foo/03-Architect/06-tasks.md".to_string(),
                "planning/specs/foo/04-Feedback/09-retrospect.md".to_string()
            ]
        );
    }

    #[test]
    fn no_flags_uses_multi_select() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0, 4]);
        let ctx = ctx(dir.path(), ui, OutputMode::Text);

        run(&ctx, "foo", ScaffoldFlags::default()).unwrap();

        assert_eq!(
            relative_files(&ctx),
            vec![
                "planning/ideas/foo.md".to_string(),
                "planning/specs/foo/04-Feedback/09-retrospect.md".to_string()
            ]
        );
    }

    #[test]
    fn conflict_without_force_is_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = text_ctx(dir.path());
        fs::create_dir_all(ctx.storage_root.tasks_dir()).unwrap();
        fs::write(ctx.storage_root.tasks_dir().join("foo.toml"), "existing").unwrap();

        let err = run(&ctx, "foo", flags(&[DocKind::Idea, DocKind::Task]))
            .unwrap_err()
            .to_string();

        assert!(err.contains("<repo-root>/.wt/execution/tasks/foo.toml"));
        assert!(!ctx.storage_root.ideas_dir().join("foo.md").exists());
    }

    #[test]
    fn force_overwrites_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = text_ctx(dir.path());
        fs::create_dir_all(ctx.storage_root.tasks_dir()).unwrap();
        fs::write(ctx.storage_root.tasks_dir().join("foo.toml"), "existing").unwrap();

        run(
            &ctx,
            "foo",
            ScaffoldFlags {
                force: true,
                ..flags(&[DocKind::Idea, DocKind::Task])
            },
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(ctx.storage_root.tasks_dir().join("foo.toml")).unwrap(),
            DocKind::Task.render("foo")[0].1
        );
        assert!(ctx.storage_root.ideas_dir().join("foo.md").exists());
    }

    #[test]
    fn json_report_has_expected_shape() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), MockUi::new(), OutputMode::Json);

        let report = execute(&ctx, "foo", flags(&[DocKind::Retrospect])).unwrap();
        assert!(ctx.is_json());

        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["feature"], "foo");
        assert_eq!(
            json["created"].as_array().unwrap()[0],
            "<repo-root>/.wt/planning/specs/foo/04-Feedback/09-retrospect.md"
        );
        assert!(json["skipped"].as_array().unwrap().is_empty());
    }
}
