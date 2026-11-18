//! TCP 连接、帧读写与服务器状态 ping（1.21.1 协议）。

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use crate::protocol::{self, login, PROTOCOL, STATE_LOGIN, STATE_STATUS};

pub fn connect(address: &str, port: u16) -> Result<TcpStream> {
    let addr = format!("{address}:{port}");
    let stream = TcpStream::connect(&addr).with_context(|| format!("connect {addr}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(15)))?;
    Ok(stream)
}

fn read_varint_stream<S: Read>(s: &mut S) -> Result<i32> {
    let mut value = 0u32;
    let mut shift = 0u32;
    for _ in 0..5 {
        let mut b = [0u8; 1];
        s.read_exact(&mut b)?;
        value |= ((b[0] & 0x7F) as u32) << shift;
        if b[0] & 0x80 == 0 {
            return Ok(value as i32);
        }
        shift += 7;
    }
    bail!("varint too long")
}

pub fn write_frame<S: Write>(s: &mut S, packet_id: i32, body: &[u8]) -> Result<()> {
    let framed = protocol::frame(packet_id, body);
    s.write_all(&framed)?;
    s.flush()?;
    Ok(())
}

pub fn read_frame<S: Read>(s: &mut S) -> Result<(i32, Vec<u8>)> {
    let len = read_varint_stream(s)?;
    if len <= 0 || len > 8 * 1024 * 1024 {
        bail!("bad frame length {len}");
    }
    let mut buf = vec![0u8; len as usize];
    s.read_exact(&mut buf)?;
    let (id, n) = protocol::get_varint(&buf).context("packet id")?;
    Ok((id, buf[n..].to_vec()))
}

pub fn write_handshake(s: &mut TcpStream, address: &str, port: u16, next_state: i32) -> Result<()> {
    let mut body = Vec::new();
    protocol::put_varint(&mut body, PROTOCOL);
    protocol::put_string(&mut body, address);
    protocol::put_u16(&mut body, port);
    protocol::put_varint(&mut body, next_state);
    write_frame(s, 0x00, &body)
}

#[derive(Debug, Deserialize, Default)]
pub struct Players {
    #[serde(default)]
    pub max: i32,
    #[serde(default)]
    pub online: i32,
}

#[derive(Debug, Deserialize, Default)]
pub struct Version {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub protocol: i32,
}

#[derive(Debug, Deserialize, Default)]
pub struct ServerStatus {
    #[serde(default)]
    pub description: serde_json::Value,
    #[serde(default)]
    pub players: Option<Players>,
    #[serde(default)]
    pub version: Option<Version>,
    #[serde(default)]
    pub favicon: Option<String>,
    #[serde(default)]
    pub enforces_secure_chat: Option<bool>,
}

impl ServerStatus {
    pub fn motd_text(&self) -> String {
        json_to_text(&self.description)
    }
}

/// 把 MOTD 的 JSON（text/extra 结构）拍平成纯文本。
fn json_to_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(o) => {
            let mut out = o.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
            if let Some(serde_json::Value::Array(extra)) = o.get("extra") {
                for e in extra {
                    out.push_str(&json_to_text(e));
                }
            }
            out
        }
        serde_json::Value::Array(a) => a.iter().map(json_to_text).collect(),
        _ => String::new(),
    }
}

/// 服务器列表 ping：返回状态 JSON 与往返延迟（毫秒）。
pub fn ping(address: &str, port: u16) -> Result<(ServerStatus, u128)> {
    let mut s = connect(address, port)?;
    let t0 = Instant::now();
    write_handshake(&mut s, address, port, STATE_STATUS)?;
    write_frame(&mut s, protocol::status::REQUEST, &[])?;
    let (id, body) = read_frame(&mut s)?;
    if id != protocol::status::RESPONSE {
        bail!("expected status response 0x00, got 0x{id:02x}");
    }
    let mut r = protocol::PacketReader::new(&body);
    let json = r.string().context("status json string")?;
    let status: ServerStatus = serde_json::from_str(&json).context("status json")?;

    let payload = 0x0123_4567_89AB_CDEFu64;
    write_frame(&mut s, protocol::status::PING, &payload.to_be_bytes())?;
    let (id2, _) = read_frame(&mut s)?;
    let rtt = t0.elapsed().as_millis();
    if id2 != protocol::status::PONG {
        bail!("expected pong 0x01, got 0x{id2:02x}");
    }
    Ok((status, rtt))
}

// ---------------------------------------------------------------- 登录加密

use aes::cipher::{BlockCipherEncrypt, KeyInit};
use aes::Aes128;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey};

/// 自实现 AES-128-CFB8 流（与 Minecraft 的 AES/CFB8/NoPadding 一致）。
pub struct Cfb8Stream {
    cipher: Aes128,
    feedback: [u8; 16],
    encrypt: bool,
}

impl Cfb8Stream {
    pub fn new(key: &[u8], iv: &[u8], encrypt: bool) -> Result<Self> {
        let cipher = Aes128::new_from_slice(key).map_err(|e| anyhow::anyhow!("{e}"))?;
        let feedback: [u8; 16] = iv.try_into().map_err(|_| anyhow::anyhow!("bad iv length"))?;
        Ok(Self {
            cipher,
            feedback,
            encrypt,
        })
    }

    /// 加/解密同一运算，feedback 用密文。
    pub fn apply(&mut self, data: &mut [u8]) {
        for b in data.iter_mut() {
            let mut block = aes::cipher::Block::<Aes128>::default();
            block.copy_from_slice(&self.feedback);
            self.cipher.encrypt_block(&mut block);
            let inb = *b;
            let outb = inb ^ block[0];
            self.feedback.rotate_left(1);
            self.feedback[15] = if self.encrypt { outb } else { inb };
            *b = outb;
        }
    }
}

/// AES-128-CFB8 加密流（Minecraft 联机加密）。
pub struct EncryptedStream {
    inner: TcpStream,
    enc: Cfb8Stream,
    dec: Cfb8Stream,
}

impl EncryptedStream {
    pub fn new(inner: TcpStream, shared_secret: &[u8]) -> Result<Self> {
        Ok(Self {
            inner,
            enc: Cfb8Stream::new(shared_secret, shared_secret, true)?,
            dec: Cfb8Stream::new(shared_secret, shared_secret, false)?,
        })
    }
}

impl Read for EncryptedStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.dec.apply(&mut buf[..n]);
        Ok(n)
    }
}

impl Write for EncryptedStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut tmp = buf.to_vec();
        self.enc.apply(&mut tmp);
        self.inner.write_all(&tmp)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// 用服务器公钥（X.509 DER）做 PKCS#1 v1.5 加密。
pub fn encrypt_with_server_key(der: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    use rsa::pkcs8::DecodePublicKey;
    let key = RsaPublicKey::from_public_key_der(der)?;
    let mut rng = rand::thread_rng();
    Ok(key.encrypt(&mut rng, Pkcs1v15Encrypt, data)?)
}

/// 生成 16 字节共享密钥并加密（shared secret + verify token）。
pub fn encryption_response(
    der_key: &[u8],
    verify_token: &[u8],
) -> Result<(Vec<u8>, Vec<u8>, [u8; 16])> {
    use rand::RngCore;
    let mut secret = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut secret);
    let enc_secret = encrypt_with_server_key(der_key, &secret)?;
    let enc_verify = encrypt_with_server_key(der_key, verify_token)?;
    Ok((enc_secret, enc_verify, secret))
}

/// 编码 Login Start（1.20.2+ 含 UUID）。
pub fn login_start_body(name: &str, uuid: uuid::Uuid) -> Vec<u8> {
    let mut body = Vec::new();
    protocol::put_string(&mut body, name);
    body.extend_from_slice(uuid.as_bytes());
    body
}

/// 编码 Encryption Response（两个 ByteArray）。
pub fn encryption_response_body(enc_secret: &[u8], enc_verify: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    protocol::put_varint(&mut body, enc_secret.len() as i32);
    body.extend_from_slice(enc_secret);
    protocol::put_varint(&mut body, enc_verify.len() as i32);
    body.extend_from_slice(enc_verify);
    body
}

/// 解析服务器 Encryption Request。
pub struct EncryptionRequest {
    pub server_id: String,
    pub public_key: Vec<u8>,
    pub verify_token: Vec<u8>,
}

pub fn parse_encryption_request(body: &[u8]) -> Result<EncryptionRequest> {
    let mut r = protocol::PacketReader::new(body);
    let server_id = r.string().context("server id")?;
    let klen = r.varint().context("pubkey len")? as usize;
    let public_key = r.bytes(klen).context("pubkey")?.to_vec();
    let tlen = r.varint().context("token len")? as usize;
    let verify_token = r.bytes(tlen).context("token")?.to_vec();
    Ok(EncryptionRequest {
        server_id,
        public_key,
        verify_token,
    })
}

// ---------------------------------------------------------------- 登录流程

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;

fn varint_len(v: i32) -> i32 {
    let mut n = 0;
    let mut x = v as u32;
    loop {
        n += 1;
        if (x & !0x7F) == 0 {
            break;
        }
        x >>= 7;
    }
    n
}

/// 写压缩帧（Set Compression 之后）。
pub fn write_frame_c<S: Write>(
    s: &mut S,
    packet_id: i32,
    body: &[u8],
    threshold: i32,
) -> Result<()> {
    let mut data = Vec::with_capacity(body.len() + 5);
    protocol::put_varint(&mut data, packet_id);
    data.extend_from_slice(body);

    let mut out = Vec::new();
    if threshold >= 0 && (data.len() as i32) >= threshold {
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&data)?;
        let compressed = enc.finish()?;
        let dl = compressed.len() as i32 + varint_len(data.len() as i32);
        protocol::put_varint(&mut out, dl);
        protocol::put_varint(&mut out, data.len() as i32);
        out.extend_from_slice(&compressed);
    } else {
        protocol::put_varint(&mut out, data.len() as i32 + 1);
        protocol::put_varint(&mut out, 0);
        out.extend_from_slice(&data);
    }
    s.write_all(&out)?;
    s.flush()?;
    Ok(())
}

/// 读压缩帧。
pub fn read_frame_c<S: Read>(s: &mut S, _threshold: i32) -> Result<(i32, Vec<u8>)> {
    let len = read_varint_stream(s)?;
    if len <= 0 || len > 8 * 1024 * 1024 {
        bail!("bad frame length {len}");
    }
    let mut buf = vec![0u8; len as usize];
    s.read_exact(&mut buf)?;
    let (data_len, n) = protocol::get_varint(&buf).context("data length")?;
    let payload = &buf[n..];
    let data = if data_len > 0 {
        let mut out = Vec::new();
        ZlibDecoder::new(payload).read_to_end(&mut out)?;
        out
    } else {
        payload.to_vec()
    };
    let (id, m) = protocol::get_varint(&data).context("packet id")?;
    Ok((id, data[m..].to_vec()))
}

#[derive(Debug, serde::Serialize)]
pub struct LoginSuccess {
    pub uuid: uuid::Uuid,
    pub name: String,
    pub property_count: i32,
}

/// 离线模式登录：握手 → Login Start → （Set Compression）→ Login Success → Ack。
pub fn login_offline(address: &str, port: u16, name: &str) -> Result<LoginSuccess> {
    let account = crate::auth::offline(name);
    let mut s = connect(address, port)?;
    write_handshake(&mut s, address, port, STATE_LOGIN)?;
    write_frame(&mut s, login::START, &login_start_body(&account.name, account.uuid))?;

    let mut threshold = -1i32;
    loop {
        let (id, body) = if threshold >= 0 {
            read_frame_c(&mut s, threshold)?
        } else {
            read_frame(&mut s)?
        };
        match id {
            0x00 => {
                let mut r = protocol::PacketReader::new(&body);
                let reason = r.string().unwrap_or_default();
                bail!("login disconnect: {reason}");
            }
            0x03 => {
                let mut r = protocol::PacketReader::new(&body);
                threshold = r.varint().unwrap_or(-1);
                eprintln!("compression threshold = {threshold}");
            }
            0x02 => {
                let mut r = protocol::PacketReader::new(&body);
                let uuid = r.uuid().context("login success uuid")?;
                let uname = r.string().context("login success name")?;
                let property_count = r.varint().unwrap_or(0);
                write_frame_c(&mut s, login::LOGIN_ACKNOWLEDGED, &[], threshold)?;
                return Ok(LoginSuccess {
                    uuid,
                    name: uname,
                    property_count,
                });
            }
            other => bail!("unexpected login packet 0x{other:02x}"),
        }
    }
}

// ---------------------------------------------------------------- 完整进服

#[derive(Debug, serde::Serialize)]
pub struct PlayLogin {
    pub entity_id: i32,
    pub hardcore: bool,
    pub dimensions: Vec<String>,
    pub max_players: i32,
    pub view_distance: i32,
    pub simulation_distance: i32,
    pub reduced_debug_info: bool,
    pub enable_respawn_screen: bool,
    pub do_limited_crafting: bool,
    pub dimension_type: i32,
    pub dimension_name: String,
    pub hashed_seed: i64,
    pub game_mode: u8,
    pub previous_game_mode: i8,
    pub is_debug: bool,
    pub is_flat: bool,
    pub has_death_location: bool,
    pub death_dimension: Option<String>,
    pub death_location: Option<(i32, i32, i32)>,
    pub portal_cooldown: i32,
    pub enforces_secure_chat: bool,
    pub registry_packets: usize,
    pub chunks_loaded: usize,
    pub non_air_blocks: usize,
    /// 服务器 ID 与其 SHA1 会话哈希（第三方/正版进服时用于 join）
    pub server_id: String,
    pub server_id_hash: String,
}

fn parse_login_play(body: &[u8]) -> Result<PlayLogin> {
    let mut r = protocol::PacketReader::new(body);
    let entity_id = r.i32().context("entity id")?;
    let hardcore = r.boolean().unwrap_or(false);
    let dim_count = r.varint().context("dimension count")? as usize;
    let mut dimensions = Vec::with_capacity(dim_count);
    for _ in 0..dim_count {
        dimensions.push(r.string().context("dimension name")?);
    }
    let max_players = r.varint().context("max players")?;
    let view_distance = r.varint().context("view distance")?;
    let simulation_distance = r.varint().context("simulation distance")?;
    let reduced_debug_info = r.boolean().unwrap_or(false);
    let enable_respawn_screen = r.boolean().unwrap_or(true);
    let do_limited_crafting = r.boolean().unwrap_or(false);
    let dimension_type = r.varint().context("dimension type")?;
    let dimension_name = r.string().context("dimension name")?;
    let hashed_seed = r.i64().context("hashed seed")?;
    let game_mode = r.u8().unwrap_or(0);
    let previous_game_mode = r.i8().unwrap_or(-1);
    let is_debug = r.boolean().unwrap_or(false);
    let is_flat = r.boolean().unwrap_or(false);
    let has_death_location = r.boolean().unwrap_or(false);
    let mut death_dimension = None;
    let mut death_location = None;
    if has_death_location {
        death_dimension = Some(r.string().context("death dimension")?);
        death_location = r.position();
    }
    let portal_cooldown = r.varint().context("portal cooldown")?;
    let enforces_secure_chat = r.boolean().unwrap_or(false);
    Ok(PlayLogin {
        entity_id,
        hardcore,
        dimensions,
        max_players,
        view_distance,
        simulation_distance,
        reduced_debug_info,
        enable_respawn_screen,
        do_limited_crafting,
        dimension_type,
        dimension_name,
        hashed_seed,
        game_mode,
        previous_game_mode,
        is_debug,
        is_flat,
        has_death_location,
        death_dimension,
        death_location,
        portal_cooldown,
        enforces_secure_chat,
        registry_packets: 0,
        chunks_loaded: 0,
        non_air_blocks: 0,
        server_id: String::new(),
        server_id_hash: String::new(),
    })
}

/// 登录传输层：明文 TCP 或 AES-CFB8 加密流。
pub enum Tx {
    Plain(TcpStream),
    Enc(EncryptedStream),
}

impl Tx {
    fn read(&mut self, comp: bool, threshold: i32) -> Result<(i32, Vec<u8>)> {
        match self {
            Tx::Plain(s) => {
                if comp {
                    read_frame_c(s, threshold)
                } else {
                    read_frame(s)
                }
            }
            Tx::Enc(s) => {
                if comp {
                    read_frame_c(s, threshold)
                } else {
                    read_frame(s)
                }
            }
        }
    }

    fn write(&mut self, id: i32, body: &[u8], comp: bool, threshold: i32) -> Result<()> {
        match self {
            Tx::Plain(s) => {
                if comp {
                    write_frame_c(s, id, body, threshold)
                } else {
                    write_frame(s, id, body)
                }
            }
            Tx::Enc(s) => {
                if comp {
                    write_frame_c(s, id, body, threshold)
                } else {
                    write_frame(s, id, body)
                }
            }
        }
    }
}

/// 完整进服：登录（含加密）→ configuration 协商 → Play 的 Login 包。
///
/// 与 `login_offline` 的区别：本函数完成 configuration 阶段（Client Information、
/// Known Packs、Registry/Tags、Finish Configuration），进入 Play 并解析 Login(play)；
/// 服务器在登录阶段下发 Encryption Request 时启用 AES-CFB8 加密流。
pub fn login_play(
    address: &str,
    port: u16,
    account: &crate::auth::Account,
) -> Result<PlayLogin> {
    let mut s = connect(address, port)?;
    write_handshake(&mut s, address, port, STATE_LOGIN)?;
    write_frame(
        &mut s,
        login::START,
        &login_start_body(&account.name, account.uuid),
    )?;
    let mut tx = Tx::Plain(s);

    // ---- Login 阶段
    let mut threshold = -1i32;
    let mut comp = false;
    let mut server_id = String::new();
    let mut server_id_hash = String::new();
    loop {
        let (id, body) = tx.read(comp, threshold)?;
        match id {
            0x00 => bail!("login disconnect"),
            0x01 => {
                let req = parse_encryption_request(&body)?;
                let (enc_secret, enc_verify, secret) =
                    encryption_response(&req.public_key, &req.verify_token)?;
                server_id_hash =
                    crate::auth::server_id_hash(&req.server_id, &secret, &req.public_key);
                server_id = req.server_id.clone();
                tx.write(
                    0x01,
                    &encryption_response_body(&enc_secret, &enc_verify),
                    comp,
                    threshold,
                )?;
                if let Tx::Plain(inner) = tx {
                    tx = Tx::Enc(EncryptedStream::new(inner, &secret)?);
                }
            }
            0x03 => {
                threshold = protocol::PacketReader::new(&body)
                    .varint()
                    .unwrap_or(-1);
                comp = true;
            }
            0x02 => {
                tx.write(login::LOGIN_ACKNOWLEDGED, &[], comp, threshold)?;
                break;
            }
            other => bail!("unexpected login packet 0x{other:02x}"),
        }
    }

    // ---- Configuration 阶段
    let mut ci = Vec::new();
    protocol::put_string(&mut ci, "zh_CN");
    protocol::put_i8(&mut ci, 6);
    protocol::put_varint(&mut ci, 0);
    protocol::put_bool(&mut ci, true);
    protocol::put_u8(&mut ci, 0xFF);
    protocol::put_varint(&mut ci, 1);
    protocol::put_bool(&mut ci, false);
    protocol::put_bool(&mut ci, true);
    tx.write(0x00, &ci, comp, threshold)?;

    let mut known = Vec::new();
    protocol::put_varint(&mut known, 1);
    protocol::put_string(&mut known, "minecraft");
    protocol::put_string(&mut known, "core");
    protocol::put_string(&mut known, "1.21");

    let mut registry_packets = 0usize;
    loop {
        let (id, body) = tx.read(comp, threshold)?;
        match id {
            0x03 => {
                tx.write(0x03, &[], comp, threshold)?;
                break;
            }
            0x04 => tx.write(0x04, &body, comp, threshold)?,
            0x05 => tx.write(0x05, &body, comp, threshold)?,
            0x07 => registry_packets += 1,
            0x0e => tx.write(0x07, &known, comp, threshold)?,
            0x02 => {
                let mut r = protocol::PacketReader::new(&body);
                let reason = r.string().unwrap_or_default();
                bail!("configuration disconnect: {reason}");
            }
            _ => {}
        }
    }

    // ---- Play 阶段：先拿 Login(play)，再收第一个区块验证地形解析
    let mut info: Option<PlayLogin> = None;
    let mut chunks = 0usize;
    let mut non_air = 0usize;
    while info.is_none() || chunks == 0 {
        let (id, body) = tx.read(comp, threshold)?;
        match id {
            0x2b => {
                let mut i = parse_login_play(&body)?;
                i.registry_packets = registry_packets;
                info = Some(i);
            }
            0x27 => match crate::world::parse_chunk_data(&body) {
                Ok(ch) => {
                    non_air += ch.non_air();
                    chunks += 1;
                    let m = crate::mesh::mesh_chunk(&ch);
                    eprintln!(
                        "chunk {},{} sections={} non_air={} triangles={}",
                        ch.x,
                        ch.z,
                        ch.sections.len(),
                        ch.non_air(),
                        m.triangles()
                    );
                }
                Err(e) => eprintln!("chunk parse error: {e}"),
            },
            _ => {}
        }
    }
    let mut i = info.context("no login play packet")?;
    i.chunks_loaded = chunks;
    i.non_air_blocks = non_air;
    i.server_id = server_id;
    i.server_id_hash = server_id_hash;
    Ok(i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motd_flattens() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"text":"A ","extra":[{"text":"BMC","color":"red"},{"text":"!"}]}"#,
        )
        .unwrap();
        assert_eq!(json_to_text(&v), "A BMC!");
    }

    #[test]
    fn aes_cfb8_openssl_vector() {
        let key: [u8; 16] = std::array::from_fn(|i| i as u8);
        let iv = [0u8; 16];
        let mut data: Vec<u8> = (0u8..16).collect();
        Cfb8Stream::new(&key, &iv, true)
            .unwrap()
            .apply(&mut data);
        assert_eq!(
            data,
            vec![0xc6, 0x41, 0x37, 0xd8, 0x6f, 0x65, 0x64, 0x1d, 0x2b, 0xa0, 0xad, 0xee, 0x67, 0xba, 0x9b, 0x5f],
            "openssl aes-128-cfb8 vector"
        );
        let mut dec = Cfb8Stream::new(&key, &iv, false).unwrap();
        dec.apply(&mut data);
        assert_eq!(data, (0u8..16).collect::<Vec<u8>>());
    }

    #[test]
    fn rsa_roundtrip() {
        use rsa::pkcs8::{DecodePublicKey, EncodePublicKey};
        use rsa::RsaPrivateKey;
        let mut rng = rand::thread_rng();
        let priv_key = RsaPrivateKey::new(&mut rng, 1024).unwrap();
        let der = priv_key.to_public_key().to_public_key_der().unwrap();
        let msg = b"shared secret";
        let ct = encrypt_with_server_key(der.as_bytes(), msg).unwrap();
        let pt = priv_key.decrypt(Pkcs1v15Encrypt, &ct).unwrap();
        assert_eq!(pt, msg);
    }
}
