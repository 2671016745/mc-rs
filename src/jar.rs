//! mod jar 解析：读取 NeoForge / Fabric / 旧版 Forge 的清单，统计内容，
//! 并把「内容型 mod」（assets/、data/）解压到游戏目录使其生效。
//!
//! 结论保持不变：Java 字节码 mod 无法在 Rust 本体中执行（见
//! `docs/java-mod-bridge.md`）。本模块负责把 jar 里**不需要 JVM 就能生效**
//! 的部分（资源、数据包、语言、图标）真正用起来，并对代码部分给出明确报告。

use anyhow::{Context, Result};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::modsys;

#[derive(Debug, serde::Serialize)]
pub struct JarReport {
    pub path: PathBuf,
    pub size: u64,
    /// 清单来源文件：META-INF/neoforge.mods.toml / fabric.mod.json / mcmod.info
    pub manifest_file: Option<String>,
    pub kind: &'static str,
    pub mod_id: Option<String>,
    pub version: Option<String>,
    pub class_count: usize,
    pub asset_count: usize,
    pub data_count: usize,
    pub mixin_configs: Vec<String>,
    pub entrypoints: Vec<String>,
    /// 是否需要 JVM 才能运行（含 .class 或有 mixin）
    pub needs_jvm: bool,
}

fn manifest_kind(name: &str) -> Option<&'static str> {
    let n = name.replace('\\', "/");
    if n.eq_ignore_ascii_case("META-INF/neoforge.mods.toml")
        || n.eq_ignore_ascii_case("META-INF/mods.toml")
    {
        Some("neoforge")
    } else if n.eq_ignore_ascii_case("fabric.mod.json") {
        Some("fabric")
    } else if n.eq_ignore_ascii_case("mcmod.info") {
        Some("forge-legacy")
    } else {
        None
    }
}

/// 解析清单并取出 mod id / version（两种格式都尝试）。
fn read_manifest(
    kind: &str,
    content: &str,
) -> Result<(Option<String>, Option<String>, Vec<String>)> {
    let mut id = None;
    let mut version = None;
    let mut entrypoints = Vec::new();
    match kind {
        "fabric" => {
            let f = modsys::parse_fabric_json(content)?;
            id = Some(f.id);
            version = Some(f.version);
            for (k, vals) in &f.entrypoints {
                for v in vals {
                    if let Some(s) = v.as_str() {
                        entrypoints.push(format!("{k}: {s}"));
                    }
                }
            }
        }
        _ => {
            let m = modsys::parse_str(content)?;
            if let Some(first) = m.mods.first() {
                id = Some(first.mod_id.clone());
                version = Some(first.version.clone());
            }
        }
    }
    Ok((id, version, entrypoints))
}

pub fn inspect(path: &Path) -> Result<JarReport> {
    let file = File::open(path)
        .with_context(|| format!("open {}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).context("open jar as zip")?;

    let mut report = JarReport {
        path: path.to_path_buf(),
        size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        manifest_file: None,
        kind: "unknown",
        mod_id: None,
        version: None,
        class_count: 0,
        asset_count: 0,
        data_count: 0,
        mixin_configs: Vec::new(),
        entrypoints: Vec::new(),
        needs_jvm: false,
    };

    let mut manifest_content: Option<(String, String)> = None;
    let mut mixin_set: Vec<String> = Vec::new();

    for i in 0..zip.len() {
        let mut f = zip.by_index(i).context("zip entry")?;
        let name = f.name().map(|c| c.into_owned()).unwrap_or_default();
        let lower = name.to_ascii_lowercase();
        if f.is_dir() {
            continue;
        }
        if lower.ends_with(".class") {
            report.class_count += 1;
        } else if let Some(k) = manifest_kind(&name) {
            let mut s = String::new();
            f.read_to_string(&mut s).ok();
            manifest_content = Some((k.to_string(), s));
        } else if lower.starts_with("assets/") {
            report.asset_count += 1;
        } else if lower.starts_with("data/") {
            report.data_count += 1;
        }
        if lower.ends_with(".mixins.json") || lower.ends_with(".mixin.json") {
            mixin_set.push(name.clone());
        }
    }

    if let Some((k, s)) = manifest_content {
        let (id, version, eps) = read_manifest(&k, &s).unwrap_or((None, None, Vec::new()));
        report.kind = match k.as_str() {
            "fabric" => "fabric",
            "forge-legacy" => "forge-legacy",
            _ => "neoforge",
        };
        report.manifest_file = Some(match k.as_str() {
            "fabric" => "fabric.mod.json".to_string(),
            "forge-legacy" => "mcmod.info".to_string(),
            _ => "META-INF/neoforge.mods.toml".to_string(),
        });
        report.mod_id = id;
        report.version = version;
        report.entrypoints = eps;
    }
    report.mixin_configs = mixin_set;
    report.needs_jvm = report.class_count > 0 || !report.mixin_configs.is_empty();
    Ok(report)
}

/// 把 jar 里的 assets/ 与 data/ 解压到游戏目录（内容型 mod 生效）。
/// 返回解压的文件数。
pub fn extract_content(jar: &Path, game_dir: &Path) -> Result<usize> {
    let file = File::open(jar).with_context(|| format!("open {}", jar.display()))?;
    let mut zip = zip::ZipArchive::new(file).context("open jar as zip")?;
    let mut n = 0;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).context("zip entry")?;
        let name = f.name().map(|c| c.into_owned()).unwrap_or_default();
        let lower = name.to_ascii_lowercase();
        if f.is_dir() {
            continue;
        }
        let target = if lower.starts_with("assets/") {
            game_dir.join(&name)
        } else if lower.starts_with("data/") {
            game_dir.join(&name)
        } else {
            continue;
        };
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).context("read jar entry")?;
        let mut out = File::create(&target)
            .with_context(|| format!("write {}", target.display()))?;
        out.write_all(&buf).context("write extracted file")?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Seek;

    fn make_test_jar(path: &Path) {
        let f = File::create(path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts: zip::write::FileOptions<'_, '_, ()> =
            zip::write::FileOptions::default();
        w.start_file("fabric.mod.json", opts).unwrap();
        w.write_all(
            br#"{"schemaVersion":1,"id":"demo","version":"1.0.0","entrypoints":{"main":["com.example.Demo"]}}"#,
        )
        .unwrap();
        w.start_file("assets/demo/models/block/x.json", opts).unwrap();
        w.write_all(b"{}").unwrap();
        w.start_file("data/demo/recipes/x.json", opts).unwrap();
        w.write_all(b"{}").unwrap();
        w.start_file("com/example/Demo.class", opts).unwrap();
        w.write_all(b"\xca\xfe\xba\xbe").unwrap();
        w.start_file("demo.mixins.json", opts).unwrap();
        w.write_all(b"{}").unwrap();
        let mut f = w.finish().unwrap();
        f.rewind().ok();
    }

    #[test]
    fn inspects_fabric_jar() {
        let dir = std::env::temp_dir().join("mcrs_jar_test");
        std::fs::create_dir_all(&dir).unwrap();
        let jar = dir.join("demo.jar");
        make_test_jar(&jar);
        let r = inspect(&jar).unwrap();
        assert_eq!(r.kind, "fabric");
        assert_eq!(r.mod_id.as_deref(), Some("demo"));
        assert_eq!(r.version.as_deref(), Some("1.0.0"));
        assert_eq!(r.class_count, 1);
        assert_eq!(r.asset_count, 1);
        assert_eq!(r.data_count, 1);
        assert_eq!(r.mixin_configs.len(), 1);
        assert!(r.needs_jvm);
        assert_eq!(r.entrypoints.len(), 1);

        let out = dir.join("game");
        let n = extract_content(&jar, &out).unwrap();
        assert_eq!(n, 2);
        assert!(out.join("assets/demo/models/block/x.json").exists());
        assert!(out.join("data/demo/recipes/x.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
