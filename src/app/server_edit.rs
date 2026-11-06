//! Server form normalization, reconnect decisions, and credential-storage results.

use crate::models::{Config, ServerProfile};

pub(super) struct ServerForm {
    url: String,
    username: String,
    password: String,
    name: String,
}

#[derive(Clone, Copy)]
pub(super) enum PasswordStorage {
    Keyring,
    Plaintext,
    Empty,
}

pub(super) struct ServerEdit {
    config: Config,
    stored_id: String,
    reconnect: bool,
    delete_existing_password: bool,
}

impl ServerForm {
    pub(super) fn new(url: &str, username: &str, password: &str, name: &str) -> Self {
        Self {
            url: url.trim().to_string(),
            username: username.trim().to_string(),
            password: password.to_string(),
            name: name.trim().to_string(),
        }
    }

    fn identity_changed(&self, target: &ServerProfile, resolved_password: &str) -> bool {
        target.url.trim() != self.url
            || target.username.trim() != self.username
            || resolved_password != self.password
    }

    pub(super) fn requires_reconnect(
        &self,
        config: &Config,
        target: &ServerProfile,
        resolved_password: &str,
    ) -> bool {
        config.active_server.as_deref() == Some(target.id.as_str())
            && (self.identity_changed(target, resolved_password) || !target.is_configured())
    }

    pub(super) fn prepare(
        self,
        config: &Config,
        target_id: &str,
        resolved_password: &str,
    ) -> Option<ServerEdit> {
        let target = config
            .servers
            .iter()
            .find(|profile| profile.id == target_id)?;
        let reconnect = self.requires_reconnect(config, target, resolved_password);
        let mut updated = config.clone();
        // A name-only edit must retain its ID even if an equivalent canonical
        // profile exists; active connection edits retain the established merge behavior.
        let stored_id = if config.active_server.as_deref() == Some(target_id)
            && self.identity_changed(target, resolved_password)
        {
            updated.store_active(&self.url, &self.username, &self.password)
        } else {
            updated.store_profile(target_id, &self.url, &self.username, &self.password);
            target_id.to_string()
        };
        let delete_existing_password = self.password.is_empty()
            && config
                .servers
                .iter()
                .any(|profile| profile.id == stored_id && profile.password_in_keyring);
        updated
            .servers
            .iter_mut()
            .find(|profile| profile.id == stored_id)?
            .name = self.name;
        Some(ServerEdit {
            config: updated,
            stored_id,
            reconnect,
            delete_existing_password,
        })
    }
}

impl ServerEdit {
    pub(super) fn profile(&self) -> &ServerProfile {
        self.config
            .servers
            .iter()
            .find(|profile| profile.id == self.stored_id)
            .expect("prepared edit retains its destination profile")
    }

    pub(super) fn requires_reconnect(&self) -> bool {
        self.reconnect
    }

    pub(super) fn delete_existing_password(&self) -> bool {
        self.delete_existing_password
    }

    pub(super) fn finish(mut self, storage: PasswordStorage) -> Config {
        let profile = self
            .config
            .servers
            .iter_mut()
            .find(|profile| profile.id == self.stored_id)
            .expect("prepared edit retains its destination profile");
        match storage {
            PasswordStorage::Keyring => {
                profile.password.clear();
                profile.password_in_keyring = true;
            }
            PasswordStorage::Plaintext => profile.password_in_keyring = false,
            PasswordStorage::Empty => {
                profile.password.clear();
                profile.password_in_keyring = false;
            }
        }
        self.config.sync_active_profile();
        self.config
    }
}

#[cfg(test)]
mod tests;
