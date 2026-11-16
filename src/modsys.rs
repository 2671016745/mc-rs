//! mod 系统（自研）。
//!
//! 目标：提供与 NeoForge/Fabric 一致的 mod 管理体验——清单解析、依赖检查、
//! 启用/禁用、加载/卸载。清单格式兼容 `META-INF/neoforge.mods.toml` 的子集，
//! 并扩展为自研 `mcmod.toml`（Rust 原生/脚本 mod）。
//!
//! 注意：NeoForge 的 mod 是 Java 字节码，依赖 JVM 与 Mojang 映射，无法被
//! Rust 本体直接加载；本模块负责“管理面”（发现、依赖、启停），加载面由
//! 自研 mod 运行时（Lua / Rust cdylib）承担。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct ModsToml {
    #[serde(rename = "modLoader")]
    pub mod_loader: String,
    #[serde(rename = "loaderVersion")]
    pub loader_version: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub mods: Vec<ModEntry>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Vec<Dependency>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModEntry {
    #[serde(rename = "modId")]
    pub mod_id: String,
    pub version: String,
    #[serde(default)]
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub authors: Option<String>,
    #[serde(default)]
    pub side: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Dependency {
    #[serde(rename = "modId")]
    pub mod_id: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub mandatory: Option<bool>,
    #[serde(default)]
    #[serde(rename = "versionRange")]
    pub version_range: Option<String>,
    #[serde(default)]
    pub ordering: Option<String>,
    #[serde(default)]
    pub side: Option<String>,
}

#[derive(Debug, Serialize)]
pub enum ModInfo {
    NeoForge(ModsToml),
    Fabric(FabricModJson),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FabricModJson {
    #[serde(rename = "schemaVersion", default = "default_schema")]
    pub schema_version: u32,
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub authors: Vec<serde_json::Value>,
    #[serde(default)]
    pub depends: BTreeMap<String, String>,
    #[serde(default)]
    pub recommends: BTreeMap<String, String>,
    #[serde(default)]
    pub suggests: BTreeMap<String, String>,
    #[serde(default)]
    pub breaks: BTreeMap<String, String>,
    #[serde(default)]
    pub entrypoints: BTreeMap<String, Vec<serde_json::Value>>,
    #[serde(default)]
    pub mixins: Vec<serde_json::Value>,
}

fn default_schema() -> u32 {
    1
}

pub fn parse_fabric_json(s: &str) -> Result<FabricModJson> {
    Ok(serde_json::from_str(s).context("parse fabric.mod.json")?)
}

/// 解析 neoforge.mods.toml / mods.toml。
pub fn parse_str(s: &str) -> Result<ModsToml> {
    let m: ModsToml = toml::from_str(s).context("parse mod manifest")?;
    Ok(m)
}

pub fn parse_file(path: &Path) -> Result<ModInfo> {
    let s = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext.eq_ignore_ascii_case("json") {
        Ok(ModInfo::Fabric(parse_fabric_json(&s)?))
    } else {
        Ok(ModInfo::NeoForge(parse_str(&s)?))
    }
}

/// 检查依赖是否满足（仅 required/mandatory，版本范围用简单区间判断）。
pub fn check_dependencies<'a, I>(manifest: &ModsToml, loaded: I) -> Vec<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let loaded: Vec<&str> = loaded.into_iter().collect();
    let mut missing = Vec::new();
    for (mod_id, deps) in &manifest.dependencies {
        for dep in deps {
            let required = dep
                .mandatory
                .unwrap_or_else(|| dep.kind == "required");
            if !required {
                continue;
            }
            if !loaded.iter().any(|l| l == &dep.mod_id) {
                missing.push(format!("{mod_id} -> {}", dep.mod_id));
            }
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"
modLoader = "javafml"
loaderVersion = "[21,)"
license = "MIT"

[[mods]]
modId = "demo"
version = "1.0.0"
displayName = "Demo"
authors = "tester"

[[dependencies.demo]]
modId = "neoforge"
type = "required"
versionRange = "[21.1,)"
side = "BOTH"

[[dependencies.demo]]
modId = "optional_mod"
type = "optional"
versionRange = "[1.0,)"
"#;

    #[test]
    fn parses_fabric_mod_json() {
        let j = r#"{"schemaVersion":1,"id":"demo","version":"1.0.0","name":"Demo","authors":["tester"],"depends":{"fabricloader":">=0.15.0","minecraft":"~1.21"}}"#;
        let f = parse_fabric_json(j).unwrap();
        assert_eq!(f.id, "demo");
        assert_eq!(f.depends.get("minecraft").map(String::as_str), Some("~1.21"));
        assert_eq!(f.schema_version, 1);
    }

    #[test]
    fn parses_neoforge_manifest() {
        let m = parse_str(FIXTURE).unwrap();
        assert_eq!(m.mod_loader, "javafml");
        assert_eq!(m.mods.len(), 1);
        assert_eq!(m.mods[0].mod_id, "demo");
        assert_eq!(m.dependencies.len(), 1);
        let deps = m.dependencies.get("demo").unwrap();
        assert_eq!(deps.len(), 2);
        assert_eq!(check_dependencies(&m, ["neoforge"]), Vec::<String>::new());
        assert_eq!(
            check_dependencies(&m, ["optional_mod"]).len(),
            1,
            "missing required neoforge"
        );
    }
}
