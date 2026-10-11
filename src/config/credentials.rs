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
pub const GROQ_ACCOUNT: &str = "groq_api_key";
pub const GROQ_ENV_VAR: &str = "GROQ_API_KEY";
pub const XAI_ACCOUNT: &str = "xai_api_key";
pub const XAI_ENV_VAR: &str = "XAI_API_KEY";

/// Where one provider's key lives: credential-store account + env var.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySource {
    pub account: &'static str,
    pub env_var: &'static str,
}

pub const OPENAI_KEY: KeySource = KeySource {
    account: OPENAI_ACCOUNT,
    env_var: OPENAI_ENV_VAR,
};
pub const GROQ_KEY: KeySource = KeySource {
    account: GROQ_ACCOUNT,
    env_var: GROQ_ENV_VAR,
};
pub const XAI_KEY: KeySource = KeySource {
    account: XAI_ACCOUNT,
    env_var: XAI_ENV_VAR,
};

impl KeySource {
    /// The key source for a `provider` name from config.toml.
    pub fn for_provider(provider: &str) -> Option<Self> {
        match provider {
            "openai" => Some(OPENAI_KEY),
            "groq" => Some(GROQ_KEY),
            "xai" => Some(XAI_KEY),
            _ => None,
        }
    }
}

pub trait CredentialStore {
    fn get(&self, account: &str) -> Result<Option<String>, CredentialError>;
    fn set(&self, account: &str, secret: &str) -> Result<(), CredentialError>;
    fn delete(&self, account: &str) -> Result<(), CredentialError>;
}

/// Picks the first non-blank key in priority order. A failing store is
/// logged and skipped, never fatal.
pub fn resolve_api_key(
    store: &dyn CredentialStore,
    source: KeySource,
    env: impl Fn(&str) -> Option<String>,
    config_value: Option<&str>,
) -> Option<String> {
    let non_blank = |v: Option<String>| v.filter(|s| !s.trim().is_empty());

    match store.get(source.account) {
        Ok(v) => {
            if let Some(key) = non_blank(v) {
                log::info!("API key: OS credential store");
                return Some(key);
            }
        }
        Err(e) => log::warn!("credential store unavailable, falling back: {e}"),
    }
    if let Some(key) = non_blank(env(source.env_var)) {
        log::info!("API key: environment variable {}", source.env_var);
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

        let key = resolve_api_key(
            &store,
            OPENAI_KEY,
            |_| Some("from-env".into()),
            Some("from-config"),
        );

        assert_eq!(key.as_deref(), Some("from-store"));
    }

    #[test]
    fn env_var_is_used_when_store_is_empty() {
        let store = FakeCredentialStore::new();

        let key = resolve_api_key(
            &store,
            OPENAI_KEY,
            |name| (name == OPENAI_ENV_VAR).then(|| "from-env".to_owned()),
            Some("from-config"),
        );

        assert_eq!(key.as_deref(), Some("from-env"));
    }

    #[test]
    fn config_value_is_the_last_resort() {
        let store = FakeCredentialStore::new();

        let key = resolve_api_key(&store, OPENAI_KEY, no_env, Some("from-config"));

        assert_eq!(key.as_deref(), Some("from-config"));
    }

    #[test]
    fn blank_values_are_skipped_and_none_means_no_key() {
        let store = FakeCredentialStore::with(OPENAI_ACCOUNT, "   ");

        assert_eq!(
            resolve_api_key(&store, OPENAI_KEY, |_| Some(String::new()), Some(" ")),
            None
        );
        assert_eq!(resolve_api_key(&store, OPENAI_KEY, no_env, None), None);
    }

    #[test]
    fn each_provider_reads_only_its_own_account_and_env_var() {
        let store = FakeCredentialStore::with(OPENAI_ACCOUNT, "openai-key");

        assert_eq!(resolve_api_key(&store, XAI_KEY, no_env, None), None);
        let key = resolve_api_key(
            &store,
            XAI_KEY,
            |name| (name == XAI_ENV_VAR).then(|| "xai-key".to_owned()),
            None,
        );
        assert_eq!(key.as_deref(), Some("xai-key"));
    }

    #[test]
    fn provider_names_map_to_key_sources() {
        assert_eq!(KeySource::for_provider("openai"), Some(OPENAI_KEY));
        assert_eq!(KeySource::for_provider("groq"), Some(GROQ_KEY));
        assert_eq!(KeySource::for_provider("xai"), Some(XAI_KEY));
        assert_eq!(KeySource::for_provider("nope"), None);
    }

    #[test]
    fn a_broken_store_falls_through_instead_of_failing() {
        let store = FakeCredentialStore::broken();

        let key = resolve_api_key(&store, OPENAI_KEY, no_env, Some("from-config"));

        assert_eq!(key.as_deref(), Some("from-config"));
    }
}
