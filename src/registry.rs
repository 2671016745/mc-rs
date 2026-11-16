//! 从官方客户端 jar 的 blockstates 构建方块注册表。
//!
//! 每个 `assets/minecraft/blockstates/<id>.json` 描述一个方块的状态变体：
//! - `variants`: 状态字符串 -> 单个模型应用 或 模型应用数组
//! - `multipart`: 条件列表（when/apply），用于台阶、箱子等组合方块

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Serialize, Default)]
pub struct BlockRegistry {
    pub version: String,
    pub block_count: usize,
    pub variant_count: usize,
    pub multipart_count: usize,
    pub model_count: usize,
    pub blocks: Vec<BlockDef>,
}

#[derive(Debug, Serialize)]
pub struct BlockDef {
    pub id: String,
    pub variants: usize,
    pub multipart: bool,
    pub models: Vec<String>,
}

fn collect_models(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(m)) = map.get("model") {
                out.insert(m.clone());
            }
            for v in map.values() {
                collect_models(v, out);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                collect_models(v, out);
            }
        }
        _ => {}
    }
}

pub fn build_registry(jar: &Path) -> Result<BlockRegistry> {
    let file = std::fs::File::open(jar)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let mut reg = BlockRegistry::default();

    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        let name = f.name().map(|c| c.into_owned()).unwrap_or_default();
        if !name.starts_with("assets/minecraft/blockstates/") || !name.ends_with(".json") {
            continue;
        }
        let id = name
            .trim_start_matches("assets/minecraft/blockstates/")
            .trim_end_matches(".json")
            .to_string();
        let mut s = String::new();
        f.read_to_string(&mut s)?;
        let v: Value = serde_json::from_str(&s).unwrap_or_default();

        let mut def = BlockDef {
            id,
            variants: 0,
            multipart: false,
            models: Vec::new(),
        };
        if let Some(Value::Object(variants)) = v.get("variants") {
            def.variants = variants.len();
        }
        if let Some(Value::Array(mp)) = v.get("multipart") {
            def.multipart = true;
            def.variants += mp.len();
        }
        let mut models = BTreeSet::new();
        collect_models(&v, &mut models);
        def.models = models.into_iter().collect();

        reg.variant_count += def.variants;
        if def.multipart {
            reg.multipart_count += 1;
        }
        reg.model_count += def.models.len();
        reg.blocks.push(def);
    }

    reg.version = "1.21.1".to_string();
    reg.block_count = reg.blocks.len();
    Ok(reg)
}
