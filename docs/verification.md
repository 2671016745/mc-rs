# 验证记录（2026-11-16）

## 单元测试

`cargo test`：13 项通过。

覆盖：VarInt/VarLong（官方向量）、字符串/组帧、坐标编解码、MOTD 拍平、
AES-128-CFB8（openssl 向量）、RSA PKCS#1 v1.5 roundtrip、
离线 UUID（与 Java `UUID.nameUUIDFromBytes` 一致）、皮肤纹理解码、
服务器去重、mod 清单解析（NeoForge + Fabric）、Lua mod 加载。

## 真实服务器 ping

`servers add` + `servers ping`（走 1.21.1 协议 767）：

- Hypixel `mc.hypixel.net`：~991ms，22384/200000
- CubeCraft `play.cubecraft.net`：~269ms，1043/5000

## 登录握手（本地 mock 服务器）

用 Python mock 服务器（未压缩 Set Compression + 压缩 Login Success）验证：

- 客户端发出 handshake + Login Start（name=TestPlayer, uuid=bb77495aa7403169a23869654c8bd2c1）
- 服务端下发 Set Compression(256) 与 Login Success
- 客户端回 Login Acknowledged(0x03)
- 结果：完整闭环，mock 侧打印 `ack packet 3 ... done`

## 加密向量

- AES-128-CFB8：key=000102…0f，iv=0，pt=000102…0f →
  ct=`c64137d86f65641d2ba0adee67ba9b5f`（openssl `enc -aes-128-cfb8`）
- RSA：1024 位密钥对，PKCS#1 v1.5 加解密 roundtrip

## 资产基准（1.21.1 官方客户端 jar，sha1 校验通过）

23519 条目 / 48.2 MB；blockstates 1062（变体 6533、multipart 70、模型引用 2181）；
recipe 1290；loot_table 1178；worldgen 896；block tags 184；en_us 6881 条。

## 已知限制

- 渲染（`render`）需要真实 GPU/显示环境；本容器无 GPU，仅验证编译通过。
- 微软/第三方登录的完整链路需要真实账号与认证服务器，未做端到端验证。
