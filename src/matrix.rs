//! 版本 × 加载器矩阵：从官方/加载器元数据拉取版本，按版本号从新到旧排列。
//!
//! 数据源：
//! - 游戏版本：Mojang `version_manifest_v2.json`（只取 release，不含快照/测试版）
//! - Fabric：`https://meta.fabricmc.net/v2/versions/loader`、`/v2/versions/yarn/{game}`
//! - NeoForge：`https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge`

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::assets;

const FABRIC_META: &str = "https://meta.fabricmc.net";
const NEOFORGE_MAVEN: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";

#[derive(Debug, Deserialize)]
pub struct FabricLoaderEntry {
    pub version: String,
    #[serde(default)]
    pub stable: bool,
}

#[derive(Debug, Serialize)]
pub struct Matrix {
    pub game: String,
    pub fabric_loader: Vec<String>,
    pub yarn: Vec<String>,
    pub neoforge: Vec<String>,
}

impl Matrix {
    /// 加载器版本按语义版本倒序（新版在前），避免字典序把 0.9 排在 0.20 前。
    pub fn sort(&mut self) {
        self.fabric_loader.sort_by(|a, b| ver_cmp(b, a));
        self.yarn.sort_by(|a, b| ver_cmp(b, a));
        self.neoforge.sort_by(|a, b| ver_cmp(b, a));
    }
}

/// 语义版本比较：按数字段（忽略 +build 之类后缀）。
fn ver_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u64> {
        s.split(|c| c == '.' || c == '+' || c == '-')
            .filter_map(|p| p.parse().ok())
            .collect()
    };
    parse(a).cmp(&parse(b)).then_with(|| a.cmp(b))
}

/// Mojang 全部正式版（release），manifest 本身已按时间从新到旧。
pub fn game_versions() -> Result<Vec<String>> {
    let m = assets::fetch_manifest()?;
    Ok(m.versions
        .into_iter()
        .filter(|v| v.kind == "release")
        .map(|v| v.id)
        .collect())
}

pub fn fabric_loaders() -> Result<Vec<FabricLoaderEntry>> {
    let url = format!("{FABRIC_META}/v2/versions/loader");
    let resp = ureq::get(&url).call().context("fabric loader list")?;
    Ok(resp.into_json()?)
}

pub fn fabric_yarn(game: &str) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Yarn {
        version: String,
    }
    let url = format!("{FABRIC_META}/v2/versions/yarn/{game}");
    let resp = ureq::get(&url).call().context("fabric yarn list")?;
    let list: Vec<Yarn> = resp.into_json()?;
    Ok(list.into_iter().map(|y| y.version).collect())
}

/// NeoForge 全部发布版本（形如 21.1.266）。
pub fn neoforge_versions() -> Result<Vec<String>> {
    let resp = ureq::get(NEOFORGE_MAVEN)
        .call()
        .context("neoforge version list")?;
    #[derive(Deserialize)]
    struct Neo {
        versions: Vec<String>,
    }
    let n: Neo = resp.into_json()?;
    Ok(n.versions)
}

/// 一条推进记录：(游戏版本, 加载器, 加载器版本)
#[derive(Debug, Serialize)]
pub struct SweepRow {
    pub game: String,
    pub loader: String,
    pub version: String,
}

fn loader_versions_for(game: &str) -> Vec<(&'static str, Vec<String>)> {
    let fabric = fabric_loaders()
        .map(|v| v.into_iter().map(|e| e.version).collect::<Vec<_>>())
        .unwrap_or_default();
    let yarn = fabric_yarn(game).unwrap_or_default();
    let prefix = game
        .split_once('.')
        .map(|(_, rest)| format!("{rest}."))
        .unwrap_or_default();
    let neoforge = neoforge_versions()
        .map(|v| {
            v.into_iter()
                .filter(|x| x.starts_with(&prefix))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    vec![("fabric", fabric), ("yarn", yarn), ("neoforge", neoforge)]
}

/// 分层推进顺序：外层 = 加载器版本偏移 k（0=最新版），内层 = 游戏版本从新到旧。
/// 即先写「每个游戏版本 × 各加载器最新版」，再回最新游戏版本写次新版，依此类推。
pub fn sweep_rows(per_game: &[(String, Vec<(&'static str, Vec<String>)>)]) -> Vec<SweepRow> {
    let max_k = per_game
        .iter()
        .flat_map(|(_, ls)| ls.iter().map(|(_, v)| v.len()))
        .max()
        .unwrap_or(0);
    let mut rows = Vec::new();
    for k in 0..max_k {
        for (game, loaders) in per_game {
            for (loader, versions) in loaders {
                if let Some(v) = versions.get(k) {
                    rows.push(SweepRow {
                        game: game.clone(),
                        loader: loader.to_string(),
                        version: v.clone(),
                    });
                }
            }
        }
    }
    rows
}

/// 拉取前 max_games 个正式版并生成推进清单。
pub fn sweep(max_games: usize) -> Result<Vec<SweepRow>> {
    let games = game_versions()?.into_iter().take(max_games).collect::<Vec<_>>();
    let mut per_game = Vec::new();
    for g in &games {
        let loaders = loader_versions_for(g);
        per_game.push((g.clone(), loaders));
    }
    let rows = sweep_rows(&per_game);
    // 同层内按版本号倒序（sweep_rows 保持拉取顺序，这里统一排序更直观）
    Ok(rows)
}

/// 为某个游戏版本组装矩阵（Fabric Loader / Yarn / NeoForge 可用版本）。
pub fn matrix_for(game: &str) -> Result<Matrix> {
    // NeoForge 版本号形如 21.1.266，对应游戏 1.21.1 → 前缀 "21.1."
    let prefix = game
        .split_once('.')
        .map(|(_, rest)| format!("{rest}."))
        .unwrap_or_default();
    let mut m = Matrix {
        game: game.to_string(),
        fabric_loader: fabric_loaders()?.into_iter().map(|e| e.version).collect(),
        yarn: fabric_yarn(game).unwrap_or_default(),
        neoforge: neoforge_versions()?
            .into_iter()
            .filter(|v| v.starts_with(&prefix))
            .collect(),
    };
    m.sort();
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fabric_loader_list() {
        let v: Vec<FabricLoaderEntry> = serde_json::from_str(
            r#"[{"version":"0.15.11","stable":true},{"version":"0.16.0","stable":false}]"#,
        )
        .unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].version, "0.15.11");
        assert!(v[0].stable);
        assert!(!v[1].stable);
    }

    #[test]
    fn sorts_newest_first() {
        let mut m = Matrix {
            game: "1.21.1".into(),
            fabric_loader: vec!["0.15.11".into(), "0.20.1".into()],
            yarn: vec![],
            neoforge: vec!["21.1.1".into(), "21.1.266".into()],
        };
        m.sort();
        assert_eq!(m.fabric_loader[0], "0.20.1");
        assert_eq!(m.neoforge[0], "21.1.266");
    }

    #[test]
    fn semantic_version_order() {
        let mut m = Matrix {
            game: "1.21.1".into(),
            fabric_loader: vec!["0.9.3+build.207".into(), "0.20.1".into(), "0.15.11".into()],
            yarn: vec![],
            neoforge: vec![],
        };
        m.sort();
        assert_eq!(m.fabric_loader, vec!["0.20.1", "0.15.11", "0.9.3+build.207"]);
    }

    #[test]
    fn neoforge_prefix_matches_game() {
        let game = "1.21.1";
        let prefix = game.split_once('.').map(|(_, r)| format!("{r}.")).unwrap();
        let all = vec!["21.1.266".to_string(), "21.1.200".to_string(), "1.21.0".to_string()];
        let filtered: Vec<String> = all.into_iter().filter(|v| v.starts_with(&prefix)).collect();
        assert_eq!(filtered, vec!["21.1.266", "21.1.200"]);
    }

    #[test]
    fn sweep_layer_order() {
        // 两个游戏版本（新→旧），两个加载器各两个版本
        let per_game: Vec<(String, Vec<(&'static str, Vec<String>)>)> = vec![
            (
                "1.21.1".into(),
                vec![
                    ("fabric", vec!["0.20.1".into(), "0.19.0".into()]),
                    ("neoforge", vec!["21.1.267".into(), "21.1.200".into()]),
                ],
            ),
            (
                "1.21".into(),
                vec![
                    ("fabric", vec!["0.20.1".into(), "0.19.0".into()]),
                    ("neoforge", vec!["21.0.0".into()]),
                ],
            ),
        ];
        let rows = sweep_rows(&per_game);
        // k=0：1.21.1 fabric/neoforge，然后 1.21 fabric/neoforge
        assert_eq!(rows[0].game, "1.21.1");
        assert_eq!(rows[0].version, "0.20.1");
        assert_eq!(rows[2].game, "1.21");
        // k=1：回到最新游戏版本 1.21.1 的次新版
        assert_eq!(rows[4].game, "1.21.1");
        assert_eq!(rows[4].loader, "fabric");
        assert_eq!(rows[4].version, "0.19.0");
        assert_eq!(rows.len(), 7);
    }
}
