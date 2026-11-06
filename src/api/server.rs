//! Server identity and OpenSubsonic capability discovery.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde_json::Value;

use super::OpenSubsonicProvider;
use crate::models::{OpenSubsonicExtension, ServerCapabilities, ServerInfo};

impl OpenSubsonicProvider {
    pub async fn ping(&self) -> Result<ServerInfo> {
        let body = self.get_json("ping", &[]).await?;
        Ok(parse_server_info(&body))
    }

    /// Probe OpenSubsonic extensions separately from `ping`: capability
    /// discovery must never delay a usable legacy Subsonic connection.
    pub async fn open_subsonic_info(&self) -> Result<ServerInfo> {
        let body = self.get_json("getOpenSubsonicExtensions", &[]).await?;
        let mut info = parse_server_info(&body);
        merge_open_subsonic_capabilities(&mut info, &body);
        Ok(info)
    }
}

fn parse_server_info(body: &Value) -> ServerInfo {
    let mut info = ServerInfo {
        app_name: body
            .get("type")
            .or_else(|| body.get("app"))
            .and_then(Value::as_str)
            .map(str::to_string),
        server_version: body
            .get("serverVersion")
            .and_then(Value::as_str)
            .map(str::to_string),
        protocol_version: body
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        open_subsonic: body
            .get("openSubsonic")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        extensions_discovered: false,
        extensions: Vec::new(),
        capabilities: ServerCapabilities::default(),
    };
    info.refresh_capabilities();
    info
}

fn merge_open_subsonic_capabilities(info: &mut ServerInfo, body: &Value) {
    if let Some(name) = body
        .get("type")
        .or_else(|| body.get("app"))
        .and_then(Value::as_str)
    {
        info.app_name = Some(name.to_string());
    }
    if let Some(version) = body.get("serverVersion").and_then(Value::as_str) {
        info.server_version = Some(version.to_string());
    }
    if let Some(version) = body.get("version").and_then(Value::as_str) {
        info.protocol_version = version.to_string();
    }

    // A successful response from this OpenSubsonic-only endpoint is itself a
    // capability signal, including for servers that omit `openSubsonic=true`.
    info.open_subsonic = true;
    info.extensions_discovered = true;
    info.extensions = parse_open_subsonic_extensions(body);
    info.refresh_capabilities();
}

fn parse_open_subsonic_extensions(body: &Value) -> Vec<OpenSubsonicExtension> {
    let mut extensions: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    for extension in body
        .get("openSubsonicExtensions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = extension.get("name").and_then(Value::as_str) else {
            continue;
        };
        if name.trim().is_empty() {
            continue;
        }
        let versions = extensions.entry(name.to_string()).or_default();
        for version in extension
            .get("versions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .filter_map(|version| u32::try_from(version).ok())
        {
            versions.insert(version);
        }
    }

    extensions
        .into_iter()
        .map(|(name, versions)| OpenSubsonicExtension {
            name,
            versions: versions.into_iter().collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests;
