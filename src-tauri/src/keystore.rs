//! keyring's credential store on Android: the client's secrets, the
//! remembered sign-in among them, in `SecureStorePlugin`, which encrypts
//! each with a key of Android's keystore. keyring has no store of its own
//! there and keeps secrets in memory, so the sign-in was gone at every start.

use std::any::Any;

use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi, CredentialPersistence};
use serde::{Deserialize, Serialize};
use tauri::plugin::PluginHandle;
use tauri::Wry;

/// Builds a credential for each keyring entry, one secret per service and user.
pub(crate) struct KeystoreCredentials(pub(crate) PluginHandle<Wry>);

impl CredentialBuilderApi for KeystoreCredentials {
    fn build(
        &self,
        _target: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<Credential>> {
        Ok(Box::new(KeystoreCredential {
            plugin: self.0.clone(),
            key: format!("{service}/{user}"),
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn persistence(&self) -> CredentialPersistence {
        CredentialPersistence::UntilDelete
    }
}

struct KeystoreCredential {
    plugin: PluginHandle<Wry>,
    key: String,
}

#[derive(Serialize)]
struct SecretArgs<'a> {
    key: &'a str,
    value: Option<&'a str>,
}

#[derive(Deserialize)]
struct Found {
    value: Option<String>,
}

#[derive(Deserialize)]
struct Deleted {
    deleted: bool,
}

fn failure(error: impl std::error::Error + Send + Sync + 'static) -> keyring::Error {
    keyring::Error::PlatformFailure(Box::new(error))
}

impl KeystoreCredential {
    fn args<'a>(&'a self, value: Option<&'a str>) -> SecretArgs<'a> {
        SecretArgs {
            key: &self.key,
            value,
        }
    }
}

impl CredentialApi for KeystoreCredential {
    /// The client keeps text only (tokens, as JSON), so the store does too.
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        let text = std::str::from_utf8(secret)
            .map_err(|_| keyring::Error::BadEncoding(secret.to_vec()))?;
        self.plugin
            .run_mobile_plugin::<()>("set", self.args(Some(text)))
            .map_err(failure)
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        let found: Found = self
            .plugin
            .run_mobile_plugin("get", self.args(None))
            .map_err(failure)?;
        found
            .value
            .map(String::into_bytes)
            .ok_or(keyring::Error::NoEntry)
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        let deleted: Deleted = self
            .plugin
            .run_mobile_plugin("delete", self.args(None))
            .map_err(failure)?;
        if deleted.deleted {
            Ok(())
        } else {
            Err(keyring::Error::NoEntry)
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
