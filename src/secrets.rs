//! 系统凭据库（里程碑 #17）：密码优先保存在操作系统凭据库里，
//! 配置文件只保留空引用（`ServerProfile::password_in_keyring = true`）。
//!
//! - Windows：Credential Manager（advapi32 的 `CredWriteW`/`CredReadW`/
//!   `CredDeleteW`，复用既有 `windows-sys` 依赖，无需新增 crate）。
//! - 其他平台：暂未接入（Linux Secret Service 需要 DH 密钥交换 + AES 的
//!   加密协议，待依赖可用时补齐）。存取函数返回"不可用"，应用自动回退
//!   为在配置文件里保存明文密码，功能不受影响。
//!
//! 所有失败路径都只记日志、不抛错给调用方：凭据库是安全增强，绝不能
//! 因为它缺失而让播放器无法连接。

use crate::models::Config;

/// 凭据目标名：navidrome-client/<服务器作用域>。
/// 作用域复用 `config::server_scope` 的字符过滤，手工编辑过的配置里
/// 出现奇怪字符也不会污染凭据库命名空间。
fn target_name(server_id: &str) -> String {
    format!(
        "navidrome-client/{}",
        crate::config::server_scope(Some(server_id))
    )
}

/// 把密码（连同用户名）写入系统凭据库。
///
/// 同一服务器重复写入会覆盖旧条目（改密码即生效）。凭据库不可用时返回
/// Err，调用方应回退为在配置文件中保留明文。
pub fn store_password(server_id: &str, username: &str, password: &str) -> anyhow::Result<()> {
    let target = target_name(server_id);
    #[cfg(target_os = "windows")]
    {
        windows_credentials::store(&target, username, password)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (username, password);
        Err(anyhow::anyhow!(
            "no system credential store is available on this platform"
        ))
    }
}

/// 读取系统凭据库里的密码；条目缺失或平台不支持时返回 None。
/// 返回 `(用户名, 密码)`，用户名用于凭据库条目的展示与诊断。
pub fn load_password(server_id: &str) -> Option<(String, String)> {
    let target = target_name(server_id);
    #[cfg(target_os = "windows")]
    {
        windows_credentials::load(&target)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target;
        None
    }
}

/// 删除系统凭据库条目；返回是否真的删掉了（平台不支持或条目不存在为 false）。
pub fn delete_password(server_id: &str) -> bool {
    let target = target_name(server_id);
    #[cfg(target_os = "windows")]
    {
        windows_credentials::delete(&target)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target;
        false
    }
}

/// 启动时的一次性迁移：把配置里残留的明文密码逐台移入系统凭据库，
/// 成功一台就在配置里清空明文并打上标记。
///
/// 任何一台写入失败（凭据库不可用）即停止迁移并保留其余明文——
/// 绝不出现"密码既不在凭据库也不在配置文件"的丢失状态。返回迁移
/// 成功的服务器 id 供调用方决定是否保存配置。
pub fn migrate_config(config: &mut Config) -> Vec<String> {
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

/// Windows Credential Manager 的最小封装（generic credential）。
#[cfg(target_os = "windows")]
mod windows_credentials {
    use anyhow::{anyhow, Result};
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    /// BOOL 的真值（windows-sys 里 BOOL 是 i32）。
    const OK: i32 = 1;

    fn to_wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn store(target: &str, username: &str, password: &str) -> Result<()> {
        let target_wide = to_wide(target);
        let username_wide = to_wide(username);
        // 密码按 UTF-16LE 字节存入 blob，与凭据管理器的常规做法一致。
        let blob: Vec<u8> = password.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target_wide.as_ptr() as _,
            Comment: std::ptr::null_mut(),
            LastWritten: Default::default(),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_ptr() as *mut u8,
            // LOCAL_MACHINE：随用户登录可用，不随企业域漫游。
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: std::ptr::null_mut(),
            UserName: username_wide.as_ptr() as _,
        };
        // 同名条目直接覆盖，改密码即生效。
        if unsafe { CredWriteW(&credential, 0) } != OK {
            return Err(anyhow!("CredWriteW failed with Windows error {}", unsafe {
                GetLastError()
            }));
        }
        Ok(())
    }

    pub fn load(target: &str) -> Option<(String, String)> {
        let target_wide = to_wide(target);
        let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
        if unsafe { CredReadW(target_wide.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) } != OK {
            return None;
        }
        // 先拷出数据再释放，任何字段异常都按"未命中"处理。
        let result = unsafe {
            let cred = &*credential;
            let blob =
                std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize);
            let units: Vec<u16> = blob
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            let password = String::from_utf16(&units).unwrap_or_default();
            let username = if cred.UserName.is_null() {
                String::new()
            } else {
                let mut length = 0usize;
                while *cred.UserName.add(length) != 0 {
                    length += 1;
                }
                String::from_utf16_lossy(std::slice::from_raw_parts(cred.UserName, length))
            };
            CredFree(credential.cast());
            (username, password)
        };
        Some(result)
    }

    pub fn delete(target: &str) -> bool {
        let target_wide = to_wide(target);
        let deleted = unsafe { CredDeleteW(target_wide.as_ptr(), CRED_TYPE_GENERIC, 0) };
        deleted == OK
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Config, ServerProfile};

    /// 每个测试独享的服务器 id（含进程号），结束后清理凭据库条目。
    fn test_id(name: &str) -> String {
        format!("secrets-test-{}-{name}", std::process::id())
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_credential_round_trips_and_deletes() {
        let id = test_id("roundtrip");
        assert!(load_password(&id).is_none(), "unknown entries must miss");

        store_password(&id, "alice", "s3cret-密码").expect("store should succeed");
        assert_eq!(
            load_password(&id),
            Some(("alice".to_string(), "s3cret-密码".to_string()))
        );

        // 覆盖写入：改密码即生效。
        store_password(&id, "alice", "new-pass").expect("overwrite should succeed");
        assert_eq!(
            load_password(&id),
            Some(("alice".to_string(), "new-pass".to_string()))
        );

        assert!(delete_password(&id));
        assert!(load_password(&id).is_none());
        // 再删为 false；未命中读取仍是 None。
        assert!(!delete_password(&id));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn migrate_config_moves_plaintext_passwords_into_the_credential_store() {
        let id = test_id("migrate");
        let mut config = Config {
            servers: vec![ServerProfile {
                id: id.clone(),
                url: "http://nas.local:4533".to_string(),
                username: "alice".to_string(),
                password: "plain-secret".to_string(),
                ..ServerProfile::default()
            }],
            ..Config::default()
        };

        let migrated = migrate_config(&mut config);
        assert_eq!(migrated, vec![id.clone()]);
        let profile = &config.servers[0];
        assert!(profile.password_in_keyring);
        assert!(profile.password.is_empty(), "明文必须被清空");
        assert_eq!(
            load_password(&id).as_ref().map(|(_, p)| p.clone()),
            Some("plain-secret".to_string())
        );
        // 镜像字段同步为空。
        assert!(config.password.is_empty());

        // 已迁移的条目在下一轮启动是 no-op。
        assert!(migrate_config(&mut config).is_empty());

        delete_password(&id);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn migrate_config_keeps_plaintext_when_the_store_is_unavailable() {
        let mut config = Config {
            servers: vec![ServerProfile {
                id: "secrets-test-fallback".to_string(),
                url: "http://nas.local:4533".to_string(),
                username: "alice".to_string(),
                password: "plain-secret".to_string(),
                ..ServerProfile::default()
            }],
            ..Config::default()
        };

        assert!(migrate_config(&mut config).is_empty());
        let profile = &config.servers[0];
        assert!(!profile.password_in_keyring);
        assert_eq!(profile.password, "plain-secret");
    }
}
