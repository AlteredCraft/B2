//! The cloud API key, remembered between launches in the macOS Keychain (GH #176), not a
//! plaintext file. The Keychain can refuse (locked, prompt declined), so every operation is
//! best-effort and reports what happened: a refused save degrades to session-only.
//!
//! One item, not one per endpoint. Under `cargo tauri dev` a rebuilt unsigned binary is a
//! new application to the Keychain, so macOS asks again.

/// A place the host can keep one secret between launches. A trait so unit tests never
/// touch the real Keychain; not one of the model seams (ADR-0005).
pub trait KeyStore {
    /// The remembered key, or `None` when there is none or the store refused (logged).
    fn load(&self) -> Option<String>;

    /// Remember `key`. `false` means the store refused: keep the key for this session.
    fn save(&self, key: &str) -> bool;

    /// Forget the remembered key. `true` when the store no longer holds one, including
    /// when it never did. On `false` the caller must keep the key as it was: a refused
    /// delete leaves the item, and the next launch would read it back.
    fn clear(&self) -> bool;
}

/// The macOS Keychain, the only [`KeyStore`] the shipped app constructs. Holds no key, so
/// `Debug` is safe here (unlike [`MemoryStore`]).
#[derive(Debug)]
pub struct Keychain;

/// The item's service: the app's bundle identifier (`tauri.conf.json`).
#[cfg(target_os = "macos")]
pub const SERVICE: &str = "dev.b2.desktop";

/// The item's account; there is exactly one item.
#[cfg(target_os = "macos")]
pub const ACCOUNT: &str = "chat-api-key";

#[cfg(target_os = "macos")]
mod platform {
    use super::{KeyStore, Keychain, ACCOUNT, SERVICE};
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };

    /// `errSecItemNotFound` (`SecBase.h`), a stable value, spelled here to avoid a
    /// dependency on `security-framework-sys`.
    const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

    impl KeyStore for Keychain {
        fn load(&self) -> Option<String> {
            match get_generic_password(SERVICE, ACCOUNT) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(key) => Some(key.trim().to_string()).filter(|k| !k.is_empty()),
                    Err(_) => {
                        eprintln!("[b2] ignoring an unreadable API key in the Keychain");
                        None
                    }
                },
                // The common case (no cloud model configured): silent.
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

/// Elsewhere there is no store: the key lives for the session, as with a refusing
/// Keychain, so upstream needs no separate handling.
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

        fn clear(&self) -> bool {
            true
        }
    }
}

/// An in-memory [`KeyStore`], the Keychain's test double for `chat.rs` and `commands.rs`.
#[cfg(test)]
pub struct MemoryStore {
    key: std::sync::Mutex<Option<String>>,
    /// Whether writes refuse, as a locked Keychain would.
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

    /// A store already holding `key`.
    pub fn holding(key: &str) -> Self {
        Self {
            key: std::sync::Mutex::new(Some(key.to_string())),
            refuses: false,
        }
    }

    /// A store that refuses every write.
    pub fn refusing() -> Self {
        Self {
            key: std::sync::Mutex::new(None),
            refuses: true,
        }
    }

    /// A store holding `key` that refuses to give it up: the failed-removal case.
    pub fn refusing_holding(key: &str) -> Self {
        Self {
            key: std::sync::Mutex::new(Some(key.to_string())),
            refuses: true,
        }
    }

    /// What the store is holding.
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
        // Nothing to remove succeeds even under refusal, as `errSecItemNotFound` does.
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
