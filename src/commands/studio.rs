use crate::context::Ctx;
use crate::studio::{ServerOptions, serve};
use anyhow::{Context, Result};

pub fn run(ctx: &Ctx, port: u16, dev: bool, dev_origin: Option<String>) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start studio runtime")?;

    runtime.block_on(serve(
        ctx,
        ServerOptions {
            host: "127.0.0.1".into(),
            port,
            dev_asset_origin: dev
                .then(|| dev_origin.unwrap_or_else(|| "http://127.0.0.1:5173".into())),
        },
    ))
}
