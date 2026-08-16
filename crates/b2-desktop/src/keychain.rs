//! Where a cloud bearer token is remembered between launches — the macOS
//! Keychain (GH #176).
//!
//! Chat's **Cloud models** configuration (invariant M5) needs a bearer token, and
//! for one release this host held it in memory for the session only: nothing was
//! written down, and `B2_LLM_API_KEY` was the only way to make one persist. That
//! was the conservative choice and it had a real cost — a desktop user who
//! configures a cloud provider and finds the key gone after a restart reads it as
//! a bug, and "export a variable in your shell" is a terminal instruction inside a
//! GUI. #176 settled it: **remember the key, in the platform's own encrypted
//! store, and let the environment override it.**
//!
//! Three things decided the shape:
//!
//! * **The Keychain, not a file.** Storing a secret is taking responsibility for
//!   it. A plaintext JSON beside `chat.json` is readable by every process running
//!   as the user, rides into backups and sync, and outlives an uninstall; the
//!   Keychain is encrypted at rest, ACL'd per application, and its access prompt
//!   is one the user already recognizes. B2 ships macOS-only, so the
//!   platform-specific answer costs nothing in portability today.
//! * **The framework, not `/usr/bin/security`.** Shelling out would put the token
//!   in `add-generic-password -w <key>` — in the `security` process's argv, and so
//!   in `ps`. That is the exact argument #154 makes against a key in a CLI flag,
//!   and it would have been funny to lose to it here.
//! * **It must never be able to break chat.** The Keychain can refuse — locked,
//!   the user declines the prompt, no keychain at all in a headless run — and the
//!   whole point of remembering a key is a convenience. So every operation here is
//!   best-effort and *says* what happened rather than raising: a refused save
//!   degrades to the session-only behavior that shipped in #155, with the key the
//!   user typed still in force for this run and the Settings copy saying so
//!   (`b2_llm::ApiKeySource::Session`).
//!
//! One item, not one per endpoint. A user repointing `base_url` at a different
//! provider overwrites the key rather than accumulating a drawer of them — which
//! is also the safer default, since the hazard a per-endpoint store would create
//! is precisely the one `chat.rs`'s Remove button exists to prevent (sending
//! provider A's token to provider B).
//!
//! Note for anyone running `cargo tauri dev`: the item's ACL names the binary that
//! created it, so a rebuilt (and unsigned) `b2-desktop` is a *different*
//! application to the Keychain and macOS will ask again. That is the platform
//! working, not a bug here — "Always Allow" applies to the binary you granted it
//! to.

/// A place the host can keep one secret between launches.
///
/// A trait for one concrete reason (the repo's rule against speculative
/// abstraction stands): **a unit test must not touch the developer's real
/// Keychain**, so `chat.rs`'s key resolution and its three-state save are
/// exercised against `MemoryStore` below. This is host infrastructure, not one of the
/// enumerated model seams (invariant M1).
pub trait KeyStore {
    /// The remembered key, or `None` when there is none — *or* when the store
    /// refused. The two are deliberately one answer: a caller whose fallback is
    /// "no stored key" does the same thing either way, and the difference is
    /// logged rather than branched on.
    fn load(&self) -> Option<String>;

    /// Remember `key`, replacing whatever was there. `false` means the store
    /// refused, which is the caller's cue to keep the key **for this session
    /// only** rather than to fail the save.
    fn save(&self, key: &str) -> bool;

    /// Forget the remembered key. `true` when the store no longer holds one —
    /// including when it never did, which is the outcome this asks for and not a
    /// failure.
    ///
    /// The return value is load-bearing, and an earlier draft of this trait got
    /// it wrong by making removal infallible. A refused delete leaves the item in
    /// the Keychain; if the caller drops the key from memory anyway, Settings
    /// shows a keyless configuration, the user believes their credential is gone
    /// — and the **next launch reads it straight back out of the store**. A
    /// secret that returns from the dead after you deleted it is the worst
    /// failure this module could have, so removal is all-or-nothing: `false` here
    /// means the caller must keep the key exactly as it was and let the UI go on
    /// showing it (`chat::apply_key`).
    fn clear(&self) -> bool;
}

/// The macOS Keychain, and the only [`KeyStore`] the shipped app constructs.
///
/// A unit struct: the item it addresses is a constant, so there is no state to
/// hold and no handle to keep open — which is also why `Debug` is safe to derive
/// here and pointedly *not* on [`MemoryStore`] below.
#[derive(Debug)]
pub struct Keychain;

/// The service half of the generic-password item's identity. The app's bundle
/// identifier (`tauri.conf.json`), so the item is unambiguously B2's in a
/// keychain full of other applications'. Both halves are `cfg`'d with the only
/// implementation that addresses an item — on any other platform they would name
/// nothing, and `-D warnings` is right to say so.
#[cfg(target_os = "macos")]
pub const SERVICE: &str = "dev.b2.desktop";

/// The account half. Named for what it is rather than for a user or an endpoint —
/// there is exactly one such item (see the module header).
#[cfg(target_os = "macos")]
pub const ACCOUNT: &str = "chat-api-key";

#[cfg(target_os = "macos")]
mod platform {
    use super::{KeyStore, Keychain, ACCOUNT, SERVICE};
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };

    /// `errSecItemNotFound`, from Apple's `SecBase.h`. Spelled here rather than
    /// pulled from `security-framework-sys` to keep this corner at one
    /// dependency; the value is part of a stable public API and has been since
    /// OS X 10.6.
    const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

    impl KeyStore for Keychain {
        fn load(&self) -> Option<String> {
            match get_generic_password(SERVICE, ACCOUNT) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(key) => Some(key.trim().to_string()).filter(|k| !k.is_empty()),
                    Err(_) => {
                        // Something else wrote a non-UTF-8 item under our name.
                        // Not ours to interpret, and not worth failing over.
                        eprintln!("[b2] ignoring an unreadable API key in the Keychain");
                        None
                    }
                },
                // No item is the overwhelmingly common case — every user who has
                // never configured a cloud model — and it is silent, and it
                // prompts for nothing.
                Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => None,
                Err(e) => {
                    eprintln!("[b2] could not read the API key from the Keychain: {e}");
                    None
                }
            }
        }

        fn save(&self, key: &str) -> bool {
            match set_generic_password(SERVICE, ACCOUNT, key.as_bytes()) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("[b2] could not save the API key to the Keychain: {e}");
                    false
                }
            }
        }

        fn clear(&self) -> bool {
            match delete_generic_password(SERVICE, ACCOUNT) {
                Ok(()) => true,
                // Nothing stored is the outcome this asks for, not a failure.
                Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => true,
                Err(e) => {
                    eprintln!("[b2] could not remove the API key from the Keychain: {e}");
                    false
                }
            }
        }
    }
}

/// Everywhere else there is no store, and the host behaves exactly as it did
/// before #176: the key lives for the session and `B2_LLM_API_KEY` is how one
/// persists. B2 ships macOS-only, so this is the compile-anywhere path rather
/// than a supported configuration — and it is the *same* path a refusing Keychain
/// takes, which is why it needs no separate handling upstream.
#[cfg(not(target_os = "macos"))]
mod platform {
    use super::{KeyStore, Keychain};

    impl KeyStore for Keychain {
        fn load(&self) -> Option<String> {
            None
        }

        fn save(&self, _key: &str) -> bool {
            false
        }

        /// Trivially true: a store that never remembers anything is always
        /// already in the state `clear` asks for.
        fn clear(&self) -> bool {
            true
        }
    }
}

/// An in-memory [`KeyStore`] — the Keychain's test double, and the reason
/// [`KeyStore`] is a trait at all. Lives here rather than in `chat.rs` because
/// both that module's tests and `commands.rs`'s need it.
#[cfg(test)]
pub struct MemoryStore {
    key: std::sync::Mutex<Option<String>>,
    /// Whether [`save`](KeyStore::save) refuses — the Keychain saying no, which
    /// the host must degrade past rather than fail on.
    refuses: bool,
}

#[cfg(test)]
impl MemoryStore {
    /// An empty, working store.
    pub fn empty() -> Self {
        Self {
            key: std::sync::Mutex::new(None),
            refuses: false,
        }
    }

    /// A store holding `key` already — a launch that finds a remembered key.
    pub fn holding(key: &str) -> Self {
        Self {
            key: std::sync::Mutex::new(Some(key.to_string())),
            refuses: false,
        }
    }

    /// A store that refuses every write: a locked Keychain, or a user who
    /// declined the prompt.
    pub fn refusing() -> Self {
        Self {
            key: std::sync::Mutex::new(None),
            refuses: true,
        }
    }

    /// A store that already holds `key` *and* refuses to give it up — the
    /// failed-removal case, which is the one that must never read as success.
    pub fn refusing_holding(key: &str) -> Self {
        Self {
            key: std::sync::Mutex::new(Some(key.to_string())),
            refuses: true,
        }
    }

    /// What the store is holding — the assertion a test actually wants to make.
    pub fn peek(&self) -> Option<String> {
        self.key.lock().expect("test store lock").clone()
    }
}

#[cfg(test)]
impl KeyStore for MemoryStore {
    fn load(&self) -> Option<String> {
        self.peek()
    }

    fn save(&self, key: &str) -> bool {
        if self.refuses {
            return false;
        }
        *self.key.lock().expect("test store lock") = Some(key.to_string());
        true
    }

    fn clear(&self) -> bool {
        let mut held = self.key.lock().expect("test store lock");
        // Nothing to remove succeeds even under refusal — the real Keychain
        // answers `errSecItemNotFound` here, which `Keychain::clear` reads as
        // "already in the asked-for state".
        if held.is_none() {
            return true;
        }
        if self.refuses {
            return false;
        }
        *held = None;
        true
    }
}
