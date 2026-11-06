//! Ordered credential migration and config updates, independent of OS APIs.

use crate::models::Config;

pub(super) fn migrate_config(
    config: &mut Config,
    mut store_password: impl FnMut(&str, &str, &str) -> anyhow::Result<()>,
) -> Vec<String> {
    let mut migrated = Vec::new();
    for profile in &mut config.servers {
        if profile.password_in_keyring || profile.password.is_empty() {
            continue;
        }
        match store_password(&profile.id, &profile.username, &profile.password) {
            Ok(()) => {
                profile.password.clear();
                profile.password_in_keyring = true;
                migrated.push(profile.id.clone());
            }
            Err(error) => {
                // 凭据库整体不可用：保留明文，之后每轮启动重试。
                log::warn!(
                    "system credential store unavailable; keeping passwords in the config file: {error:#}"
                );
                break;
            }
        }
    }
    if !migrated.is_empty() {
        config.sync_active_profile();
    }
    migrated
}

#[cfg(test)]
mod tests;
