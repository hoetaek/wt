use crate::context::Ctx;
use crate::messages::AgentId;
use crate::services::identity_locator::{self, AnchorKind, Marker};
use anyhow::{Result, bail};
use serde::Serialize;
use std::env;
use std::io::{self, Write};

pub fn set(ctx: &Ctx, id: &str) -> Result<()> {
    let agent = AgentId::parse(id)?;
    let key = identity_locator::current_anchor_key()?;
    identity_locator::write_marker(
        ctx,
        &key,
        agent.as_str(),
        identity_locator::current_agent_kind().as_deref(),
    )?;
    println!("export WT_AGENT_ID={};", agent.as_str());
    Ok(())
}

pub fn unset(ctx: &Ctx) -> Result<()> {
    let key = identity_locator::current_anchor_key()?;
    identity_locator::remove_marker(ctx, &key)?;
    println!("unset WT_AGENT_ID;");
    Ok(())
}

pub fn whoami(ctx: &Ctx, json: bool) -> Result<()> {
    let report = resolve_report(ctx)?;
    if json {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        serde_json::to_writer_pretty(&mut handle, &report)?;
        writeln!(handle)?;
    } else {
        print_text_report(&report);
    }
    Ok(())
}

fn resolve_report(ctx: &Ctx) -> Result<WhoamiReport> {
    let key = identity_locator::current_anchor_key()?;
    let marker_path = identity_locator::marker_path(ctx, &key);
    let cmux_workspace_id = (key.kind == AnchorKind::Surface)
        .then(|| env::var("CMUX_WORKSPACE_ID").ok())
        .flatten()
        .filter(|value| !value.trim().is_empty());

    if let Some(id) = agent_id_from_env()? {
        return Ok(WhoamiReport {
            id: Some(id),
            source: IdentitySource::Env,
            anchor_kind: anchor_kind_name(&key.kind).into(),
            anchor_value: key.value,
            marker_path: marker_path.display().to_string(),
            cmux_workspace_id,
        });
    }

    if let Some(marker) = identity_locator::resolve_identity(ctx)? {
        return Ok(report_from_marker(marker, marker_path, cmux_workspace_id));
    }

    Ok(WhoamiReport {
        id: None,
        source: IdentitySource::None,
        anchor_kind: anchor_kind_name(&key.kind).into(),
        anchor_value: key.value,
        marker_path: marker_path.display().to_string(),
        cmux_workspace_id,
    })
}

fn agent_id_from_env() -> Result<Option<String>> {
    match env::var("WT_AGENT_ID") {
        Ok(value) => {
            let value = value.trim();
            if value.is_empty() {
                Ok(None)
            } else {
                Ok(Some(
                    AgentId::parse(value)
                        .map_err(|err| anyhow::anyhow!("Invalid WT_AGENT_ID: {err:#}"))?
                        .as_str()
                        .to_string(),
                ))
            }
        }
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => bail!("Invalid WT_AGENT_ID: value is not Unicode"),
    }
}

fn report_from_marker(
    marker: Marker,
    marker_path: std::path::PathBuf,
    cmux_workspace_id: Option<String>,
) -> WhoamiReport {
    WhoamiReport {
        id: Some(marker.id),
        source: IdentitySource::Marker,
        anchor_kind: anchor_kind_name(&marker.anchor_kind).into(),
        anchor_value: marker.anchor_value,
        marker_path: marker_path.display().to_string(),
        cmux_workspace_id,
    }
}

fn print_text_report(report: &WhoamiReport) {
    println!("id: {}", report.id.as_deref().unwrap_or("none"));
    println!("source: {}", report.source.as_str());
    println!("anchor_kind: {}", report.anchor_kind);
    println!("anchor_value: {}", report.anchor_value);
    println!("marker: {}", report.marker_path);
    if report.anchor_kind == "surface" {
        if let Some(workspace) = report.cmux_workspace_id.as_deref() {
            println!("cmux_workspace_id: {workspace}");
        }
    }
}

fn anchor_kind_name(kind: &AnchorKind) -> &'static str {
    match kind {
        AnchorKind::Surface => "surface",
        AnchorKind::ClaudeSession => "claude-session",
        AnchorKind::CodexThread => "codex-thread",
        AnchorKind::ShellSid => "shell-sid",
    }
}

#[derive(Debug, Serialize)]
struct WhoamiReport {
    id: Option<String>,
    source: IdentitySource,
    anchor_kind: String,
    anchor_value: String,
    marker_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cmux_workspace_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum IdentitySource {
    Env,
    Marker,
    None,
}

impl IdentitySource {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::Marker => "marker",
            Self::None => "none",
        }
    }
}
