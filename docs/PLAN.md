# mc-rs 工程计划

## 目标

用 Rust 重写 Minecraft Java Edition **1.21.1** 客户端本体（目标平台 Windows），
并支持用户要求的全部功能：

- 正版验证（微软账号登录）
- 离线登录（直接用用户名进入）
- 第三方登录（Yggdrasil 兼容外置登录，authlib 风格）
- 服务器：多人联机、服务器列表/收藏
- 皮肤：正版皮肤、离线/自定义皮肤显示
- 模组功能（自研 mod 系统）
- 版本矩阵：先 1.21.1 + NeoForge，后续按「游戏版本 × 加载器版本」扩展

## 关键技术结论

**NeoForge/Forge/Fabric 的 mod 是 Java 字节码**：`META-INF/neoforge.mods.toml`
清单 + `@Mod` 入口类 + Mixin/事件总线，运行在 JVM 上，并 patch Minecraft 客户端本体
（Mojang 官方映射）。Rust 本体无法直接加载这些 mod。

因此 mod 功能采用业界 Rust 重写项目的通行做法（参考 RustCraft、FPSMaster、Horizon）：

1. **管理面**：解析 NeoForge 清单（已可做），负责发现/依赖检查/启停/冲突提示；
2. **加载面**：自研 mod 运行时（Lua 脚本 + Rust `cdylib` 原生扩展），提供
   方块/物品注册、渲染钩子、事件、配置、HUD 等 API。

## 架构模块

| 模块 | 状态 | 说明 |
|---|---|---|
| `assets` | ✅ 基础完成 | 版本清单下载、sha1 校验、jar 资产/数据索引 |
| `registry` | ✅ 基础完成 | blockstates → 方块注册表（变体/multipart/模型） |
| `protocol` | ✅ 基础完成 | VarInt/VarLong/字符串/组帧（protocol 767） |
| `modsys` | 🚧 管理面完成 | 清单解析、依赖检查；运行时待实现 |
| `auth` | ⬜ | 离线 / 第三方 Yggdrasil / 微软 OAuth |
| `skin` | ⬜ | 皮肤与披风加载、离线自定义皮肤 |
| `net` | ⬜ | 服务器 ping、状态、登录握手、加密 |
| `render` | ⬜ | wgpu 渲染、区块 meshing、模型/纹理 |
| `game` | ⬜ | 世界/实体/玩法循环 |

## 发布策略

- 重写阶段：本地推进，达到可运行里程碑后新建 GitHub 仓库。
- 发行版：GitHub Releases，tag 用版本号（`v0.1.0` 起），按版本号排列。
- 构建：GitHub Actions 交叉编译 `x86_64-pc-windows-msvc`（容器内无 Windows 工具链）。
- 每个里程碑发布后清理本地构建缓存/下载产物（控制磁盘占用）。

## 当前进度

已完成：项目骨架、资产管线（实测 1.21.1：1062 方块 / 6533 变体 / 1290 配方 /
1178 战利品表 / 184 方块标签）、协议基础编解码、mod 清单解析与依赖检查。
`cargo test` 5 项通过。

下一步：网络协议（握手/登录/加密）→ auth（离线/第三方/正版）→ skin。
