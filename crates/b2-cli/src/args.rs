//! The command line itself: the clap definitions, the agent help text, and the rule
//! for which vault root a command runs against.

use crate::error::CliError;
use clap::{Args, Parser, Subcommand};
use std::path::{Path, PathBuf};

/// The retrieval loop, stated where an agent will actually read it (`b2 --help`) —
/// the toolset carrying its own usage instructions, the way an MCP server ships its
/// tool descriptions. The loop itself lives in the *caller*: B2's part is dumb,
/// composable reads (ADR-0012), and the vault being plain Markdown on disk is what
/// makes "open the file yourself" a step no B2 command needs to exist for.
pub const AGENT_HELP: &str = "\
For agents (--json):
  Every command takes --json, and the vault is plain Markdown on disk — a result's
  `path` is a real file, readable and greppable directly. The retrieval loop:

  1. b2 search \"<query>\" --json   Hybrid keyword+semantic search: the ranked rows
                                  plus an evidence verdict. `\"vouched\": false`
                                  means the vault holds no evidence for this query
                                  — report \"no matches\" instead of using the rows.
  2. Read the files it names      Paths are vault-relative; open them for the full
                                  note, not just the matched snippet.
  3. b2 neighbors/explain <note>  Follow the typed graph around a promising note;
                                  b2 similar <note> ranks unlinked semantic
                                  neighbors.
  4. Refine and search again      Pass --exclude <path> (repeatable) for notes
                                  already inspected, so follow-ups surface fresh
                                  material. Fewer results than asked = the head of
                                  the ranking is spent; refine the query.";

#[derive(Parser)]
#[command(
    name = "b2",
    version,
    about = "B2 — explore a Markdown vault's typed graph and search from the terminal",
    after_long_help = AGENT_HELP
)]
pub struct Cli {
    /// Vault root (the folder of Markdown). The index lives in `<vault>/.b2/`.
    /// Set it with `-C <path>` or `$B2_VAULT_PATH` (the flag wins). Read-only commands
    /// fall back to the current dir; commands that write (`reindex`/`add`/`write`/`mv`/
    /// `rm`/`link`) require it explicitly, so they can never silently touch the wrong
    /// directory.
    #[arg(short = 'C', long = "vault", global = true, env = "B2_VAULT_PATH")]
    pub vault: Option<PathBuf>,

    /// Emit machine-readable JSON instead of human-readable text.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Download + verify the embedding model into the shared cache (one-time setup).
    Init,
    /// Re-project every note under the vault into the index. Reads your vault and
    /// writes only to `.b2/`. Incremental by default: notes whose content is
    /// unchanged keep their vectors.
    Reindex {
        /// Vault root (overrides --vault / $B2_VAULT_PATH). Required — no cwd default.
        vault: Option<PathBuf>,
        /// Re-embed every note, even unchanged ones (a full rebuild in place).
        #[arg(long)]
        force: bool,
        /// Preview what a reindex would do without touching the index — how many
        /// notes it would project and embed.
        #[arg(long)]
        dry_run: bool,
        /// Stop a reindex already running on this vault — the way to reach one
        /// backgrounded with `b2 reindex &`, which has no controlling terminal for
        /// Ctrl-C. It stops after the current batch, leaving the same consistent,
        /// re-runnable partial index a foreground Ctrl-C does.
        #[arg(long, conflicts_with_all = ["force", "dry_run"])]
        cancel: bool,
    },
    /// Report embedding coverage — how many notes are embedded (so semantic ranking
    /// is live vs. keyword-only) and whether a reindex is currently running, with the
    /// process id to stop it by. A pure, model-free read; handy after kicking off a
    /// slow reindex in the background with `b2 reindex &`.
    Status,
    /// Create a new note and project it into the index (it's immediately in the
    /// graph + searchable). PATH is vault-relative (the `.md` extension is optional).
    Add {
        /// Where the new note goes: a vault-relative path (`.md` optional).
        path: String,
        /// The note's human title (frontmatter `title:`).
        #[arg(long)]
        title: Option<String>,
        /// Initial body content (Markdown). Omit for an empty note to fill in later.
        #[arg(long)]
        content: Option<String>,
    },
    /// Replace a note's **body** with Markdown piped on stdin — the CLI/agent editing
    /// surface (the counterpart to the desktop editor's save). NOTE is a vault-relative
    /// path; the note must already exist (`b2 add` creates one). Pipe the body
    /// **alone** — the frontmatter (including B2's managed `b2_relations:`) is
    /// left untouched, so no `---` block. The note is re-projected (chunks + FTS + graph)
    /// so the index stays live; a later `b2 reindex` fills the changed body's vectors.
    Write {
        /// The note whose body to overwrite: a vault-relative path.
        note: String,
    },
    /// Show a note's typed neighbors. NOTE is a vault-relative path.
    Neighbors { note: String },
    /// Explain a note's connections — every typed edge and its "why". NOTE is a
    /// vault-relative path.
    Explain { note: String },
    /// Move/rename a note, file, or folder and rewrite every inbound link to
    /// point at the new path. An existing directory moves as a whole folder
    /// (everything under it, one rename).
    Mv {
        /// What to move: a vault-relative path (note, file, or folder).
        from: String,
        /// The new vault-relative path (for a note, the `.md` extension is optional).
        to: String,
    },
    /// Delete a note, file, or folder from the vault *and* the disk. Inbound links
    /// are never rewritten — they dangle and surface as unresolved (`b2 explain`).
    /// An existing directory deletes as a whole folder and requires --recursive.
    Rm {
        /// What to delete: a vault-relative path (note, file, or folder).
        target: String,
        /// Required to delete a folder (and everything inside it, unindexed files too).
        #[arg(short = 'r', long)]
        recursive: bool,
    },
    /// Hybrid keyword+semantic search across the vault. A query the vault
    /// holds no evidence for answers "no matches" rather than serving its nearest
    /// rows (invariants.md D2); `--json` is an **object** — the rows plus that
    /// verdict, which has nowhere to live in a bare list.
    Search {
        query: String,
        /// Maximum number of notes to return.
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Leave a note out of the results: a vault-relative path exactly as a
        /// previous search served it (repeatable). The follow-up-search flag for
        /// agent loops — pass the notes already inspected so a re-query surfaces
        /// fresh material instead of the same head. It subtracts rows only: the
        /// evidence verdict still reads the whole vault, and a heavily-excluded
        /// query may serve fewer than LIMIT notes — the cue to refine the query
        /// rather than page deeper.
        #[arg(long, value_name = "PATH")]
        exclude: Vec<String>,
    },
    /// Surface the notes most semantically similar to NOTE that you haven't linked
    /// yet — connection discovery, ranked nearest first. NOTE is a vault-relative
    /// path. A local read over stored vectors (run `b2 reindex` with the real
    /// model first). Similarity is relative to your vault: the list is always
    /// the ranked nearest, and you are the judge of which are worth a link.
    Similar {
        /// The note to find similar notes for: a vault-relative path.
        note: String,
        /// Maximum number of similar notes to return.
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Explain one note against NOTE's list instead of printing the list: where it
        /// stands (and why, if it is not a card), and the passage pairs behind it. No
        /// model call. Read at `--limit`, so the rank is the one the list shows.
        #[arg(long, value_name = "OTHER")]
        explain: Option<String>,
    },
    /// Commit a typed connection SRC → DST into SRC's frontmatter `b2_relations:`.
    /// SRC and DST are each a vault-relative path.
    Link {
        /// The source note (the edge points *from* it): a vault-relative path.
        src: String,
        /// The target note (the edge points *to* it): a vault-relative path.
        dst: String,
        /// The relation verb (a core verb: references/supports/contradicts).
        #[arg(long = "type", default_value = "references")]
        edge_type: String,
        /// Optional explanation — trailing text shown after the link.
        #[arg(long)]
        explanation: Option<String>,
    },
    /// Ask one question about your vault and stream a grounded answer. The model
    /// answers **only** from passages retrieved out of your notes and cites them by
    /// `[n]`; `--json` emits the answer as a JSON Lines event stream (one token
    /// event per token, then the final answer with its citations resolved).
    /// Needs a model server — Ollama by default (see `--llm-url`).
    Ask {
        /// The question, in plain language.
        question: String,
        #[command(flatten)]
        llm: LlmArgs,
    },
    /// Explain why CANDIDATE shows up in `b2 similar NOTE` — a grounded, cited answer
    /// streamed from the chat model, which is given B2's read-only tools (the matched
    /// passages, the similar list, a note's links, reading a note) and looks up what it
    /// needs. The answer lists the tools that ran. A model with no tool support is handed
    /// the matched passages instead. `--json` streams events as `ask` does.
    /// Needs a model server — Ollama by default (see `--llm-url`).
    Why {
        /// The note the list was for: a vault-relative path.
        note: String,
        /// The suggested note to explain: a vault-relative path.
        candidate: String,
        /// The length of the `similar` list the rank is quoted against.
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[command(flatten)]
        llm: LlmArgs,
    },
    /// Interactive grounded chat about your vault — the same answers as `ask`, with
    /// follow-up questions that remember the conversation. Ctrl-C stops an answer
    /// mid-stream (the partial text stands); `/exit` or Ctrl-D leaves. History is
    /// session-only: nothing about a chat is ever written to your notes or the index.
    Chat {
        #[command(flatten)]
        llm: LlmArgs,
    },
}

/// Which model to chat with, and where it lives — shared by `ask` and `chat`. Precedence
/// is the `B2_VAULT_PATH` convention exactly: an explicit flag beats the environment beats
/// the default. Resolution lives once in `b2_llm::LlmConfig`, so the desktop's settings
/// layer over the same base — which is why these are plain options, not clap `env` args.
#[derive(Args, Debug)]
pub struct LlmArgs {
    /// The OpenAI-compatible base URL of your model server
    /// [env: B2_LLM_URL, default: http://localhost:11434/v1 — Ollama's].
    /// Any compatible endpoint works: LM Studio, llama.cpp, vLLM, or a cloud
    /// provider's (which sends your question and the retrieved passages to them —
    /// pair it with B2_LLM_API_KEY).
    #[arg(long = "llm-url", value_name = "URL")]
    pub llm_url: Option<String>,
    /// The chat model id, as the server names it
    /// [env: B2_LLM_MODEL, default: llama3.2]. Pull it first, e.g. `ollama pull
    /// llama3.2`.
    #[arg(long = "llm-model", value_name = "MODEL")]
    pub llm_model: Option<String>,
}

impl Cli {
    /// The vault root for **read-only** commands (`search`, `neighbors`, `explain`,
    /// `similar`): the `-C`/`$B2_VAULT_PATH` value if given, else the current directory.
    /// A pure read can't pollute anything, so the cwd convenience is safe here.
    pub fn vault_or_cwd(&self) -> &Path {
        self.vault.as_deref().unwrap_or_else(|| Path::new("."))
    }

    /// The vault root for commands that **write** (`reindex`, `add`, `write`, `mv`, `rm`,
    /// `link`): `positional` wins, then `-C`/`$B2_VAULT_PATH`; with none, error rather than
    /// silently mutating the current directory, where a stale binary or mistyped var would
    /// otherwise leave a stray `.b2/`. The write-side counterpart to
    /// [`vault_or_cwd`](Self::vault_or_cwd).
    pub fn require_vault<'a>(&'a self, positional: Option<&'a Path>) -> Result<&'a Path, CliError> {
        positional
            .or(self.vault.as_deref())
            .ok_or(CliError::VaultRequired)
    }
}
