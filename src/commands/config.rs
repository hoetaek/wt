use crate::config::{AGENT_PROMPT_WORKFLOW_SCOPE, Config};
use crate::config_render::render_effective_config;
use crate::context::Ctx;
use crate::error::WtError;
use anyhow::{Context, Result, anyhow, bail};
use std::borrow::Cow;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn effective(ctx: &Ctx, profile: Option<&str>) -> Result<()> {
    let config = effective_config(ctx, profile)?;
    let rendered = render_effective_config(&config);

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(rendered.as_bytes())?;
    Ok(())
}

fn effective_config<'a>(ctx: &'a Ctx, profile: Option<&str>) -> Result<Cow<'a, Config>> {
    let Some(profile) = profile else {
        return Ok(Cow::Borrowed(&ctx.config));
    };

    let config = Config::load_profile_from_storage(
        &ctx.repo_root,
        &ctx.storage_root,
        profile,
        &ctx.base_config,
    )?
    .ok_or_else(|| anyhow!("Profile '{profile}' not found"))?;
    Ok(Cow::Owned(config))
}

pub fn extract(ctx: &Ctx, profile: Option<&str>, source: Option<&Path>) -> Result<()> {
    if profile.is_some() {
        bail!("wt config extract does not accept --profile; pass a source file instead");
    }

    let summary = match source {
        Some(source) => analyze_source(ctx, &resolve_source_path(ctx, source)?)?,
        None => select_source(ctx)?,
    };

    extract_from_source(ctx, summary)
}

pub fn inline(ctx: &Ctx, profile: Option<&str>, source: Option<&Path>) -> Result<()> {
    if profile.is_some() {
        bail!("wt config inline does not accept --profile; pass a source file instead");
    }

    let summary = match source {
        Some(source) => analyze_inline_source(ctx, &resolve_source_path(ctx, source)?)?,
        None => select_inline_source(ctx)?,
    };

    inline_from_source(ctx, summary)
}

pub fn edit(ctx: &Ctx, profile: Option<&str>, source: Option<&Path>) -> Result<()> {
    if profile.is_some() {
        bail!("wt config edit does not accept --profile; pass a source file instead");
    }

    let path = match source {
        Some(source) => resolve_source_path(ctx, source)?,
        None => select_edit_source(ctx)?,
    };
    crate::commands::editor::open_file(ctx, &path)
}

fn select_edit_source(ctx: &Ctx) -> Result<PathBuf> {
    let sources = discover_edit_sources(ctx)?;
    if sources.is_empty() {
        let path = ctx.storage_root.config_toml();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        return Ok(path);
    }

    if sources.len() == 1 {
        return Ok(sources[0].clone());
    }

    let items = sources
        .iter()
        .map(|path| display_path(ctx, path))
        .collect::<Vec<_>>();
    let selected = ctx.ui.select("Select config file:", &items)?;
    Ok(sources[selected].clone())
}

fn discover_edit_sources(ctx: &Ctx) -> Result<Vec<PathBuf>> {
    let mut sources = Vec::new();
    for path in [
        ctx.repo_root.join(".wt.toml"),
        ctx.storage_root.config_toml(),
    ] {
        if path.exists() {
            sources.push(path.canonicalize()?);
        }
    }

    let profiles_dir = ctx.storage_root.profiles_dir();
    if profiles_dir.exists() {
        let mut profile_sources = Vec::new();
        for entry in fs::read_dir(&profiles_dir)? {
            let entry = entry?;
            let path = entry.path().join("profile.toml");
            if path.exists() {
                profile_sources.push(path.canonicalize()?);
            }
        }
        profile_sources.sort();
        sources.extend(profile_sources);
    }
    Ok(sources)
}

fn display_path(ctx: &Ctx, path: &Path) -> String {
    relative_display(ctx, path)
}

fn select_inline_source(ctx: &Ctx) -> Result<InlineSummary> {
    let mut summaries = discover_inline_sources(ctx)?;
    summaries.sort_by(|a, b| {
        b.inlineable_count()
            .cmp(&a.inlineable_count())
            .then_with(|| b.blocked_count().cmp(&a.blocked_count()))
            .then_with(|| a.display.cmp(&b.display))
    });

    let inlineable = summaries
        .iter()
        .filter(|summary| summary.inlineable_count() > 0)
        .count();
    if inlineable == 0 {
        ctx.ui.print_step("No inlineable config items found.");
        return Err(WtError::Cancelled.into());
    }

    if inlineable == 1 {
        return summaries
            .into_iter()
            .find(|summary| summary.inlineable_count() > 0)
            .ok_or_else(|| anyhow!("No inlineable config items found"));
    }

    let visible = summaries
        .into_iter()
        .filter(|summary| summary.inlineable_count() > 0 || summary.blocked_count() > 0)
        .collect::<Vec<_>>();
    let items = visible.iter().map(InlineSummary::item).collect::<Vec<_>>();
    let selected = ctx.ui.select("Select config file:", &items)?;
    visible
        .into_iter()
        .nth(selected)
        .ok_or_else(|| anyhow!("Invalid config file selection"))
}

fn discover_inline_sources(ctx: &Ctx) -> Result<Vec<InlineSummary>> {
    let mut summaries = Vec::new();
    let local_config = ctx.storage_root.config_toml();
    if local_config.exists() {
        summaries.push(analyze_inline_source(ctx, &local_config)?);
    }

    let profiles_dir = ctx.storage_root.profiles_dir();
    if !profiles_dir.exists() {
        return Ok(summaries);
    }

    let mut paths = fs::read_dir(&profiles_dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    paths.sort();

    for summary in paths
        .into_iter()
        .filter(|path| path.is_dir())
        .map(|path| path.join("profile.toml"))
        .filter(|path| path.exists())
        .map(|path| analyze_inline_source(ctx, &path))
    {
        summaries.push(summary?);
    }

    Ok(summaries)
}

fn inline_from_source(ctx: &Ctx, summary: InlineSummary) -> Result<()> {
    if summary.inlineable_count() == 0 {
        print_no_inlineable(ctx, &summary);
        return Ok(());
    }

    let items = summary
        .candidates
        .iter()
        .map(InlineCandidate::item)
        .collect::<Vec<_>>();
    let selected_indices = ctx.ui.multi_select("Select items to inline:", &items)?;
    if selected_indices.is_empty() {
        ctx.ui.print_warning("No items selected");
        return Ok(());
    }

    let mut selected = Vec::new();
    for index in selected_indices {
        let candidate = summary
            .candidates
            .get(index)
            .ok_or_else(|| anyhow!("Invalid item selection"))?;
        if let Some(reason) = candidate.blocked.as_deref() {
            bail!("Selected item is blocked: {} ({reason})", candidate.name);
        }
        selected.push(candidate.clone());
    }

    let plan = build_inline_plan(ctx, &selected);
    ctx.ui.print_step("Plan:");
    for line in &plan {
        ctx.ui.print_dim(&format!("  {line}"));
    }

    if !ctx.ui.confirm("Apply?", true)? {
        return Err(WtError::Cancelled.into());
    }

    apply_inline_selected(ctx, &selected)?;
    ctx.ui.print_step("Config items inlined.");
    Ok(())
}

fn print_no_inlineable(ctx: &Ctx, summary: &InlineSummary) {
    ctx.ui.print_step(&format!(
        "No inlineable config items in {}.",
        summary.display
    ));

    for candidate in summary
        .candidates
        .iter()
        .filter(|candidate| candidate.blocked.is_some())
    {
        ctx.ui.print_dim(&format!("  {}", candidate.item()));
    }
}

fn select_source(ctx: &Ctx) -> Result<SourceSummary> {
    let mut summaries = discover_sources(ctx)?;
    summaries.sort_by(|a, b| {
        b.extractable_count()
            .cmp(&a.extractable_count())
            .then_with(|| b.blocked_count().cmp(&a.blocked_count()))
            .then_with(|| a.display.cmp(&b.display))
    });

    let extractable = summaries
        .iter()
        .filter(|summary| summary.extractable_count() > 0)
        .count();
    if extractable == 0 {
        ctx.ui.print_step("No extractable config sections found.");
        return Err(WtError::Cancelled.into());
    }

    if extractable == 1 {
        return summaries
            .into_iter()
            .find(|summary| summary.extractable_count() > 0)
            .ok_or_else(|| anyhow!("No extractable config sections found"));
    }

    let visible = summaries
        .into_iter()
        .filter(|summary| {
            summary.extractable_count() > 0
                || summary.blocked_count() > 0
                || summary.selected_profile.is_some()
        })
        .collect::<Vec<_>>();
    let items = visible.iter().map(SourceSummary::item).collect::<Vec<_>>();
    let selected = ctx.ui.select("Select config file:", &items)?;
    visible
        .into_iter()
        .nth(selected)
        .ok_or_else(|| anyhow!("Invalid config file selection"))
}

fn discover_sources(ctx: &Ctx) -> Result<Vec<SourceSummary>> {
    let mut paths = Vec::new();
    push_if_exists(&mut paths, ctx.repo_root.join(".wt.toml"));
    push_if_exists(&mut paths, ctx.storage_root.config_toml());

    let profiles_dir = ctx.storage_root.profiles_dir();
    if profiles_dir.exists() {
        let mut entries = fs::read_dir(&profiles_dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        entries.sort();
        for dir in entries {
            if dir.is_dir() {
                push_if_exists(&mut paths, dir.join("profile.toml"));
            }
        }
    }

    paths
        .into_iter()
        .map(|path| analyze_source(ctx, &path))
        .collect()
}

fn push_if_exists(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if path.exists() {
        paths.push(path);
    }
}

fn extract_from_source(ctx: &Ctx, summary: SourceSummary) -> Result<()> {
    if summary.extractable_count() == 0 {
        print_no_extractable(ctx, &summary);
        return Ok(());
    }

    let items = summary
        .candidates
        .iter()
        .map(ExtractCandidate::item)
        .collect::<Vec<_>>();
    let selected_indices = ctx.ui.multi_select("Select sections to extract:", &items)?;
    if selected_indices.is_empty() {
        ctx.ui.print_warning("No sections selected");
        return Ok(());
    }

    let mut selected = Vec::new();
    for index in selected_indices {
        let candidate = summary
            .candidates
            .get(index)
            .ok_or_else(|| anyhow!("Invalid section selection"))?;
        if let Some(reason) = candidate.blocked.as_deref() {
            bail!("Selected section is blocked: {} ({reason})", candidate.name);
        }
        selected.push(candidate.clone());
    }

    let profile_name = if selected
        .iter()
        .any(|candidate| matches!(candidate.kind, ExtractKind::InlineProfileToNamed))
    {
        let name = ctx.ui.input("Profile name:", None)?;
        crate::config::validate_profile_name(&name)?;
        Some(name)
    } else {
        None
    };

    let plan = build_plan(ctx, &summary, &selected, profile_name.as_deref())?;
    ctx.ui.print_step("Plan:");
    for line in &plan {
        ctx.ui.print_dim(&format!("  {line}"));
    }

    if !ctx.ui.confirm("Apply?", true)? {
        return Err(WtError::Cancelled.into());
    }

    apply_selected(ctx, &summary, &selected, profile_name.as_deref())?;
    ctx.ui.print_step("Config sections extracted.");
    Ok(())
}

fn print_no_extractable(ctx: &Ctx, summary: &SourceSummary) {
    ctx.ui
        .print_step(&format!("No extractable sections in {}.", summary.display));

    for candidate in summary
        .candidates
        .iter()
        .filter(|candidate| candidate.blocked.is_some())
    {
        ctx.ui.print_dim(&format!("  {}", candidate.item()));
    }

    if let Some(profile) = summary.selected_profile.as_deref() {
        let target = ctx.storage_root.display_path(
            &ctx.storage_root
                .profiles_dir()
                .join(profile)
                .join("profile.toml"),
        );
        ctx.ui.print_step("Selected profile:");
        ctx.ui.print_dim(&format!("  {profile} -> {target}"));
        ctx.ui
            .print_step(&format!("Try: wt config extract {target}"));
    }
}

fn build_plan(
    ctx: &Ctx,
    summary: &SourceSummary,
    selected: &[ExtractCandidate],
    profile_name: Option<&str>,
) -> Result<Vec<String>> {
    let mut plan = Vec::new();

    for candidate in selected {
        match &candidate.kind {
            ExtractKind::SharedToLocal { section } => {
                let target = ctx
                    .storage_root
                    .display_path(&ctx.storage_root.config_toml());
                plan.push(format!("move [{section}] -> {target} [{section}]"));
            }
            ExtractKind::InlineProfileToNamed => {
                let name = profile_name
                    .ok_or_else(|| anyhow!("Profile name is required for [profile.*] extract"))?;
                let profile_dir = ctx.storage_root.profiles_dir().join(name);
                let profile_toml = profile_dir.join("profile.toml");
                if profile_toml.exists() {
                    bail!(
                        "Profile '{name}' already exists: {}",
                        relative_display(ctx, &profile_dir)
                    );
                }
                plan.push(format!("create {}", relative_display(ctx, &profile_toml)));
                plan.push(format!(
                    "move [profile.*] -> {}",
                    relative_display(ctx, &profile_toml)
                ));
                plan.push(format!(
                    "replace inline profile in {} with [profile] name = \"{name}\"",
                    summary.display
                ));
            }
            ExtractKind::ProfilePrompt { mode, target, .. } => {
                if target.exists() {
                    bail!(
                        "Prompt file already exists: {}",
                        relative_display(ctx, target)
                    );
                }
                plan.push(format!("create {}", relative_display(ctx, target)));
                plan.push(format!(
                    "move [agent.prompt].{mode} -> {}",
                    relative_display(ctx, target)
                ));
            }
        }
    }

    Ok(plan)
}

fn apply_selected(
    ctx: &Ctx,
    summary: &SourceSummary,
    selected: &[ExtractCandidate],
    profile_name: Option<&str>,
) -> Result<()> {
    match selected.first().map(|candidate| &candidate.kind) {
        Some(ExtractKind::SharedToLocal { .. }) => apply_shared_to_local(ctx, summary, selected),
        Some(ExtractKind::InlineProfileToNamed) => apply_inline_profile(
            ctx,
            summary,
            profile_name.ok_or_else(|| anyhow!("Profile name is required"))?,
        ),
        Some(ExtractKind::ProfilePrompt { .. }) => apply_profile_prompts(ctx, summary, selected),
        None => Ok(()),
    }
}

fn apply_shared_to_local(
    ctx: &Ctx,
    summary: &SourceSummary,
    selected: &[ExtractCandidate],
) -> Result<()> {
    let source_content = fs::read_to_string(&summary.path)?;
    let target = ctx.storage_root.config_toml();
    let target_content = if target.exists() {
        fs::read_to_string(&target)?
    } else {
        String::new()
    };

    let sections = selected
        .iter()
        .map(|candidate| match &candidate.kind {
            ExtractKind::SharedToLocal { section } => Ok(section.as_str()),
            _ => bail!("Selected sections must come from the same extract kind"),
        })
        .collect::<Result<Vec<_>>>()?;

    for section in &sections {
        if has_root_section(&target_content, section) {
            bail!(
                "Target already has [{section}]: {}",
                relative_display(ctx, &target)
            );
        }
    }

    let mut moved = Vec::new();
    for section in &sections {
        let text = root_section_text(&source_content, section)
            .ok_or_else(|| anyhow!("Source no longer has [{section}]"))?;
        moved.push(text);
    }

    let updated_source = remove_root_sections(&source_content, &sections);
    let updated_target = append_sections(&target_content, &moved);
    toml::from_str::<Config>(&updated_source)
        .with_context(|| format!("updated {} would be invalid", summary.display))?;
    toml::from_str::<Config>(&updated_target).with_context(|| {
        format!(
            "updated {} would be invalid",
            relative_display(ctx, &target)
        )
    })?;

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&summary.path, updated_source)?;
    fs::write(target, updated_target)?;
    Ok(())
}

fn apply_inline_profile(ctx: &Ctx, summary: &SourceSummary, name: &str) -> Result<()> {
    let source_content = fs::read_to_string(&summary.path)?;
    let config = Config::load_file(&summary.path)?;
    let profile = config
        .profile
        .as_ref()
        .ok_or_else(|| anyhow!("No inline profile found in {}", summary.display))?;
    if let Some(existing) = profile.name.as_deref() {
        bail!("Local config already selects named profile '{existing}'");
    }
    if !profile.has_inline_settings() {
        bail!("No inline profile settings found in {}", summary.display);
    }

    let profile_dir = ctx.storage_root.profiles_dir().join(name);
    let profile_toml = profile_dir.join("profile.toml");
    if profile_toml.exists() {
        bail!(
            "Profile '{name}' already exists: {}",
            relative_display(ctx, &profile_dir)
        );
    }

    let promoted = render_inline_profile_as_profile_toml(&source_content)?;
    let updated_local = replace_inline_profile_with_name(&source_content, name)?;
    toml::from_str::<Config>(&promoted).with_context(|| {
        format!(
            "generated {} would be invalid",
            relative_display(ctx, &profile_toml)
        )
    })?;
    toml::from_str::<Config>(&updated_local)
        .with_context(|| format!("updated {} would be invalid", summary.display))?;

    fs::create_dir_all(&profile_dir)?;
    fs::write(profile_toml, promoted)?;
    fs::write(&summary.path, updated_local)?;
    Ok(())
}

fn apply_profile_prompts(
    ctx: &Ctx,
    summary: &SourceSummary,
    selected: &[ExtractCandidate],
) -> Result<()> {
    let source_content = fs::read_to_string(&summary.path)?;
    let mut doc = source_content
        .parse::<toml::Table>()
        .with_context(|| format!("failed to parse {}", summary.display))?;
    let root = &mut doc;

    let mut prompts = Vec::new();
    for candidate in selected {
        let ExtractKind::ProfilePrompt {
            mode,
            prompt,
            target,
        } = &candidate.kind
        else {
            bail!("Selected sections must come from the same extract kind");
        };
        if target.exists() {
            bail!(
                "Prompt file already exists: {}",
                relative_display(ctx, target)
            );
        }
        prompts.push((mode.as_str(), prompt.as_str(), target));
    }

    let agent = root
        .get_mut("agent")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| anyhow!("{} no longer has [agent]", summary.display))?;
    let prompt_table = agent
        .get_mut("prompt")
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| anyhow!("{} no longer has [agent.prompt]", summary.display))?;
    for (mode, _, _) in &prompts {
        prompt_table.remove(*mode);
    }
    if prompt_table.is_empty() {
        agent.remove("prompt");
    }
    if agent.is_empty() {
        root.remove("agent");
    }

    let updated = toml::to_string_pretty(&doc)?;
    toml::from_str::<Config>(&updated)
        .with_context(|| format!("updated {} would be invalid", summary.display))?;

    for (_, prompt, target) in &prompts {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, prompt)?;
    }
    fs::write(&summary.path, updated)?;
    Ok(())
}

fn analyze_source(ctx: &Ctx, path: &Path) -> Result<SourceSummary> {
    if !path.exists() {
        bail!("Config source does not exist: {}", path.display());
    }

    let path = path.to_path_buf();
    let display = relative_display(ctx, &path);
    let content = fs::read_to_string(&path)?;

    if same_existing_path(&path, &ctx.repo_root.join(".wt.toml")) {
        analyze_shared_config(ctx, path, display, &content)
    } else if same_existing_path(&path, &ctx.storage_root.config_toml()) {
        analyze_local_config(ctx, path, display, &content)
    } else if let Some(profile_name) = profile_name_for_source(ctx, &path) {
        analyze_profile_config(ctx, path, display, &content, profile_name)
    } else {
        bail!(
            "Unsupported config source: {display}. Use .wt.toml, <git-common-dir>/wt/config/local.toml, or <git-common-dir>/wt/config/profiles/<name>/profile.toml"
        );
    }
}

fn analyze_shared_config(
    ctx: &Ctx,
    path: PathBuf,
    display: String,
    content: &str,
) -> Result<SourceSummary> {
    Config::load_file(&path).with_context(|| format!("failed to load {display}"))?;
    let target = ctx.storage_root.config_toml();
    let target_content = if target.exists() {
        fs::read_to_string(&target)?
    } else {
        String::new()
    };

    let mut candidates = Vec::new();
    for section in root_sections(content) {
        if !is_movable_shared_section(&section) {
            continue;
        }
        let blocked = has_root_section(&target_content, &section)
            .then(|| format!("{} already has [{section}]", relative_display(ctx, &target)));
        candidates.push(ExtractCandidate {
            name: format!("[{section}]"),
            target: format!("{} [{section}]", relative_display(ctx, &target)),
            blocked,
            kind: ExtractKind::SharedToLocal { section },
        });
    }

    Ok(SourceSummary {
        path,
        display,
        candidates,
        selected_profile: None,
    })
}

fn analyze_local_config(
    _ctx: &Ctx,
    path: PathBuf,
    display: String,
    content: &str,
) -> Result<SourceSummary> {
    let config = Config::load_file(&path).with_context(|| format!("failed to load {display}"))?;
    let selected_profile = config
        .profile
        .as_ref()
        .and_then(|profile| profile.name.clone());
    let mut candidates = Vec::new();

    if config
        .profile
        .as_ref()
        .is_some_and(|profile| profile.name.is_none() && profile.has_inline_settings())
    {
        let has_profile_sections = content
            .lines()
            .any(|line| table_header(line).is_some_and(|section| section.starts_with("profile.")));
        if has_profile_sections {
            candidates.push(ExtractCandidate {
                name: "[profile.*]".into(),
                target: "<git-common-dir>/wt/config/profiles/<name>/profile.toml".into(),
                blocked: None,
                kind: ExtractKind::InlineProfileToNamed,
            });
        }
    }

    Ok(SourceSummary {
        path,
        display,
        candidates,
        selected_profile,
    })
}

fn analyze_profile_config(
    ctx: &Ctx,
    path: PathBuf,
    display: String,
    content: &str,
    _profile_name: String,
) -> Result<SourceSummary> {
    Config::load_file(&path).with_context(|| format!("failed to load {display}"))?;
    let doc = content
        .parse::<toml::Table>()
        .with_context(|| format!("failed to parse {display}"))?;
    let agent = doc.get("agent").and_then(toml::Value::as_table);
    let prompt = agent
        .and_then(|agent| agent.get("prompt"))
        .and_then(toml::Value::as_table);
    let agent_has_runtime = agent.is_some_and(|agent| agent.keys().any(|key| key != "prompt"));

    let mut candidates = Vec::new();
    if let Some(prompt) = prompt {
        let profile_dir = path
            .parent()
            .ok_or_else(|| anyhow!("Profile source has no parent directory: {display}"))?;
        for mode in ["issue", "branch", "pr", AGENT_PROMPT_WORKFLOW_SCOPE] {
            let Some(value) = prompt.get(mode) else {
                continue;
            };
            let target = profile_dir.join("prompts").join(format!("{mode}.md"));
            let prompt_values = value
                .as_array()
                .map(|values| {
                    values
                        .iter()
                        .filter_map(toml::Value::as_str)
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let blocked = if target.exists() {
                Some(format!("{} already exists", relative_display(ctx, &target)))
            } else if prompt_values.len() != 1 {
                Some("prompt array must contain exactly one string".into())
            } else if !agent_has_runtime && ctx.base_config.agent.is_none() {
                Some("no base [agent] is available for the prompt convention file".into())
            } else {
                None
            };
            candidates.push(ExtractCandidate {
                name: format!("[agent.prompt].{mode}"),
                target: relative_display(ctx, &target),
                blocked,
                kind: ExtractKind::ProfilePrompt {
                    mode: mode.into(),
                    prompt: prompt_values.first().cloned().unwrap_or_default(),
                    target,
                },
            });
        }
    }

    Ok(SourceSummary {
        path,
        display,
        candidates,
        selected_profile: None,
    })
}

fn analyze_inline_source(ctx: &Ctx, path: &Path) -> Result<InlineSummary> {
    if !path.exists() {
        bail!("Config source does not exist: {}", path.display());
    }

    let path = path.to_path_buf();
    let display = relative_display(ctx, &path);

    if same_existing_path(&path, &ctx.storage_root.config_toml()) {
        return analyze_inline_local_config(ctx, path, display);
    }

    if let Some(file_name) = path.file_name().and_then(|name| name.to_str())
        && matches!(file_name, "new.md" | "new.append.md")
    {
        bail!(
            "{display} is no longer supported; use prompts/branch.md or prompts/branch.append.md"
        );
    }

    if let Some(profile_name) = profile_name_for_source(ctx, &path) {
        return analyze_inline_profile(ctx, path, display, profile_name, None);
    }

    if let Some(prompt_source) = prompt_source_for_path(ctx, &path) {
        let profile_toml = prompt_source.profile_dir.join("profile.toml");
        if !profile_toml.exists() {
            bail!(
                "Profile config does not exist: {}",
                relative_display(ctx, &profile_toml)
            );
        }
        return analyze_inline_profile(
            ctx,
            profile_toml,
            display,
            prompt_source.profile_name,
            Some(path),
        );
    }

    bail!(
        "Unsupported inline source: {display}. Use <git-common-dir>/wt/config/local.toml, <git-common-dir>/wt/config/profiles/<name>/profile.toml, or <git-common-dir>/wt/config/profiles/<name>/prompts/<mode>.md"
    );
}

fn analyze_inline_local_config(ctx: &Ctx, path: PathBuf, display: String) -> Result<InlineSummary> {
    let config = Config::load_file(&path).with_context(|| format!("failed to load {display}"))?;
    let mut candidates = Vec::new();

    if let Some(profile_name) = config
        .profile
        .as_ref()
        .and_then(|profile| profile.name.clone())
    {
        let profile_toml = ctx
            .storage_root
            .profiles_dir()
            .join(&profile_name)
            .join("profile.toml");
        let blocked = named_profile_inline_blocker(ctx, &profile_name, &profile_toml)?;
        candidates.push(InlineCandidate {
            name: format!("[profile] name = {}", toml_quote(&profile_name)),
            target: format!("{} [profile.*]", relative_display(ctx, &path)),
            blocked,
            kind: InlineKind::NamedProfile {
                profile_name,
                local_toml: path.clone(),
                profile_toml,
            },
        });
    }

    Ok(InlineSummary {
        display,
        candidates,
    })
}

fn analyze_inline_profile(
    ctx: &Ctx,
    path: PathBuf,
    display: String,
    profile_name: String,
    only_source: Option<PathBuf>,
) -> Result<InlineSummary> {
    Config::load_file(&path)
        .with_context(|| format!("failed to load {}", relative_display(ctx, path.as_path())))?;
    let content = fs::read_to_string(&path)?;
    let doc = content
        .parse::<toml::Table>()
        .with_context(|| format!("failed to parse {}", relative_display(ctx, &path)))?;
    let profile = Config::load_profile_from_storage(
        &ctx.repo_root,
        &ctx.storage_root,
        &profile_name,
        &ctx.base_config,
    )?
    .with_context(|| format!("Profile '{profile_name}' not found"))?;
    let has_effective_agent = profile.agent.is_some();
    let profile_dir = path
        .parent()
        .ok_or_else(|| anyhow!("Profile source has no parent directory: {}", path.display()))?;

    let mut candidates = Vec::new();
    for spec in prompt_file_specs() {
        let source = profile_dir.join("prompts").join(spec.file_name());
        if !source.exists() {
            continue;
        }
        if only_source
            .as_ref()
            .is_some_and(|only_source| !same_existing_path(&source, only_source))
        {
            continue;
        }

        let prompt = fs::read_to_string(&source)?;
        let blocked = if target_prompt_key_exists(&doc, &spec) {
            Some(format!(
                "{} already has {}",
                relative_display(ctx, &path),
                spec.target_label()
            ))
        } else if !has_effective_agent {
            Some(
                "no base [agent] or profile [agent] is available for the prompt convention file"
                    .into(),
            )
        } else {
            None
        };

        candidates.push(InlineCandidate {
            name: relative_display(ctx, &source),
            target: format!("{} {}", relative_display(ctx, &path), spec.target_label()),
            blocked,
            kind: InlineKind::ProfilePromptFile {
                spec,
                source,
                prompt,
                profile_toml: path.clone(),
            },
        });
    }

    Ok(InlineSummary {
        display,
        candidates,
    })
}

fn build_inline_plan(ctx: &Ctx, selected: &[InlineCandidate]) -> Vec<String> {
    selected
        .iter()
        .map(|candidate| match &candidate.kind {
            InlineKind::ProfilePromptFile {
                spec,
                source,
                profile_toml,
                ..
            } => format!(
                "move {} -> {} {}",
                relative_display(ctx, source),
                relative_display(ctx, profile_toml),
                spec.target_label()
            ),
            InlineKind::NamedProfile {
                profile_name,
                local_toml,
                profile_toml,
            } => format!(
                "move {} -> {} [profile.*] for {profile_name}",
                relative_display(ctx, profile_toml),
                relative_display(ctx, local_toml)
            ),
        })
        .collect()
}

fn apply_inline_selected(ctx: &Ctx, selected: &[InlineCandidate]) -> Result<()> {
    let Some(first) = selected.first() else {
        return Ok(());
    };
    if matches!(first.kind, InlineKind::NamedProfile { .. }) {
        return apply_named_profile_inline(ctx, selected);
    }

    apply_inline_prompt_files(ctx, selected)
}

fn apply_inline_prompt_files(ctx: &Ctx, selected: &[InlineCandidate]) -> Result<()> {
    let Some(first) = selected.first() else {
        return Ok(());
    };
    let profile_toml = match &first.kind {
        InlineKind::ProfilePromptFile { profile_toml, .. } => profile_toml.clone(),
        InlineKind::NamedProfile { .. } => {
            bail!("Selected items must come from the same inline kind")
        }
    };
    if selected.iter().any(|candidate| match &candidate.kind {
        InlineKind::ProfilePromptFile {
            profile_toml: other,
            ..
        } => !same_existing_path(&profile_toml, other),
        InlineKind::NamedProfile { .. } => true,
    }) {
        bail!("Selected prompt files must belong to the same profile.toml");
    }

    let content = fs::read_to_string(&profile_toml)?;
    let mut doc = content
        .parse::<toml::Table>()
        .with_context(|| format!("failed to parse {}", relative_display(ctx, &profile_toml)))?;

    for candidate in selected {
        if let Some(reason) = candidate.blocked.as_deref() {
            bail!("Selected item is blocked: {} ({reason})", candidate.name);
        }
        let InlineKind::ProfilePromptFile { spec, prompt, .. } = &candidate.kind else {
            bail!("Selected items must come from the same inline kind");
        };
        if target_prompt_key_exists(&doc, spec) {
            bail!(
                "{} already has {}",
                relative_display(ctx, &profile_toml),
                spec.target_label()
            );
        }
        insert_agent_prompt(&mut doc, spec, prompt)?;
    }

    let updated = toml::to_string_pretty(&doc)?;
    toml::from_str::<Config>(&updated).with_context(|| {
        format!(
            "updated {} would be invalid",
            relative_display(ctx, &profile_toml)
        )
    })?;

    fs::write(&profile_toml, updated)?;
    for candidate in selected {
        let InlineKind::ProfilePromptFile { source, .. } = &candidate.kind else {
            bail!("Selected items must come from the same inline kind");
        };
        fs::remove_file(source)?;
    }
    remove_empty_prompts_dir(profile_toml.parent());
    Ok(())
}

fn apply_named_profile_inline(ctx: &Ctx, selected: &[InlineCandidate]) -> Result<()> {
    if selected.len() != 1 {
        bail!("Select one named profile to inline at a time");
    }

    let candidate = &selected[0];
    if let Some(reason) = candidate.blocked.as_deref() {
        bail!("Selected item is blocked: {} ({reason})", candidate.name);
    }

    let InlineKind::NamedProfile {
        profile_name,
        local_toml,
        profile_toml,
    } = &candidate.kind
    else {
        bail!("Selected items must come from the same inline kind");
    };

    if let Some(reason) = named_profile_inline_blocker(ctx, profile_name, profile_toml)? {
        bail!("Selected item is blocked: {} ({reason})", candidate.name);
    }

    let local_content = fs::read_to_string(local_toml)?;
    let profile_content = fs::read_to_string(profile_toml)?;
    let inline_profile = render_profile_toml_as_inline_profile(&profile_content)?;
    let updated_local =
        replace_named_profile_with_inline_profile(&local_content, profile_name, &inline_profile)?;
    toml::from_str::<Config>(&updated_local).with_context(|| {
        format!(
            "updated {} would be invalid",
            relative_display(ctx, local_toml)
        )
    })?;

    fs::write(local_toml, updated_local)?;
    fs::remove_file(profile_toml)?;
    remove_empty_profile_dir(profile_toml.parent());
    Ok(())
}

#[derive(Debug, Clone)]
struct SourceSummary {
    path: PathBuf,
    display: String,
    candidates: Vec<ExtractCandidate>,
    selected_profile: Option<String>,
}

impl SourceSummary {
    fn extractable_count(&self) -> usize {
        self.candidates
            .iter()
            .filter(|candidate| candidate.blocked.is_none())
            .count()
    }

    fn blocked_count(&self) -> usize {
        self.candidates
            .iter()
            .filter(|candidate| candidate.blocked.is_some())
            .count()
    }

    fn item(&self) -> String {
        let extractable = self.extractable_count();
        let blocked = self.blocked_count();
        let mut parts = Vec::new();
        parts.push(format!(
            "{extractable} extractable {}",
            plural(extractable, "section", "sections")
        ));
        if blocked > 0 {
            parts.push(format!(
                "{blocked} blocked {}",
                plural(blocked, "section", "sections")
            ));
        }
        if let Some(profile) = self.selected_profile.as_deref() {
            parts.push(format!("selected profile: {profile}"));
        }
        format!("{}  {}", self.display, parts.join(", "))
    }
}

#[derive(Debug, Clone)]
struct ExtractCandidate {
    name: String,
    target: String,
    blocked: Option<String>,
    kind: ExtractKind,
}

impl ExtractCandidate {
    fn item(&self) -> String {
        let base = format!("{} -> {}", self.name, self.target);
        match self.blocked.as_deref() {
            Some(reason) => format!("[blocked] {base} ({reason})"),
            None => base,
        }
    }
}

#[derive(Debug, Clone)]
enum ExtractKind {
    SharedToLocal {
        section: String,
    },
    InlineProfileToNamed,
    ProfilePrompt {
        mode: String,
        prompt: String,
        target: PathBuf,
    },
}

#[derive(Debug, Clone)]
struct InlineSummary {
    display: String,
    candidates: Vec<InlineCandidate>,
}

impl InlineSummary {
    fn inlineable_count(&self) -> usize {
        self.candidates
            .iter()
            .filter(|candidate| candidate.blocked.is_none())
            .count()
    }

    fn blocked_count(&self) -> usize {
        self.candidates
            .iter()
            .filter(|candidate| candidate.blocked.is_some())
            .count()
    }

    fn item(&self) -> String {
        let inlineable = self.inlineable_count();
        let blocked = self.blocked_count();
        let mut parts = Vec::new();
        parts.push(format!(
            "{inlineable} inlineable {}",
            plural(inlineable, "item", "items")
        ));
        if blocked > 0 {
            parts.push(format!(
                "{blocked} blocked {}",
                plural(blocked, "item", "items")
            ));
        }
        format!("{}  {}", self.display, parts.join(", "))
    }
}

#[derive(Debug, Clone)]
struct InlineCandidate {
    name: String,
    target: String,
    blocked: Option<String>,
    kind: InlineKind,
}

impl InlineCandidate {
    fn item(&self) -> String {
        let base = format!("{} -> {}", self.name, self.target);
        match self.blocked.as_deref() {
            Some(reason) => format!("[blocked] {base} ({reason})"),
            None => base,
        }
    }
}

#[derive(Debug, Clone)]
enum InlineKind {
    ProfilePromptFile {
        spec: PromptFileSpec,
        source: PathBuf,
        prompt: String,
        profile_toml: PathBuf,
    },
    NamedProfile {
        profile_name: String,
        local_toml: PathBuf,
        profile_toml: PathBuf,
    },
}

#[derive(Debug, Clone)]
struct PromptFileSpec {
    mode: String,
    append: bool,
}

impl PromptFileSpec {
    fn file_name(&self) -> String {
        if self.append {
            format!("{}.append.md", self.mode)
        } else {
            format!("{}.md", self.mode)
        }
    }

    fn target_label(&self) -> String {
        if self.append {
            format!("[agent.prompt.append].{}", self.mode)
        } else {
            format!("[agent.prompt].{}", self.mode)
        }
    }
}

fn resolve_source_path(ctx: &Ctx, source: &Path) -> Result<PathBuf> {
    let path = if source.is_absolute() {
        source.to_path_buf()
    } else {
        ctx.repo_root.join(source)
    };
    if path.exists() {
        Ok(path.canonicalize()?)
    } else {
        Ok(path)
    }
}

fn profile_name_for_source(ctx: &Ctx, path: &Path) -> Option<String> {
    let profiles_dir = ctx.storage_root.profiles_dir();
    let canonical_profiles_dir = profiles_dir.canonicalize().unwrap_or(profiles_dir);
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let relative = canonical_path.strip_prefix(&canonical_profiles_dir).ok()?;
    if relative.file_name()? != "profile.toml" {
        return None;
    }
    if relative.components().count() != 2 {
        return None;
    }
    relative
        .parent()?
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

struct PromptSource {
    profile_name: String,
    profile_dir: PathBuf,
}

fn prompt_source_for_path(ctx: &Ctx, path: &Path) -> Option<PromptSource> {
    let profiles_dir = ctx.storage_root.profiles_dir();
    let canonical_profiles_dir = profiles_dir.canonicalize().unwrap_or(profiles_dir);
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let relative = canonical_path.strip_prefix(&canonical_profiles_dir).ok()?;
    let mut components = relative.components();
    let profile_name = components
        .next()?
        .as_os_str()
        .to_string_lossy()
        .into_owned();
    if components.next()?.as_os_str() != "prompts" {
        return None;
    }
    let file_name = components
        .next()?
        .as_os_str()
        .to_string_lossy()
        .into_owned();
    if components.next().is_some() || prompt_file_spec(&file_name).is_none() {
        return None;
    }
    crate::config::validate_profile_name(&profile_name).ok()?;
    Some(PromptSource {
        profile_dir: canonical_profiles_dir.join(&profile_name),
        profile_name,
    })
}

fn prompt_file_specs() -> Vec<PromptFileSpec> {
    [
        "common",
        "issue",
        "branch",
        "pr",
        AGENT_PROMPT_WORKFLOW_SCOPE,
    ]
    .into_iter()
    .flat_map(|mode| {
        [
            PromptFileSpec {
                mode: mode.to_string(),
                append: false,
            },
            PromptFileSpec {
                mode: mode.to_string(),
                append: true,
            },
        ]
    })
    .collect()
}

fn prompt_file_spec(file_name: &str) -> Option<PromptFileSpec> {
    prompt_file_specs()
        .into_iter()
        .find(|spec| spec.file_name() == file_name)
}

fn same_existing_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn relative_display(ctx: &Ctx, path: &Path) -> String {
    if path.starts_with(ctx.storage_root.git_common_dir()) {
        return ctx.storage_root.display_path(path);
    }
    path.strip_prefix(&ctx.repo_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn plural<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}

fn is_movable_shared_section(section: &str) -> bool {
    matches!(
        section.split('.').next(),
        Some(
            "worktree"
                | "setup"
                | "profile"
                | "herd"
                | "site"
                | "workspace"
                | "agent"
                | "test"
                | "issues"
                | "editor"
        )
    )
}

fn root_sections(content: &str) -> Vec<String> {
    let mut sections = Vec::new();
    for line in content.lines() {
        let Some(header) = table_header(line) else {
            continue;
        };
        let root = header.split('.').next().unwrap_or(header).to_string();
        if !sections.contains(&root) {
            sections.push(root);
        }
    }
    sections
}

fn has_root_section(content: &str, root: &str) -> bool {
    content.lines().any(|line| {
        table_header(line)
            .is_some_and(|header| header == root || header.starts_with(&format!("{root}.")))
    })
}

fn target_prompt_key_exists(doc: &toml::Table, spec: &PromptFileSpec) -> bool {
    let Some(prompt) = doc
        .get("agent")
        .and_then(toml::Value::as_table)
        .and_then(|agent| agent.get("prompt"))
        .and_then(toml::Value::as_table)
    else {
        return false;
    };

    if spec.append {
        return prompt
            .get("append")
            .and_then(toml::Value::as_table)
            .is_some_and(|append| append.contains_key(&spec.mode));
    }

    prompt.contains_key(&spec.mode)
}

fn insert_agent_prompt(doc: &mut toml::Table, spec: &PromptFileSpec, prompt: &str) -> Result<()> {
    let agent = get_or_insert_table(doc, "agent")?;
    let prompt_table = get_or_insert_table(agent, "prompt")?;
    let value = toml::Value::Array(vec![toml::Value::String(prompt.to_string())]);

    if spec.append {
        let append_table = get_or_insert_table(prompt_table, "append")?;
        if append_table.contains_key(&spec.mode) {
            bail!("profile.toml already has {}", spec.target_label());
        }
        append_table.insert(spec.mode.clone(), value);
        return Ok(());
    }

    if prompt_table.contains_key(&spec.mode) {
        bail!("profile.toml already has {}", spec.target_label());
    }
    prompt_table.insert(spec.mode.clone(), value);
    Ok(())
}

fn get_or_insert_table<'a>(table: &'a mut toml::Table, key: &str) -> Result<&'a mut toml::Table> {
    if !table.contains_key(key) {
        table.insert(key.to_string(), toml::Value::Table(toml::Table::new()));
    }
    table
        .get_mut(key)
        .and_then(toml::Value::as_table_mut)
        .ok_or_else(|| anyhow!("{key} must be a table"))
}

fn remove_empty_prompts_dir(profile_dir: Option<&Path>) {
    let Some(profile_dir) = profile_dir else {
        return;
    };
    remove_dir_if_empty(&profile_dir.join("prompts"));
}

fn remove_empty_profile_dir(profile_dir: Option<&Path>) {
    let Some(profile_dir) = profile_dir else {
        return;
    };
    remove_dir_if_empty(&profile_dir.join("prompts"));
    remove_dir_if_empty(&profile_dir.join("scaffold"));
    remove_dir_if_empty(profile_dir);
}

fn remove_dir_if_empty(path: &Path) {
    let Ok(mut entries) = fs::read_dir(path) else {
        return;
    };
    if entries.next().is_none() {
        let _ = fs::remove_dir(path);
    }
}

fn named_profile_inline_blocker(
    ctx: &Ctx,
    profile_name: &str,
    profile_toml: &Path,
) -> Result<Option<String>> {
    if !profile_toml.exists() {
        return Ok(Some(format!(
            "profile config does not exist: {}",
            relative_display(ctx, profile_toml)
        )));
    }

    let profile_config = Config::load_file(profile_toml)
        .with_context(|| format!("failed to load {}", relative_display(ctx, profile_toml)))?;
    if profile_config.profile.is_some() {
        return Ok(Some("[profile] is not valid in profile.toml".into()));
    }

    let content = fs::read_to_string(profile_toml)?;
    let roots = profile_toml_root_sections(&content);
    for root in &roots {
        if !is_inlineable_profile_root(root) {
            return Ok(Some(format!(
                "[{root}] cannot be represented as inline [profile.*]"
            )));
        }
    }
    if roots.is_empty() {
        return Ok(Some(
            "profile.toml has no inlineable profile sections".into(),
        ));
    }

    let profile_dir = profile_toml
        .parent()
        .ok_or_else(|| anyhow!("Profile source has no parent directory: {profile_name}"))?;
    if profile_has_prompt_convention_files(profile_dir) {
        return Ok(Some(format!(
            "profile has prompt convention files; run wt config inline {} first",
            relative_display(ctx, profile_toml)
        )));
    }
    if dir_has_entries(&profile_dir.join("scaffold")) {
        return Ok(Some(
            "profile has scaffold files, and scaffold files are not inlined".into(),
        ));
    }

    Ok(None)
}

fn profile_has_prompt_convention_files(profile_dir: &Path) -> bool {
    let prompts_dir = profile_dir.join("prompts");
    prompt_file_specs()
        .iter()
        .any(|spec| prompts_dir.join(spec.file_name()).exists())
}

fn dir_has_entries(path: &Path) -> bool {
    fs::read_dir(path)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

fn profile_toml_root_sections(content: &str) -> Vec<String> {
    let mut sections = Vec::new();
    for line in content.lines() {
        let Some((_, header)) = toml_table_header(line) else {
            continue;
        };
        let root = header.split('.').next().unwrap_or(header).to_string();
        if !sections.contains(&root) {
            sections.push(root);
        }
    }
    sections
}

fn is_inlineable_profile_root(root: &str) -> bool {
    matches!(
        root,
        "worktree" | "setup" | "site" | "workspace" | "agent" | "test"
    )
}

fn render_profile_toml_as_inline_profile(content: &str) -> Result<String> {
    let mut lines = Vec::new();
    let mut has_profile_section = false;

    for line in content.lines() {
        if let Some((array, header)) = toml_table_header(line) {
            let root = header.split('.').next().unwrap_or(header);
            if !is_inlineable_profile_root(root) {
                bail!("[{root}] cannot be represented as inline [profile.*]");
            }
            has_profile_section = true;
            if array {
                lines.push(format!("[[profile.{header}]]"));
            } else {
                lines.push(format!("[profile.{header}]"));
            }
            continue;
        }

        lines.push(line.to_string());
    }

    let rendered = finalize_lines(lines);
    if !has_profile_section {
        bail!("No inlineable profile sections found");
    }
    Ok(rendered)
}

fn replace_named_profile_with_inline_profile(
    content: &str,
    name: &str,
    inline_profile: &str,
) -> Result<String> {
    let mut lines = Vec::new();
    let mut skipping_profile = false;

    for line in content.lines() {
        if let Some(header) = table_header(line) {
            skipping_profile = header == "profile";
            if skipping_profile {
                continue;
            }
        }

        if !skipping_profile {
            lines.push(line.to_string());
        }
    }

    let mut updated = finalize_lines(lines);
    updated = updated.trim_end().to_string();
    if !updated.is_empty() {
        updated.push_str("\n\n");
    }
    updated.push_str(inline_profile.trim());
    updated.push('\n');

    let parsed = toml::from_str::<Config>(&updated)?;
    let profile = parsed
        .profile
        .as_ref()
        .ok_or_else(|| anyhow!("updated config no longer has [profile.*]"))?;
    if profile.name.as_deref() == Some(name) {
        bail!("updated config still selects profile '{name}'");
    }
    Ok(updated)
}

fn root_section_text(content: &str, root: &str) -> Option<String> {
    let mut lines = Vec::new();
    let mut in_section = false;

    for line in content.lines() {
        if let Some(header) = table_header(line) {
            in_section = header == root || header.starts_with(&format!("{root}."));
        }
        if in_section {
            lines.push(line.to_string());
        }
    }

    (!lines.is_empty()).then(|| {
        let mut text = lines.join("\n");
        text.push('\n');
        text
    })
}

fn remove_root_sections(content: &str, roots: &[&str]) -> String {
    let mut lines = Vec::new();
    let mut skipping = false;

    for line in content.lines() {
        if let Some(header) = table_header(line) {
            skipping = roots
                .iter()
                .any(|root| header == *root || header.starts_with(&format!("{root}.")));
            if skipping {
                continue;
            }
        }
        if !skipping {
            lines.push(line.to_string());
        }
    }

    finalize_lines(lines)
}

fn append_sections(content: &str, sections: &[String]) -> String {
    let mut updated = content.trim_end().to_string();
    for section in sections {
        if !updated.is_empty() {
            updated.push_str("\n\n");
        }
        updated.push_str(section.trim());
        updated.push('\n');
    }
    updated
}

fn render_inline_profile_as_profile_toml(content: &str) -> Result<String> {
    let mut lines = Vec::new();
    let mut in_profile_section = false;

    for line in content.lines() {
        if let Some(header) = table_header(line) {
            in_profile_section = header.starts_with("profile.");
            if in_profile_section {
                let stripped = header
                    .strip_prefix("profile.")
                    .ok_or_else(|| anyhow!("Invalid profile section header: {header}"))?;
                lines.push(format!("[{stripped}]"));
                continue;
            }
        }

        if in_profile_section {
            lines.push(line.to_string());
        }
    }

    let rendered = finalize_lines(lines);
    if rendered.trim().is_empty() {
        bail!("No inline [profile.*] sections found");
    }
    Ok(rendered)
}

fn replace_inline_profile_with_name(content: &str, name: &str) -> Result<String> {
    let mut lines = Vec::new();
    let mut skipping_profile = false;

    for line in content.lines() {
        if let Some(header) = table_header(line) {
            skipping_profile = header == "profile" || header.starts_with("profile.");
            if skipping_profile {
                continue;
            }
        }

        if !skipping_profile {
            lines.push(line.to_string());
        }
    }

    let mut updated = finalize_lines(lines);
    updated = updated.trim_end().to_string();
    if !updated.is_empty() {
        updated.push_str("\n\n");
    }
    updated.push_str("[profile]\n");
    updated.push_str(&format!("name = {}\n", toml_quote(name)));
    toml::from_str::<Config>(&updated)?;
    Ok(updated)
}

fn finalize_lines(lines: Vec<String>) -> String {
    let mut rendered = lines.join("\n").trim_end().to_string();
    if !rendered.is_empty() {
        rendered.push('\n');
    }
    rendered
}

fn table_header(line: &str) -> Option<&str> {
    let trimmed = line
        .trim()
        .split_once('#')
        .map_or_else(|| line.trim(), |(before, _)| before.trim_end());
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return None;
    }
    let header = trimmed.trim_start_matches('[').trim_end_matches(']');
    (!header.starts_with('[') && !header.ends_with(']')).then_some(header)
}

fn toml_table_header(line: &str) -> Option<(bool, &str)> {
    let trimmed = line
        .trim()
        .split_once('#')
        .map_or_else(|| line.trim(), |(before, _)| before.trim_end());
    if trimmed.starts_with("[[") && trimmed.ends_with("]]") {
        let header = &trimmed[2..trimmed.len() - 2];
        return (!header.starts_with('[') && !header.ends_with(']')).then_some((true, header));
    }
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        let header = &trimmed[1..trimmed.len() - 1];
        return (!header.starts_with('[') && !header.ends_with(']')).then_some((false, header));
    }
    None
}

fn toml_quote(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentCli, Config};
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{Ctx, CtxOptions, OutputMode};

    fn ctx_with_ui(root: &Path, ui: MockUi) -> Ctx {
        Ctx::new_with_options(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
            CtxOptions {
                base_config: Config::default(),
                config_source: crate::config::ConfigSource::Default,
                storage_root: None,
                output_mode: OutputMode::Text,
                verbosity: 0,
                quiet: false,
                launcher_coordinator_id: None,
            },
        )
    }

    #[test]
    fn extract_inline_profile_creates_named_profile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git/wt/config")).unwrap();
        std::fs::write(
            dir.path().join(".git/wt/config/local.toml"),
            r#"
[workspace]
tabs = ["lazygit"]

[profile.agent]
cli = "codex"
args = ["--yolo"]

[profile.agent.prompt]
issue = ["Handle issue\n"]
"#,
        )
        .unwrap();

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0]);
        ui.add_input("codex");
        ui.add_confirm(true);
        let ctx = ctx_with_ui(dir.path(), ui);

        extract(&ctx, None, Some(Path::new(".git/wt/config/local.toml"))).unwrap();

        let local = std::fs::read_to_string(dir.path().join(".git/wt/config/local.toml")).unwrap();
        assert!(local.contains("[workspace]"));
        assert!(local.contains("[profile]"));
        assert!(local.contains("name = \"codex\""));
        assert!(!local.contains("[profile.agent]"));

        let profile = std::fs::read_to_string(
            dir.path()
                .join(".git/wt/config/profiles/codex/profile.toml"),
        )
        .unwrap();
        assert!(profile.contains("[agent]"));
        assert!(profile.contains("cli = \"codex\""));
        assert!(profile.contains("[agent.prompt]"));
        assert!(profile.contains("issue = [\"Handle issue\\n\"]"));

        let config = Config::load(dir.path()).unwrap();
        let agent = config.agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Codex);
        assert_eq!(agent.args, vec!["--yolo"]);
        assert_eq!(
            agent.prompt.get("issue").unwrap(),
            &vec!["Handle issue\n".to_string()]
        );
    }

    #[test]
    fn extract_profile_prompt_creates_convention_file() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join(".git/wt/config/profiles/codex");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(
            profile_dir.join("profile.toml"),
            r#"
[agent]
cli = "codex"

[agent.prompt]
issue = ["First\n", "Second\n"]
branch = ["Handle branch\n"]
"#,
        )
        .unwrap();

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![1]);
        ui.add_confirm(true);
        let ctx = ctx_with_ui(dir.path(), ui);

        extract(
            &ctx,
            None,
            Some(Path::new(".git/wt/config/profiles/codex/profile.toml")),
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(profile_dir.join("prompts/branch.md")).unwrap(),
            "Handle branch\n"
        );
        let updated = std::fs::read_to_string(profile_dir.join("profile.toml")).unwrap();
        assert!(!updated.contains("branch ="));
        assert!(updated.contains("issue = ["));
        assert!(updated.contains("First"));
        assert!(updated.contains("Second"));

        let profile = Config::load_profile(dir.path(), "codex", &Config::default())
            .unwrap()
            .unwrap();
        let agent = profile.agent.unwrap();
        assert_eq!(
            agent.prompt.get("issue").unwrap(),
            &vec!["First\n".to_string(), "Second\n".to_string()]
        );
        assert_eq!(
            agent.prompt.get("branch").unwrap(),
            &vec!["Handle branch\n".to_string()]
        );
    }

    #[test]
    fn inline_profile_prompt_files_moves_conventions_into_profile_toml() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join(".git/wt/config/profiles/codex");
        let prompts_dir = profile_dir.join("prompts");
        std::fs::create_dir_all(&prompts_dir).unwrap();
        std::fs::write(
            profile_dir.join("profile.toml"),
            r#"
[agent]
cli = "codex"
args = ["--yolo"]

[workspace]
tabs = ["pnpm dev"]
"#,
        )
        .unwrap();
        std::fs::write(prompts_dir.join("common.md"), "Common\n").unwrap();
        std::fs::write(prompts_dir.join("common.append.md"), "Common append\n").unwrap();
        std::fs::write(prompts_dir.join("issue.md"), "Issue\n").unwrap();
        std::fs::write(prompts_dir.join("issue.append.md"), "Issue append\n").unwrap();
        std::fs::write(prompts_dir.join("branch.md"), "Branch\n").unwrap();
        std::fs::write(prompts_dir.join("branch.append.md"), "Branch append\n").unwrap();
        std::fs::write(prompts_dir.join("pr.append.md"), "PR append\n").unwrap();

        let before = Config::load_profile(dir.path(), "codex", &Config::default())
            .unwrap()
            .unwrap();

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0, 1, 2, 3, 4, 5, 6]);
        ui.add_confirm(true);
        let ctx = ctx_with_ui(dir.path(), ui);

        inline(
            &ctx,
            None,
            Some(Path::new(".git/wt/config/profiles/codex/profile.toml")),
        )
        .unwrap();

        assert!(!prompts_dir.exists());
        let updated = std::fs::read_to_string(profile_dir.join("profile.toml")).unwrap();
        assert!(updated.contains("[agent.prompt]"));
        assert!(updated.contains("common"));
        assert!(updated.contains("Common"));
        assert!(updated.contains("issue"));
        assert!(updated.contains("Issue"));
        assert!(updated.contains("branch"));
        assert!(updated.contains("Branch"));
        assert!(updated.contains("[agent.prompt.append]"));
        assert!(updated.contains("pr"));
        assert!(updated.contains("PR append"));

        let after = Config::load_profile(dir.path(), "codex", &Config::default())
            .unwrap()
            .unwrap();
        assert_eq!(after, before);
    }

    #[test]
    fn inline_selected_named_profile_moves_profile_toml_into_local_config() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join(".git/wt/config/profiles/codex");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(
            dir.path().join(".wt.toml"),
            "[issues]\nprovider = \"github\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join(".git/wt/config")).unwrap();
        std::fs::write(
            dir.path().join(".git/wt/config/local.toml"),
            r#"
[worktree]
copy = [".env"]

[profile]
name = "codex"
"#,
        )
        .unwrap();
        std::fs::write(
            profile_dir.join("profile.toml"),
            r#"
[agent]
cli = "codex"
args = ["--yolo"]

[agent.prompt]
issue = ["Handle issue\n"]

[workspace]
tabs = ["pnpm dev"]
"#,
        )
        .unwrap();

        let before = Config::load(dir.path()).unwrap();

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0]);
        ui.add_confirm(true);
        let ctx = ctx_with_ui(dir.path(), ui);

        inline(&ctx, None, None).unwrap();

        assert!(!profile_dir.join("profile.toml").exists());
        let local = std::fs::read_to_string(dir.path().join(".git/wt/config/local.toml")).unwrap();
        assert!(local.contains("[profile.agent]"));
        assert!(local.contains("cli = \"codex\""));
        assert!(local.contains("[profile.agent.prompt]"));
        assert!(local.contains("[profile.workspace]"));
        assert!(!local.contains("name = \"codex\""));

        let after = Config::load(dir.path()).unwrap();
        assert_eq!(after, before);
    }

    #[test]
    fn inline_selected_named_profile_blocks_when_profile_has_convention_files() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join(".git/wt/config/profiles/codex");
        let prompts_dir = profile_dir.join("prompts");
        std::fs::create_dir_all(&prompts_dir).unwrap();
        std::fs::create_dir_all(dir.path().join(".git/wt/config")).unwrap();
        std::fs::write(
            dir.path().join(".git/wt/config/local.toml"),
            "[profile]\nname = \"codex\"\n",
        )
        .unwrap();
        std::fs::write(
            profile_dir.join("profile.toml"),
            "[agent]\ncli = \"codex\"\n",
        )
        .unwrap();
        std::fs::write(prompts_dir.join("issue.md"), "Handle issue\n").unwrap();

        let ctx = ctx_with_ui(dir.path(), MockUi::new());

        inline(&ctx, None, Some(Path::new(".git/wt/config/local.toml"))).unwrap();

        assert!(profile_dir.join("profile.toml").exists());
        assert!(prompts_dir.join("issue.md").exists());
        let local = std::fs::read_to_string(dir.path().join(".git/wt/config/local.toml")).unwrap();
        assert!(local.contains("name = \"codex\""));
        assert!(!local.contains("[profile.agent]"));
    }

    #[test]
    fn inline_profile_prompt_file_blocks_existing_prompt_key() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join(".git/wt/config/profiles/codex");
        let prompts_dir = profile_dir.join("prompts");
        std::fs::create_dir_all(&prompts_dir).unwrap();
        std::fs::write(
            profile_dir.join("profile.toml"),
            r#"
[agent]
cli = "codex"

[agent.prompt]
issue = ["Existing issue\n"]
"#,
        )
        .unwrap();
        std::fs::write(prompts_dir.join("common.md"), "Common\n").unwrap();
        std::fs::write(prompts_dir.join("issue.md"), "Issue file\n").unwrap();

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![1]);
        let ctx = ctx_with_ui(dir.path(), ui);

        let err = inline(
            &ctx,
            None,
            Some(Path::new(".git/wt/config/profiles/codex/profile.toml")),
        )
        .unwrap_err();

        assert!(err.to_string().contains("already has [agent.prompt].issue"));
        assert!(prompts_dir.join("issue.md").exists());
        let profile = std::fs::read_to_string(profile_dir.join("profile.toml")).unwrap();
        assert!(!profile.contains("Common"));
    }

    #[test]
    fn inline_accepts_direct_prompt_file_source() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join(".git/wt/config/profiles/codex");
        let prompts_dir = profile_dir.join("prompts");
        std::fs::create_dir_all(&prompts_dir).unwrap();
        std::fs::write(
            profile_dir.join("profile.toml"),
            r#"
[agent]
cli = "codex"
"#,
        )
        .unwrap();
        std::fs::write(prompts_dir.join("pr.append.md"), "PR append\n").unwrap();

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0]);
        ui.add_confirm(true);
        let ctx = ctx_with_ui(dir.path(), ui);

        inline(
            &ctx,
            None,
            Some(Path::new(
                ".git/wt/config/profiles/codex/prompts/pr.append.md",
            )),
        )
        .unwrap();

        assert!(!prompts_dir.join("pr.append.md").exists());
        let after = Config::load_profile(dir.path(), "codex", &Config::default())
            .unwrap()
            .unwrap();
        let agent = after.agent.unwrap();
        assert_eq!(
            agent.prompt.get("pr").unwrap(),
            &vec!["PR append\n".to_string()]
        );
    }

    #[test]
    fn inline_profile_prompt_file_blocks_when_convention_file_is_ineffective() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join(".git/wt/config/profiles/codex");
        let prompts_dir = profile_dir.join("prompts");
        std::fs::create_dir_all(&prompts_dir).unwrap();
        std::fs::write(profile_dir.join("profile.toml"), "").unwrap();
        std::fs::write(prompts_dir.join("issue.md"), "Issue\n").unwrap();

        let ctx = ctx_with_ui(dir.path(), MockUi::new());
        inline(
            &ctx,
            None,
            Some(Path::new(".git/wt/config/profiles/codex/profile.toml")),
        )
        .unwrap();

        assert!(prompts_dir.join("issue.md").exists());
        assert_eq!(
            std::fs::read_to_string(profile_dir.join("profile.toml")).unwrap(),
            ""
        );
    }

    #[test]
    fn extract_shared_section_moves_it_to_local_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".wt.toml"),
            r#"
[issues]
provider = "github"

[agent]
cli = "codex"
"#,
        )
        .unwrap();

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![1]);
        ui.add_confirm(true);
        let ctx = ctx_with_ui(dir.path(), ui);

        extract(&ctx, None, Some(Path::new(".wt.toml"))).unwrap();

        let shared = std::fs::read_to_string(dir.path().join(".wt.toml")).unwrap();
        assert!(shared.contains("[issues]"));
        assert!(!shared.contains("[agent]"));
        let local = std::fs::read_to_string(dir.path().join(".git/wt/config/local.toml")).unwrap();
        assert!(local.contains("[agent]"));
        assert!(local.contains("cli = \"codex\""));

        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.agent.unwrap().cli, AgentCli::Codex);
        assert!(config.issues.is_some());
    }

    #[test]
    fn editor_section_can_be_extracted_from_shared_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".wt.toml"),
            r#"
[editor]
command = "vi {{path}}"
"#,
        )
        .unwrap();

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0]);
        ui.add_confirm(true);
        let ctx = ctx_with_ui(dir.path(), ui);

        extract(&ctx, None, Some(Path::new(".wt.toml"))).unwrap();

        let shared = std::fs::read_to_string(dir.path().join(".wt.toml")).unwrap();
        assert!(!shared.contains("[editor]"));
        let local = std::fs::read_to_string(dir.path().join(".git/wt/config/local.toml")).unwrap();
        assert!(local.contains("[editor]"));
        assert!(local.contains("command = \"vi {{path}}\""));
    }

    #[test]
    fn selected_named_profile_suggests_profile_toml_without_extracting() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git/wt/config")).unwrap();
        std::fs::write(
            dir.path().join(".git/wt/config/local.toml"),
            "[profile]\nname = \"codex\"\n",
        )
        .unwrap();

        let ctx = ctx_with_ui(dir.path(), MockUi::new());
        extract(&ctx, None, Some(Path::new(".git/wt/config/local.toml"))).unwrap();
    }

    #[test]
    fn select_edit_source_lists_known_config_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git/wt/config/profiles/codex")).unwrap();
        std::fs::write(dir.path().join(".wt.toml"), "").unwrap();
        std::fs::write(dir.path().join(".git/wt/config/local.toml"), "").unwrap();
        std::fs::write(
            dir.path()
                .join(".git/wt/config/profiles/codex/profile.toml"),
            "",
        )
        .unwrap();
        let mut ui = MockUi::new();
        ui.add_select(1);
        let ctx = ctx_with_ui(dir.path(), ui);

        let selected = select_edit_source(&ctx).unwrap();

        assert!(selected.ends_with(".git/wt/config/local.toml"));
    }

    #[test]
    fn select_edit_source_defaults_to_local_config_when_none_exist() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_ui(dir.path(), MockUi::new());

        let selected = select_edit_source(&ctx).unwrap();

        assert!(selected.ends_with(".git/wt/config/local.toml"));
        assert!(dir.path().join(".git/wt").is_dir());
    }
}
