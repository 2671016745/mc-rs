# mc-rs 架构

## 分层

```
CLI (main.rs)
 ├─ assets    官方资源获取/校验/索引
 ├─ registry  方块注册表（blockstates → 定义）
 ├─ protocol  协议编解码（VarInt/Reader/位置）
 ├─ net       连接、帧、ping、登录加密流
 ├─ auth      离线 / Yggdrasil / 微软
 ├─ skin      皮肤纹理属性与离线皮肤
 ├─ server    服务器列表与 ping
 ├─ modsys    mod 清单、依赖、启停
 ├─ render    wgpu 渲染（待建）
 └─ game      世界/实体/玩法循环（待建）
```

## 数据流（启动到进入游戏）

1. `assets fetch`：版本清单 → 客户端 jar → sha1 校验。
2. `assets index` / `registry`：建立资源索引与方块注册表。
3. `auth`：选择登录方式，得到 `(name, uuid, access_token)`。
4. `net`：TCP 连接 → 握手(nextState=2) → Login Start → Encryption Request/Response
   （RSA + AES-128-CFB8）→ Set Compression → Login Success → Login Acknowledged。
5. Play 状态：绑定/出生点、区块、实体、皮肤。
6. `render`：wgpu 渲染世界与 UI。

## 登录方式

| 方式 | 实现 | 状态 |
|---|---|---|
| 离线 | MD5("OfflinePlayer:"+name) 生成 UUID | ✅ |
| 第三方 | Yggdrasil authenticate/refresh/validate | ✅ |
| 微软 | device code → MSA → XBL → XSTS → MC → profile | ✅ |

## 加密

- 密钥交换：服务器公钥 X.509 DER + PKCS#1 v1.5。
- 流加密：AES-128-CFB8，key = iv = 16 字节共享密钥；读/写各一个独立 cipher 实例。
- 压缩：Set Compression 后启用 zlib（后续）。

## mod 系统

- 管理面：解析 `neoforge.mods.toml`，依赖检查、启停、冲突。
- 加载面（自研）：Lua 脚本 + Rust `cdylib`，提供注册/事件/渲染钩子 API。
- 原因：NeoForge/Fabric mod 是 Java 字节码，依赖 JVM 与 Mojang 映射，无法被
  Rust 本体直接加载（见 `docs/java-mod-bridge.md`）。
