//! Model wiring: which embedder a vault opens with, and which chat provider answers.
//! The rules themselves live in `b2-embed` and `b2-llm`, shared with the desktop; this
//! module only lays the CLI's flags over them.

use crate::args::LlmArgs;
use crate::error::CliError;
use b2_core::llm::LlmProvider;
use b2_core::vault::Vault;
use b2_llm::LlmConfig;
use std::path::Path;

/// Pick + wire the chat provider — [`open_vault`]'s sibling for the second seam.
///
/// `B2_LLM=fake` forces the deterministic fake, the `B2_EMBEDDER=fake` sibling. Otherwise
/// the real client is built from the environment with this command's flags laid over it,
/// and **probed before anything else happens** — the `b2 init` posture applied to chat: a
/// stopped daemon is a sentence before the question, never a surprise after it.
pub fn open_llm(args: &LlmArgs) -> Result<Box<dyn LlmProvider>, CliError> {
    let config =
        LlmConfig::from_env().with_overrides(args.llm_url.as_deref(), args.llm_model.as_deref());
    Ok(b2_llm::probed_provider(config)?)
}

/// Open a vault with the appropriate embedder. `needs_semantic` commands (`reindex`,
/// `search`) load the real model and **fail fast** with "run `b2 init`" if it is absent;
/// pure-graph commands pass `false` and use the fake, so no model is required just to
/// explore the graph. `B2_EMBEDDER=fake` forces the fake everywhere.
pub fn open_vault(root: &Path, needs_semantic: bool) -> Result<Vault, CliError> {
    Ok(match b2_embed::embedder_for(needs_semantic)? {
        Some(embedder) => Vault::open_with_embedder(root, Box::new(embedder))?,
        None => Vault::open(root)?,
    })
}
