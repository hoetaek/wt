use crate::messages::AgentId;
use anyhow::Result;

pub fn use_coordinator(id: &str) -> Result<()> {
    let agent = AgentId::parse(id)?;
    println!("export WT_AGENT_ID={};", agent.as_str());
    println!("export WT_COORDINATOR_AGENT_ID={};", agent.as_str());
    Ok(())
}

pub fn exit_coordinator() {
    println!("unset WT_AGENT_ID;");
    println!("unset WT_COORDINATOR_AGENT_ID;");
}
