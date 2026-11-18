//! 登录方式：离线 / 第三方 Yggdrasil / 微软（正版）。
//!
//! - 离线：Java 的 `UUID.nameUUIDFromBytes("OfflinePlayer:"+name)`。
//! - 第三方：authlib / Yggdrasil 兼容（authenticate / refresh / validate）。
//! - 微软：device code → MSA token → Xbox Live → XSTS → Minecraft token → profile。

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthKind {
    Offline,
    ThirdParty {
        authserver: String,
        client_token: String,
    },
    Microsoft {
        client_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub name: String,
    pub uuid: Uuid,
    pub access_token: String,
    pub kind: AuthKind,
}

impl Account {
    pub fn client_token(&self) -> String {
        match &self.kind {
            AuthKind::ThirdParty { client_token, .. } => client_token.clone(),
            _ => self.uuid.simple().to_string(),
        }
    }
}

/// Java `UUID.nameUUIDFromBytes`（MD5，version 3）。
pub fn offline_uuid(name: &str) -> Uuid {
    let digest = md5::compute(format!("OfflinePlayer:{name}").as_bytes());
    let mut b = digest.0;
    b[6] = (b[6] & 0x0f) | 0x30;
    b[8] = (b[8] & 0x3f) | 0x80;
    Uuid::from_bytes(b)
}

pub fn offline(name: &str) -> Account {
    Account {
        name: name.to_string(),
        uuid: offline_uuid(name),
        access_token: String::new(),
        kind: AuthKind::Offline,
    }
}

// ------------------------------------------------------------------ 第三方

#[derive(Debug, Deserialize)]
pub struct YggdrasilProfile {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct YggdrasilAuth {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "clientToken")]
    pub client_token: String,
    #[serde(rename = "selectedProfile", default)]
    pub selected_profile: Option<YggdrasilProfile>,
}

fn yggdrasil_url(authserver: &str, action: &str) -> String {
    format!("{}/{action}", authserver.trim_end_matches('/'))
}

pub fn yggdrasil_authenticate(
    authserver: &str,
    username: &str,
    password: &str,
    client_token: &str,
) -> Result<YggdrasilAuth> {
    let body = serde_json::json!({
        "agent": { "name": "Minecraft", "version": 1 },
        "username": username,
        "password": password,
        "clientToken": client_token,
        "requestUser": true
    });
    let resp = ureq::post(&yggdrasil_url(authserver, "authenticate"))
        .send_json(&body)
        .context("yggdrasil authenticate")?;
    Ok(resp.into_json()?)
}

pub fn yggdrasil_refresh(authserver: &str, access_token: &str, client_token: &str) -> Result<YggdrasilAuth> {
    let body = serde_json::json!({
        "accessToken": access_token,
        "clientToken": client_token,
        "requestUser": true
    });
    let resp = ureq::post(&yggdrasil_url(authserver, "refresh"))
        .send_json(&body)?;
    Ok(resp.into_json()?)
}

pub fn yggdrasil_validate(authserver: &str, access_token: &str, client_token: &str) -> Result<bool> {
    let body = serde_json::json!({ "accessToken": access_token, "clientToken": client_token });
    let resp = ureq::post(&yggdrasil_url(authserver, "validate")).send_json(&body);
    Ok(resp.is_ok())
}

pub fn yggdrasil_account(
    authserver: &str,
    username: &str,
    password: &str,
    client_token: &str,
) -> Result<Account> {
    let auth = yggdrasil_authenticate(authserver, username, password, client_token)?;
    let profile = auth
        .selected_profile
        .context("no selected profile in yggdrasil response")?;
    let uuid = Uuid::parse_str(&profile.id).unwrap_or_else(|_| offline_uuid(&profile.name));
    Ok(Account {
        name: profile.name,
        uuid,
        access_token: auth.access_token,
        kind: AuthKind::ThirdParty {
            authserver: authserver.to_string(),
            client_token: auth.client_token,
        },
    })
}

// ------------------------------------------------------------------ 会话验证

/// Java 的 serverId hash：`BigInteger(1, SHA1(serverId + secret + pubKey)).toString(16)`。
pub fn server_id_hash(server_id: &str, shared_secret: &[u8], public_key: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(server_id.as_bytes());
    h.update(shared_secret);
    h.update(public_key);
    let d = h.finalize();
    let hex: String = d.iter().map(|b| format!("{b:02x}")).collect();
    let t = hex.trim_start_matches('0');
    if t.is_empty() {
        "0".to_string()
    } else {
        t.to_string()
    }
}

/// 通知第三方 authserver：该账号正在加入某服务器（进服前必做）。
pub fn yggdrasil_join(
    authserver: &str,
    access_token: &str,
    selected_profile: &str,
    server_id: &str,
) -> Result<()> {
    let body = serde_json::json!({
        "accessToken": access_token,
        "selectedProfile": selected_profile,
        "serverId": server_id,
    });
    ureq::post(&yggdrasil_url(
        authserver,
        "sessionserver/session/minecraft/join",
    ))
    .send_json(&body)
    .context("yggdrasil join")?;
    Ok(())
}

/// authserver 侧校验：某玩家是否声明加入了该服务器（返回其 profile）。
pub fn yggdrasil_has_joined(
    authserver: &str,
    username: &str,
    server_id: &str,
) -> Result<Option<YggdrasilProfile>> {
    let url = format!(
        "{}/sessionserver/session/minecraft/hasJoined?username={}&serverId={}",
        authserver.trim_end_matches('/'),
        username,
        server_id
    );
    let resp = ureq::get(&url).call().context("yggdrasil hasJoined")?;
    Ok(Some(resp.into_json()?))
}

// ------------------------------------------------------------------ 微软

const MS_DEVICECODE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const MS_TOKEN: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBL_AUTH: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE: &str = "https://api.minecraftservices.com/minecraft/profile";
const MC_SESSION_JOIN: &str = "https://api.minecraftservices.com/session/join";

#[derive(Debug, Deserialize)]
pub struct DeviceCode {
    #[serde(rename = "device_code")]
    pub device_code: String,
    #[serde(rename = "user_code")]
    pub user_code: String,
    #[serde(rename = "verification_uri")]
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: i64,
}

pub fn microsoft_device_code(client_id: &str) -> Result<DeviceCode> {
    let resp = ureq::post(MS_DEVICECODE)
        .send_form(&[
            ("client_id", client_id),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .context("microsoft devicecode")?;
    Ok(resp.into_json()?)
}

#[derive(Debug, Deserialize)]
pub struct MsToken {
    #[serde(rename = "access_token", default)]
    pub access_token: String,
    #[serde(rename = "refresh_token", default)]
    pub refresh_token: String,
    #[serde(rename = "error", default)]
    pub error: Option<String>,
}

pub fn microsoft_poll_token(client_id: &str, device_code: &str) -> Result<MsToken> {
    let resp = ureq::post(MS_TOKEN).send_form(&[
        ("client_id", client_id),
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ("device_code", device_code),
    ])?;
    Ok(resp.into_json()?)
}

#[derive(Debug, Deserialize)]
struct XblResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: XblClaims,
}

#[derive(Debug, Deserialize)]
struct XblClaims {
    xui: Vec<XblXui>,
}

#[derive(Debug, Deserialize)]
struct XblXui {
    uhs: String,
}

fn xbox_live_auth(ms_access_token: &str) -> Result<(String, String)> {
    let body = serde_json::json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            "RpsTicket": format!("d={ms_access_token}")
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT"
    });
    let resp = ureq::post(XBL_AUTH)
        .send_json(&body)
        .context("xbox live auth")?;
    let r: XblResponse = resp.into_json()?;
    let uhs = r
        .display_claims
        .xui
        .first()
        .context("no xui in xbl response")?
        .uhs
        .clone();
    Ok((r.token, uhs))
}

fn xsts_authorize(xbl_token: &str) -> Result<String> {
    let body = serde_json::json!({
        "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] },
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT"
    });
    let resp = ureq::post(XSTS_AUTH)
        .send_json(&body)
        .context("xsts authorize")?;
    let v: serde_json::Value = resp.into_json()?;
    v.get("Token")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .context("no XSTS token")
}

#[derive(Debug, Deserialize)]
struct McLoginResponse {
    #[serde(rename = "access_token")]
    access_token: String,
}

fn minecraft_login(uhs: &str, xsts_token: &str) -> Result<String> {
    let body = serde_json::json!({
        "identityToken": format!("XBL3.0 x={uhs};{xsts_token}")
    });
    let resp = ureq::post(MC_LOGIN)
        .send_json(&body)
        .context("minecraft login_with_xbox")?;
    let r: McLoginResponse = resp.into_json()?;
    Ok(r.access_token)
}

/// 正版账号进服：向官方 session 服务器声明 join（服务器会回 hasJoined 校验）。
pub fn microsoft_join(access_token: &str, selected_profile: &str, server_id: &str) -> Result<()> {
    let body = serde_json::json!({
        "accessToken": access_token,
        "selectedProfile": selected_profile,
        "serverId": server_id,
    });
    ureq::post(MC_SESSION_JOIN)
        .send_json(&body)
        .context("minecraft session join")?;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct McProfile {
    pub id: String,
    pub name: String,
}

pub fn minecraft_profile(mc_token: &str) -> Result<McProfile> {
    let resp = ureq::get(MC_PROFILE)
        .set("Authorization", &format!("Bearer {mc_token}"))
        .call()
        .context("minecraft profile")?;
    Ok(resp.into_json()?)
}

/// 用已有的 MSA access token 走完整 Xbox 链，得到 Minecraft 账号。
pub fn microsoft_login_with_msa(client_id: &str, ms_access_token: &str) -> Result<Account> {
    let (_xbl, uhs) = xbox_live_auth(ms_access_token)?;
    let xsts = xsts_authorize(&_xbl)?;
    let mc = minecraft_login(&uhs, &xsts)?;
    let profile = minecraft_profile(&mc)?;
    let uuid = Uuid::parse_str(&profile.id).unwrap_or_else(|_| offline_uuid(&profile.name));
    Ok(Account {
        name: profile.name,
        uuid,
        access_token: mc,
        kind: AuthKind::Microsoft {
            client_id: client_id.to_string(),
        },
    })
}

/// 刷新微软账号（refresh token → 新的 MSA access token）。
pub fn microsoft_refresh(client_id: &str, refresh_token: &str) -> Result<MsToken> {
    let resp = ureq::post(MS_TOKEN).send_form(&[
        ("client_id", client_id),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
        ("scope", "XboxLive.signin offline_access"),
    ])?;
    let t: MsToken = resp.into_json()?;
    if t.access_token.is_empty() {
        bail!("microsoft refresh failed: {:?}", t.error);
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_java() {
        // Java: UUID.nameUUIDFromBytes("OfflinePlayer:Notch".getBytes())
        assert_eq!(
            offline_uuid("Notch").to_string(),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
        assert_eq!(offline_uuid("Notch").get_version(), Some(uuid::Version::Md5));
    }
}
