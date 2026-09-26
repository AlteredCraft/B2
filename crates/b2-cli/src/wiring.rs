//! Model wiring: which embedder a vault opens with, and which chat provider answers.

use crate::args::LlmArgs;
use crate::error::CliError;
use b2_core::embed::Embedder;
use b2_core::llm::{FakeLlm, LlmProvider};
use b2_core::vault::Vault;
use b2_embed::{EmbedConfig, LocalEmbedder};
use b2_llm::{LlmConfig, OpenAiCompatProvider};
use std::path::Path;

/// Pick + wire the chat provider — [`open_vault`]'s sibling for the second seam.
///
/// `B2_LLM=fake` forces the deterministic [`FakeLlm`], the `B2_EMBEDDER=fake` sibling.
/// Otherwise the real client is built from the environment with this command's flags laid
/// over it, and **probed before anything else happens** — the `b2 init` posture applied to
/// chat: a stopped daemon is a sentence before the question, never a surprise after it.
pub fn open_llm(args: &LlmArgs) -> Result<Box<dyn LlmProvider>, CliError> {
    if use_fake_llm() {
        return Ok(Box::new(FakeLlm));
    }
    let config =
        LlmConfig::from_env().with_overrides(args.llm_url.as_deref(), args.llm_model.as_deref());
    let provider = OpenAiCompatProvider::new(config);
    provider.probe()?;
    Ok(Box::new(provider))
}

pub fn use_fake_llm() -> bool {
    std::env::var_os("B2_LLM").is_some_and(|v| v == "fake")
}

/// Open a vault with the appropriate embedder. `needs_semantic` commands (`reindex`,
/// `search`) load the real [`LocalEmbedder`] and **fail fast** with "run `b2 init`" if it
/// is absent; pure-graph commands pass `false` and use the fake, so no model is required
/// just to explore the graph. `B2_EMBEDDER=fake` forces the fake everywhere.
pub fn open_vault(root: &Path, needs_semantic: bool) -> Result<Vault, CliError> {
    if needs_semantic && !use_fake_embedder() {
        let config = EmbedConfig::load()?;
        let embedder = LocalEmbedder::load(&config)?;
        Ok(Vault::open_with_embedder(
            root,
            Box::new(embedder) as Box<dyn Embedder>,
        )?)
    } else {
        Ok(Vault::open(root)?)
    }
}

pub fn use_fake_embedder() -> bool {
    std::env::var_os("B2_EMBEDDER").is_some_and(|v| v == "fake")
}
