//! 自研 mod 运行时：`mcmod.toml` 清单 + Lua 脚本入口。
//!
//! 与 Java mod（NeoForge/Fabric）不同，这里 mod 用 Lua 编写，运行在嵌入式 Lua 5.4
//! 中，通过全局 `mc` 表调用引擎 API（注册方块/物品、事件、日志等）。

use anyhow::{Context, Result};
use mlua::Lua;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModManifest {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default = "default_entry")]
    pub entry: String,
}

fn default_entry() -> String {
    "main.lua".to_string()
}

#[derive(Debug, Clone)]
pub struct LoadedMod {
    pub manifest: ModManifest,
    pub path: PathBuf,
}

pub struct ModRuntime {
    pub lua: Lua,
    pub mods: Vec<LoadedMod>,
}

impl ModRuntime {
    pub fn new() -> Result<Self> {
        let lua = Lua::new();
        let rt = Self {
            lua,
            mods: Vec::new(),
        };
        rt.register_api()?;
        Ok(rt)
    }

    fn register_api(&self) -> Result<()> {
        let mc = self.lua.create_table()?;
        mc.set("version", env!("CARGO_PKG_VERSION"))?;
        mc.set(
            "log",
            self.lua
                .create_function(|_lua, (level, msg): (String, String)| {
                    println!("[mod:{level}] {msg}");
                    Ok(())
                })?,
        )?;
        mc.set(
            "register_block",
            self.lua
                .create_function(|_lua, (id, name): (String, String)| {
                    println!("[mod] register_block {id} ({name})");
                    Ok(())
                })?,
        )?;
        self.lua.globals().set("mc", mc)?;
        Ok(())
    }

    pub fn load_mod(&mut self, dir: &Path) -> Result<LoadedMod> {
        let manifest_path = dir.join("mcmod.toml");
        let s = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("read {}", manifest_path.display()))?;
        let manifest: ModManifest = toml::from_str(&s).context("parse mcmod.toml")?;
        let entry = dir.join(&manifest.entry);
        let code = std::fs::read_to_string(&entry)
            .with_context(|| format!("read {}", entry.display()))?;
        self.lua
            .load(&code)
            .set_name(&manifest.id)
            .exec()
            .context("exec mod entry")?;
        let loaded = LoadedMod {
            manifest,
            path: dir.to_path_buf(),
        };
        self.mods.push(loaded.clone());
        Ok(loaded)
    }

    pub fn load_dir(&mut self, dir: &Path) -> Result<Vec<LoadedMod>> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir)? {
            let p = e?.path();
            if p.is_dir() && p.join("mcmod.toml").exists() {
                out.push(self.load_mod(&p)?);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_lua_mod() {
        let dir = std::env::temp_dir().join(format!("mcmod-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("mcmod.toml"),
            "id = \"demo\"\nversion = \"1.0.0\"\nname = \"Demo\"\nauthors = [\"tester\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("main.lua"),
            "mc.log('info', 'hello')\nmc.register_block('demo:ruby', 'Ruby')\nresult = 'ok'\n",
        )
        .unwrap();

        let mut rt = ModRuntime::new().unwrap();
        let m = rt.load_mod(&dir).unwrap();
        assert_eq!(m.manifest.id, "demo");
        let result: String = rt.lua.globals().get("result").unwrap();
        assert_eq!(result, "ok");
        std::fs::remove_dir_all(&dir).ok();
    }
}
