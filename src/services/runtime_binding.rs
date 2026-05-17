use crate::context::Ctx;
use crate::services::work::{self, CmuxContact, Work, WorkTarget};
use anyhow::{Result, bail};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeBinding {
    pub(crate) target: WorkTarget,
    pub(crate) contact: CmuxContact,
}

pub(crate) struct RuntimeBindingResolver<'a> {
    ctx: &'a Ctx,
}

impl<'a> RuntimeBindingResolver<'a> {
    pub(crate) fn new(ctx: &'a Ctx) -> Self {
        Self { ctx }
    }

    pub(crate) fn observe(&self, target: Option<&str>) -> Result<Work> {
        let target = work::resolve_target(self.ctx, target)?;
        work::observe_target(self.ctx, target)
    }

    pub(crate) fn unique_live_binding(&self, work: &Work) -> Option<RuntimeBinding> {
        let contacts = live_contacts(&work.cmux_contacts);
        match contacts.as_slice() {
            [contact] => Some(RuntimeBinding {
                target: work.target.clone(),
                contact: (*contact).clone(),
            }),
            _ => None,
        }
    }

    pub(crate) fn live_candidates(&self, work: &Work) -> Vec<CmuxContact> {
        live_contacts(&work.cmux_contacts)
            .into_iter()
            .cloned()
            .collect()
    }

    pub(crate) fn bind_contact(&self, work: &Work, contact: &CmuxContact) -> RuntimeBinding {
        RuntimeBinding {
            target: work.target.clone(),
            contact: contact.clone(),
        }
    }

    pub(crate) fn revalidate(&self, binding: &RuntimeBinding) -> Result<RuntimeBinding> {
        let worktree = binding.target.worktree.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "Runtime binding target is not checked out in a local worktree: {}",
                binding.target.branch
            )
        })?;
        let contacts = work::cmux_contacts(self.ctx, worktree)?;
        let Some(contact) = contacts.into_iter().find(|candidate| {
            same_runtime_contact(candidate, &binding.contact) && candidate.is_live_agent_candidate()
        }) else {
            bail!(
                "Runtime binding for {} is stale or no longer a live agent surface after revalidation: {} {}",
                binding.target.label,
                binding.contact.workspace,
                binding.contact.surface
            );
        };

        Ok(RuntimeBinding {
            target: binding.target.clone(),
            contact,
        })
    }
}

fn live_contacts(contacts: &[CmuxContact]) -> Vec<&CmuxContact> {
    contacts
        .iter()
        .filter(|contact| contact.is_live_agent_candidate())
        .collect()
}

fn same_runtime_contact(a: &CmuxContact, b: &CmuxContact) -> bool {
    a.workspace_id == b.workspace_id
        && a.workspace == b.workspace
        && a.surface == b.surface
        && a.pane == b.pane
        && same_optional_id(a.surface_id.as_deref(), b.surface_id.as_deref())
}

fn same_optional_id(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}
