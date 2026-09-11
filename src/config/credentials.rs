//! API-key lookup. Priority: OS credential store > environment variable >
//! config file. Keys are never logged.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialError {
    #[error("credential store unavailable: {0}")]
    Unavailable(String),
}

/// Service / account names under which the key is stored.
pub const SERVICE: &str = "voice_input";
pub const OPENAI_ACCOUNT: &str = "openai_api_key";
pub const OPENAI_ENV_VAR: &str = "OPENAI_API_KEY";

pub trait CredentialStore {
    fn get(&self, account: &str) -> Result<Option<String>, CredentialError>;
    fn set(&self, account: &str, secret: &str) -> Result<(), CredentialError>;
    fn delete(&self, account: &str) -> Result<(), CredentialError>;
}

/// Picks the first non-blank key in priority order. A failing store is
/// logged and skipped, never fatal.
pub fn resolve_api_key(
    store: &dyn CredentialStore,
    env: impl Fn(&str) -> Option<String>,
    config_value: Option<&str>,
) -> Option<String> {
    let non_blank = |v: Option<String>| v.filter(|s| !s.trim().is_empty());

    match store.get(OPENAI_ACCOUNT) {
        Ok(v) => {
            if let Some(key) = non_blank(v) {
                log::info!("API key: OS credential store");
                return Some(key);
            }
        }
        Err(e) => log::warn!("credential store unavailable, falling back: {e}"),
    }
    if let Some(key) = non_blank(env(OPENAI_ENV_VAR)) {
        log::info!("API key: environment variable {OPENAI_ENV_VAR}");
        return Some(key);
    }
    if let Some(key) = non_blank(config_value.map(str::to_owned)) {
        log::info!("API key: config file");
        return Some(key);
    }
    None
}

/// OS credential store (Windows Credential Manager, Secret Service on
/// Linux) via the `keyring` crate.
pub struct KeyringStore;

impl CredentialStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<String>, CredentialError> {
        let entry = keyring::Entry::new(SERVICE, account)
            .map_err(|e| CredentialError::Unavailable(e.to_string()))?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(CredentialError::Unavailable(e.to_string())),
        }
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), CredentialError> {
        keyring::Entry::new(SERVICE, account)
            .and_then(|e| e.set_password(secret))
            .map_err(|e| CredentialError::Unavailable(e.to_string()))
    }

    fn delete(&self, account: &str) -> Result<(), CredentialError> {
        match keyring::Entry::new(SERVICE, account).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(CredentialError::Unavailable(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doubles::FakeCredentialStore;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn credential_store_wins_over_env_and_config() {
        let store = FakeCredentialStore::with(OPENAI_ACCOUNT, "from-store");

        let key = resolve_api_key(&store, |_| Some("from-env".into()), Some("from-config"));

        assert_eq!(key.as_deref(), Some("from-store"));
    }

    #[test]
    fn env_var_is_used_when_store_is_empty() {
        let store = FakeCredentialStore::new();

        let key = resolve_api_key(
            &store,
            |name| (name == OPENAI_ENV_VAR).then(|| "from-env".to_owned()),
            Some("from-config"),
        );

        assert_eq!(key.as_deref(), Some("from-env"));
    }

    #[test]
    fn config_value_is_the_last_resort() {
        let store = FakeCredentialStore::new();

        let key = resolve_api_key(&store, no_env, Some("from-config"));

        assert_eq!(key.as_deref(), Some("from-config"));
    }

    #[test]
    fn blank_values_are_skipped_and_none_means_no_key() {
        let store = FakeCredentialStore::with(OPENAI_ACCOUNT, "   ");

        assert_eq!(
            resolve_api_key(&store, |_| Some(String::new()), Some(" ")),
            None
        );
        assert_eq!(resolve_api_key(&store, no_env, None), None);
    }

    #[test]
    fn a_broken_store_falls_through_instead_of_failing() {
        let store = FakeCredentialStore::broken();

        let key = resolve_api_key(&store, no_env, Some("from-config"));

        assert_eq!(key.as_deref(), Some("from-config"));
    }
}
