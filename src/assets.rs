//! 官方版本文件下载、校验与资产索引。
//!
//! 数据源：Mojang launchermeta 版本清单 + piston-meta 版本 json。
//! 客户端 jar 内含 assets/minecraft（模型/方块状态/部分纹理）与 data/minecraft（标签/世界生成等）。

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

pub const VERSION_MANIFEST: &str =
    "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";

const PREFIXES: &[&str] = &[
    "assets/minecraft/blockstates/",
    "assets/minecraft/models/block/",
    "assets/minecraft/models/item/",
    "assets/minecraft/textures/",
    "assets/minecraft/lang/",
    "assets/minecraft/atlases/",
    "assets/minecraft/particles/",
    "assets/minecraft/shaders/",
    "data/minecraft/tags/block/",
    "data/minecraft/tags/item/",
    "data/minecraft/recipe/",
    "data/minecraft/worldgen/",
    "data/minecraft/dimension_type/",
    "data/minecraft/loot_table/",
];

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub versions: Vec<ManifestVersion>,
}

#[derive(Debug, Deserialize)]
pub struct ManifestVersion {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct VersionJson {
    pub id: String,
    #[serde(default)]
    pub downloads: BTreeMap<String, Download>,
    #[serde(default)]
    #[serde(rename = "assetIndex")]
    pub asset_index: Option<AssetIndex>,
    #[serde(default)]
    #[serde(rename = "javaVersion")]
    pub java_version: Option<JavaVersion>,
}

#[derive(Debug, Deserialize)]
pub struct Download {
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Deserialize)]
pub struct AssetIndex {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct JavaVersion {
    pub component: String,
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

#[derive(Debug, Serialize)]
pub struct IndexReport {
    pub jar: String,
    pub entries: usize,
    pub uncompressed_bytes: u64,
    pub counts: BTreeMap<String, usize>,
    pub lang_files: Vec<String>,
    pub lang_keys: usize,
    pub block_tag_files: usize,
    pub block_tag_entries: usize,
    pub item_tag_files: usize,
    pub item_tag_entries: usize,
}

pub fn fetch_manifest() -> Result<Manifest> {
    let resp = ureq::get(VERSION_MANIFEST)
        .call()
        .context("failed to fetch version manifest")?;
    let manifest: Manifest = resp.into_json().context("invalid manifest json")?;
    Ok(manifest)
}

pub fn fetch_version_json(url: &str) -> Result<VersionJson> {
    let resp = ureq::get(url).call().with_context(|| format!("GET {url}"))?;
    let vj: VersionJson = resp.into_json().context("invalid version json")?;
    Ok(vj)
}

pub fn download(url: &str) -> Result<Vec<u8>> {
    let resp = ureq::get(url).call().with_context(|| format!("GET {url}"))?;
    let mut buf = Vec::new();
    resp.into_reader().read_to_end(&mut buf).context("read body")?;
    Ok(buf)
}

pub fn verify_sha1(data: &[u8], expected: &str) -> Result<()> {
    let digest = Sha1::digest(data);
    let got: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    if !got.eq_ignore_ascii_case(expected) {
        bail!("sha1 mismatch: got {got}, expected {expected}");
    }
    Ok(())
}

fn count_json_object_keys(value: &serde_json::Value) -> usize {
    value.as_object().map(|o| o.len()).unwrap_or(0)
}

fn count_tag_values(value: &serde_json::Value) -> usize {
    value
        .get("values")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0)
}

/// 打开 jar/zip 并返回 (名称列表, 归档)。
fn open_zip(path: &Path) -> Result<zip::ZipArchive<std::fs::File>> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let zip = zip::ZipArchive::new(file)?;
    Ok(zip)
}

pub fn index_jar(path: &Path) -> Result<IndexReport> {
    let mut zip = open_zip(path)?;
    let len = zip.len();
    let mut counts: BTreeMap<String, usize> = PREFIXES
        .iter()
        .map(|p| (p.to_string(), 0))
        .collect();
    let mut uncompressed = 0u64;
    let mut lang_files = Vec::new();
    let mut lang_keys = 0usize;
    let mut block_tag_files = 0usize;
    let mut block_tag_entries = 0usize;
    let mut item_tag_files = 0usize;
    let mut item_tag_entries = 0usize;

    for i in 0..len {
        let mut f = zip.by_index(i)?;
        let name = f.name().map(|c| c.into_owned()).unwrap_or_default();
        uncompressed += f.size();
        for p in PREFIXES {
            if name.starts_with(p) {
                *counts.get_mut(*p).unwrap() += 1;
            }
        }
        if name.starts_with("assets/minecraft/lang/") && name.ends_with(".json") {
            let mut s = String::new();
            f.read_to_string(&mut s)?;
            let v: serde_json::Value = serde_json::from_str(&s).unwrap_or_default();
            lang_keys += count_json_object_keys(&v);
            lang_files.push(name);
        } else if name.starts_with("data/minecraft/tags/block/") && name.ends_with(".json") {
            let mut s = String::new();
            f.read_to_string(&mut s)?;
            let v: serde_json::Value = serde_json::from_str(&s).unwrap_or_default();
            block_tag_files += 1;
            block_tag_entries += count_tag_values(&v);
        } else if name.starts_with("data/minecraft/tags/item/") && name.ends_with(".json") {
            let mut s = String::new();
            f.read_to_string(&mut s)?;
            let v: serde_json::Value = serde_json::from_str(&s).unwrap_or_default();
            item_tag_files += 1;
            item_tag_entries += count_tag_values(&v);
        }
    }

    Ok(IndexReport {
        jar: path.display().to_string(),
        entries: len,
        uncompressed_bytes: uncompressed,
        counts,
        lang_files,
        lang_keys,
        block_tag_files,
        block_tag_entries,
        item_tag_files,
        item_tag_entries,
    })
}

/// 读取 jar 内单个文本文件。
#[allow(dead_code)]
pub fn read_text(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}
