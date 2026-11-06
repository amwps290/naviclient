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

mod migration;
#[cfg(target_os = "windows")]
mod windows_credentials;

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
    #[cfg(target_os = "windows")]
    {
        let target = target_name(server_id);
        windows_credentials::store(&target, username, password)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (server_id, username, password);
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
    migration::migrate_config(config, store_password)
}

#[cfg(test)]
mod tests;
