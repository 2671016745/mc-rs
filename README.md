# mc-rs

Minecraft Java Edition **1.21.1** 客户端本体的 Rust 重写（目标平台 Windows，兼容 Linux 开发/验证）。

当前阶段：协议/登录/mod 运行时已跑通；渲染与游戏循环待建。

## 已实现

- `assets fetch`：从 Mojang launchermeta 拉取版本清单，下载客户端/服务端 jar 并校验 sha1。
- `assets index`：索引客户端 jar 的资产与数据（方块状态、模型、纹理、语言、标签、配方、战利品表、世界生成）。
- `registry`：从 `blockstates` 构建方块注册表（变体、multipart、模型引用）。
- `protocol`：1.21.1（protocol 767）VarInt/VarLong/字符串/坐标/组帧，含官方测试向量。
- `net`：TCP 帧、服务器状态 ping（已实测 Hypixel/CubeCraft）、离线登录握手（已用 mock 服务器端到端验证）、RSA + AES-128-CFB8 加密（openssl 向量验证）。
- `auth`：离线（Java 同款 MD5 UUID）、第三方 Yggdrasil、微软 device code → XBL → XSTS → MC 完整链。
- `skin`：正版纹理属性解码（session server）、离线皮肤 PNG 读取。
- `server`：服务器列表持久化 + ping。
- `modsys`：解析 `neoforge.mods.toml`（清单/依赖检查）。
- `modruntime`：自研 mod 运行时（`mcmod.toml` + Lua 5.4，`mc.log/register_block` API）。

## 实测数据（1.21.1 官方客户端 jar）

- 条目 23519，解压后 48.2 MB
- blockstates 1062（变体 6533，multipart 70，模型引用 2181）
- models/block 2137，models/item 1754，textures 3167
- lang：仅 `en_us.json`（6881 条）；其他语言需另行获取
- tags：block 184（1833 条目）、item 147（924 条目）
- recipe 1290，loot_table 1178，worldgen 896，dimension_type 4

## 构建

```bash
cargo build
cargo test
```

## 使用

```bash
cargo run -- fetch --version 1.21.1 --kind client --out client.jar
cargo run -- index client.jar --out index.json
cargo run -- registry client.jar --out registry.json
cargo run -- mod path/to/neoforge.mods.toml
cargo run -- auth-offline Steve
cargo run -- auth-yggdrasil https://authserver.example.com <username>
cargo run -- auth-microsoft-device --client-id <client-id>
cargo run -- servers add "My Server" mc.example.com --port 25565
cargo run -- servers ping
cargo run -- skin-fetch <uuid>
cargo run -- login-offline 127.0.0.1 --port 25565 Steve
cargo run -- login-play 127.0.0.1 --port 25565 Steve
cargo run -- mods ./mods
cargo run -- render
```
