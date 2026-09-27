//! Model wiring: which embedder a vault opens with, and which chat provider answers.
//! The rules themselves live in `b2-embed` and `b2-llm`, shared with the desktop; this
//! module only lays the CLI's flags over them.

use crate::args::LlmArgs;
use crate::error::CliError;
use b2_core::llm::LlmProvider;
use b2_core::vault::Vault;
use b2_llm::LlmConfig;
use std::path::Path;

/// The chat provider: the fake under `B2_LLM=fake`, else the real client from the
/// environment plus flags, probed first so a stopped daemon fails before the question.
pub fn open_llm(args: &LlmArgs) -> Result<Box<dyn LlmProvider>, CliError> {
    let config =
        LlmConfig::from_env().with_overrides(args.llm_url.as_deref(), args.llm_model.as_deref());
    Ok(b2_llm::probed_provider(config)?)
}

/// Open a vault with the right embedder: `needs_semantic` loads the real model (failing
/// fast without it); `false` uses the fake. `B2_EMBEDDER=fake` forces the fake everywhere.
pub fn open_vault(root: &Path, needs_semantic: bool) -> Result<Vault, CliError> {
    Ok(match b2_embed::embedder_for(needs_semantic)? {
        Some(embedder) => Vault::open_with_embedder(root, Box::new(embedder))?,
        None => Vault::open(root)?,
    })
}
