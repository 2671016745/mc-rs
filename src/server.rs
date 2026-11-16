//! 服务器列表与收藏（持久化到 $HOME/.mc-rs/servers.json）。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::net;

pub const DEFAULT_PORT: u16 = 25565;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerEntry {
    pub name: String,
    pub address: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ServerList {
    #[serde(default)]
    pub servers: Vec<ServerEntry>,
}

impl ServerList {
    pub fn config_dir() -> PathBuf {
        match std::env::var_os("HOME") {
            Some(h) => PathBuf::from(h).join(".mc-rs"),
            None => PathBuf::from(".mc-rs"),
        }
    }

    pub fn path() -> PathBuf {
        Self::config_dir().join("servers.json")
    }

    pub fn load() -> Result<Self> {
        let p = Self::path();
        if !p.exists() {
            return Ok(Self::default());
        }
        let s = std::fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
        Ok(serde_json::from_str(&s).unwrap_or_default())
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::config_dir();
        std::fs::create_dir_all(&dir)?;
        let p = Self::path();
        let s = serde_json::to_string_pretty(self)?;
        std::fs::write(&p, s).with_context(|| format!("write {}", p.display()))?;
        Ok(())
    }

    pub fn add(&mut self, entry: ServerEntry) {
        if !self
            .servers
            .iter()
            .any(|s| s.address == entry.address && s.port == entry.port)
        {
            self.servers.push(entry);
        }
    }

    pub fn remove(&mut self, index: usize) -> Option<ServerEntry> {
        if index < self.servers.len() {
            Some(self.servers.remove(index))
        } else {
            None
        }
    }

    /// 对所有服务器执行 ping。
    pub fn ping_all(&self) -> Vec<(usize, Result<(net::ServerStatus, u128)>)> {
        self.servers
            .iter()
            .enumerate()
            .map(|(i, s)| (i, net::ping(&s.address, s.port)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_dedupes_by_address_and_port() {
        let mut list = ServerList::default();
        list.add(ServerEntry {
            name: "A".into(),
            address: "mc.example".into(),
            port: 25565,
            version: None,
            notes: None,
        });
        list.add(ServerEntry {
            name: "B".into(),
            address: "mc.example".into(),
            port: 25565,
            version: None,
            notes: None,
        });
        assert_eq!(list.servers.len(), 1);
    }
}
