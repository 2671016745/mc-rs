//! 皮肤：正版（session server 纹理属性）与离线/自定义皮肤文件。

use anyhow::{Context, Result};
use base64::Engine;
use serde::Deserialize;
use uuid::Uuid;

const PROFILE_URL: &str = "https://sessionserver.mojang.com/session/minecraft/profile";

#[derive(Debug, Deserialize)]
struct Profile {
    id: String,
    name: String,
    #[serde(default)]
    properties: Vec<Property>,
}

#[derive(Debug, Deserialize)]
struct Property {
    name: String,
    value: String,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Textures {
    pub skin_url: Option<String>,
    pub cape_url: Option<String>,
    /// "default"（Steve）或 "slim"（Alex）。
    pub model: String,
}

/// 从 Mojang session server 获取账号纹理属性。
pub fn fetch_textures(uuid: &Uuid) -> Result<Textures> {
    let url = format!("{PROFILE_URL}/{}", uuid.simple());
    let resp = ureq::get(&url).call().with_context(|| format!("GET {url}"))?;
    let p: Profile = resp.into_json()?;
    let value = p
        .properties
        .into_iter()
        .find(|x| x.name == "textures")
        .map(|x| x.value)
        .context("no textures property")?;
    decode_textures(&value)
}

/// 解码 base64 的 textures 属性（内含皮肤/披风 URL 与模型）。
pub fn decode_textures(b64: &str) -> Result<Textures> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("base64 decode textures")?;
    let v: serde_json::Value =
        serde_json::from_slice(&raw).context("textures json")?;
    let tex = v
        .get("textures")
        .and_then(|t| t.as_object())
        .context("textures object")?;
    let url = |key: &str| {
        tex.get(key)
            .and_then(|t| t.get("url"))
            .and_then(|u| u.as_str())
            .map(|s| s.to_string())
    };
    let model = match tex.get("SKIN") {
        Some(skin) => {
            if skin
                .get("metadata")
                .and_then(|m| m.get("model"))
                .and_then(|m| m.as_str())
                == Some("slim")
            {
                "slim"
            } else {
                "default"
            }
        }
        None => "default",
    };
    Ok(Textures {
        skin_url: url("SKIN"),
        cape_url: url("CAPE"),
        model: model.to_string(),
    })
}

/// 读取离线皮肤文件（PNG），返回原始字节。
pub fn read_offline_skin(path: &std::path::Path) -> Result<Vec<u8>> {
    let data = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if data.len() < 8 || data[..8] != [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        anyhow::bail!("not a PNG file: {}", path.display());
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_textures_payload() {
        // textures value: {"textures":{"SKIN":{"url":"http://x/skin.png"},"CAPE":{"url":"http://x/cape.png"}}}
        let json = r#"{"textures":{"SKIN":{"url":"http://x/skin.png"},"CAPE":{"url":"http://x/cape.png"}}}"#;
        let b64 = base64::engine::general_purpose::STANDARD.encode(json);
        let t = decode_textures(&b64).unwrap();
        assert_eq!(t.skin_url.as_deref(), Some("http://x/skin.png"));
        assert_eq!(t.cape_url.as_deref(), Some("http://x/cape.png"));
        assert_eq!(t.model, "default");
    }
}
