//! Windows Credential Manager adapter for generic credentials.

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
