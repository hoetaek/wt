use crate::context::Ctx;
use crate::local_ui::{ServerOptions, serve};
use anyhow::{Context, Result};

pub fn run(ctx: &Ctx, port: u16) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start local UI runtime")?;

    runtime.block_on(serve(
        ctx,
        ServerOptions {
            host: "127.0.0.1".into(),
            port,
        },
    ))
}
