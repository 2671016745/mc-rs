# Fabric × Minecraft 1.21 技术调研（mc-rs 第二阶段）

> 目标：为 mc-rs（Rust 重写 Minecraft Java 客户端）下一阶段「1.21 + Fabric」提供版本事实、
> 加载机制差异与可复用的 API 接口。数据来自 FabricMC / NeoForged / Mojang 官方接口与文档，
> 快照时间 **2026-11-16**，文末附来源链接。
>
> 关联文档：[`docs/version-matrix.md`](version-matrix.md)、[`docs/PLAN.md`](PLAN.md)。

---

## 0. 结论速览

- **1.21 与 1.21.1 共享协议号 767**，数据包/资源包格式也相同（48 / 34）。对客户端重写而言，
  两者几乎可以共用同一套网络协议、资源管线与注册表解析代码，差异只在数据版本（3953 → 3955）
  带来的少量注册表内容变化。
- **Fabric 侧版本（1.21 / 1.21.1）**：
  - Fabric Loader：1.21 发布时官方推荐 **0.15.11**；当前（2026-11）最新稳定 **0.20.1**，仍兼容 1.21。
  - Fabric API：**0.102.0+1.21** / **0.116.18+1.21.1**（版本号后缀即游戏版本）。
  - Intermediary：**1.21** / **1.21.1**（与游戏版本同名，一一对应）。
  - Yarn：**1.21+build.9** / **1.21.1+build.3**（各自最新构建；`stable` 为全局标记，见 §3 说明）。
- **Fabric 与 NeoForge 的 mod 都是 JVM 字节码**，Rust 本体都无法直接加载。mc-rs 的路线不变：
  管理面解析两种清单（`fabric.mod.json` / `neoforge.mods.toml`），加载面走自研运行时。
- 第二阶段建议直接以 **1.21.1 为协议/资源基线**（仓库现有 `protocol.rs` 即 protocol 767），
  1.21 作为同协议兼容目标，避免重复实现。

---

## 1. 版本对应关系

### 1.1 游戏版本事实

| 游戏版本 | 发布日期 | 协议 | 数据版本 | 资源包格式 | 数据包格式 | 最低 Java |
|---|---|---|---|---|---|---|
| 1.21（Tricky Trials） | 2024-06-13 | **767** | 3953 | 34 | 48 | Java 21 |
| 1.21.1 | 2024-08-08 | **767** | 3955 | 34 | 48 | Java 21 |

两者协议号相同（wiki.vg 明确标注 1.21.1 与 1.21 为同一 PVN）。1.21.1 是**热修复版本**：
修复方块实体交换等严重漏洞、新增两种语言，并兼容 1.21 服务器，没有新玩法内容。

### 1.2 加载器 / 映射版本

| 项目 | 1.21 | 1.21.1 | 备注 |
|---|---|---|---|
| Fabric Loader | 0.15.11（发布时推荐）/ **0.20.1**（当前稳定） | 同左 | 一个 loader 可服务多个游戏版本；meta API 会给出兼容列表 |
| Fabric API | **0.102.0+1.21** | **0.116.18+1.21.1** | 不在 meta API，见 maven.fabricmc.net；后缀 = 游戏版本 |
| Intermediary | `net.fabricmc:intermediary:1.21` | `net.fabricmc:intermediary:1.21.1` | 与游戏版本同名，跨版本稳定命名（`class_xxx`/`method_xxx`） |
| Yarn | **1.21+build.9** | **1.21.1+build.3** | 社区可读映射；1.21 时代仍是 Fabric 默认映射 |
| NeoForge | 21.0.167（21.0.x 线） | 21.1.266（21.1.x 线） | 阶段一已用 1.21.1 + NeoForge |
| Forge | 1.21-51.0.33 | 1.21.1-52.1.21 | 参考项，mc-rs 暂不纳入加载器支持 |

**关于 Fabric Loader 版本选择**：Fabric Loader 与游戏版本基本解耦。`/v2/versions/loader/{game_version}`
对 1.21 返回的稳定项就是 0.20.1，因此第二阶段用 **0.20.1** 即可；若需还原 2024-06 的“原始”
环境，再用 0.15.11。两者对协议/资源管线没有影响。

**关于 Yarn 与映射**：1.21/1.21.1 时代 Yarn 仍是 Fabric 默认映射（Fabric 到 1.21.10 才改用
Mojang 官方映射）。但 Yarn 已停止维护（当前唯一 `stable` 的 Yarn 是 1.21.11+build.6，
1.21 各构建均为 `stable:false`）。做**新**映射工作流时建议直接用 Mojang 官方映射（mojmap），
Yarn 仅用于兼容旧 mod 与旧教程。

---

## 2. Fabric 与 NeoForge 的 mod 加载机制差异

两者都是「扫描 mod 文件 → 解析清单 → 解析依赖 → 字节码注入 → 调用入口」的 JVM 方案，
但清单格式、入口模型、依赖语义、生命周期与事件系统都不一样。对 mc-rs 来说，**管理面必须
同时解析两种清单**，而加载面（自研运行时）需要同时提供两套 API 语义。

### 2.1 对照表

| 维度 | Fabric | NeoForge |
|---|---|---|
| 清单文件 | `fabric.mod.json`（JSON，`schemaVersion: 1`） | `META-INF/neoforge.mods.toml`（TOML） |
| 清单关键字段 | `id` / `version` / `environment` / `entrypoints` / `depends` / `mixins` | `modLoader` / `loaderVersion` / `[[mods]]` / `[[dependencies.*]]` / `[[mixins]]` |
| 入口模型 | 声明式 entrypoint：`main` / `client` / `server`，分别实现 `ModInitializer` / `ClientModInitializer` / `DedicatedServerModInitializer` | `@Mod("id")` 注解类 + 构造函数注入（`IEventBus` / `ModContainer` / `Dist`） |
| 依赖语义 | `depends`（硬失败）、`recommends`/`suggests`（软）、`breaks`（硬失败）、`conflicts`（警告）；值为 SemVer 超集或 `*` | `mandatory`/`optional` + Maven 版本区间 `versionRange` + `ordering` + `side` |
| 初始化阶段 | Loader 发现 → 依赖解析 → remap → 依次执行 `main`、`client`/`server` 入口 | 扫描 → 构造 `ModContainer` → 生命周期事件：construct → `FMLCommonSetupEvent` → `FMLClientSetupEvent`/`FMLDedicatedServerSetupEvent` → `InterModComms` → `FMLLoadCompleteEvent` |
| 事件系统 | **Loader 本身无事件总线**；事件 API 由 Fabric API（或 mod 自定义 entrypoint）提供 | **内置双事件总线**：`NeoForge.EVENT_BUS`（游戏总线）+ 每 mod 的 mod 总线；`@SubscribeEvent` / `@EventBusSubscriber` |
| 字节码注入 | Mixin；在 `fabric.mod.json` 的 `mixins` 数组声明配置文件（可按 `environment` 区分端） | Mixin；在 `neoforge.mods.toml` 的 `[[mixins]]` 声明 `config`，可用 `requiredMods` 条件启用 |
| 映射 | mod 编译到 **intermediary**；运行时 Loader 把 MC 从官方混淆名映射到 intermediary | mod 编译到 **Mojang 官方映射（mojmap）**；运行时即官方名 |
| API 形态 | Fabric API 为**模块化聚合包**（可按需引入子模块），也可完全不用 | NeoForge API **内置于 loader**（注册、事件、能力、网络、配置等一体） |
| 端（side） | `environment`（`*` / `client` / `server`）+ entrypoint 区分 | `@Mod(dist = Dist.CLIENT)`、`FMLEnvironment.dist`、依赖级 `side` |
| 发现位置 | `mods/` 目录 + classpath（`fabric.modsFolder` 可改） | `mods/` 目录 + classpath |

### 2.2 对 mc-rs 的具体影响

1. **管理面（可做）**：`modsys.rs` 已解析 `neoforge.mods.toml`，需新增 `fabric.mod.json` 解析。
   两者字段可归一化为统一模型：`mod_id / version / display_name / side / dependencies[]`。
   - Fabric 依赖：`{ mod_id: version_range }`，需实现 SemVer 超集区间（`*`、`>1.0`、`[1,)`、多组件）。
   - NeoForge 依赖：已是 `{ mod_id, mandatory, versionRange, ordering, side }`。
2. **加载面（自研运行时）**：无论哪种加载器，真实 mod 都是 JVM 字节码 + Mixin，Rust 无法加载。
   自研运行时可提供统一 API（注册、事件、HUD、配置），并用 `mcmod.toml` 作为原生/脚本 mod 清单。
3. **映射差异**：Fabric 生态大量 mod/教程基于 Yarn（`class_1234` 之外的 `named` 名），
   NeoForge 基于 mojmap。若未来支持「导入/兼容」旧 mod 元数据，需按加载器选择映射表。
4. **事件模型差异**：NeoForge mod 依赖事件总线，Fabric mod 多用 entrypoint + Fabric API 事件。
   自研运行时若提供事件，建议对齐 NeoForge 的「游戏总线 + 生命周期」模型（更完整），
   并额外提供 Fabric 风格的 entrypoint 钩子（`on_init` / `on_client_init`）。

---

## 3. FabricMC meta API（meta.fabricmc.net）

- 基址：<https://meta.fabricmc.net/> ，JSON HTTP API，**每 5 分钟更新**，可匿名访问。
- 端点文档（GitHub）：<https://github.com/FabricMC/fabric-meta>
- 路由总览（含未写进 README 的端点）：<https://meta.fabricmc.net/> （根路径返回路由表）
- `game_version` / `loader_version` 需 URL 编码（如 `1.14 Pre-Release 5` → `1.14%20Pre-Release%205`）。

### 3.1 端点清单

| 用途 | Endpoint |
|---|---|
| 全部游戏版本 | `GET /v2/versions/game` |
| 某游戏版本可用的 loader（含最佳 intermediary） | `GET /v2/versions/loader/{game_version}` |
| 指定 loader 的 intermediary + `launcherMeta` | `GET /v2/versions/loader/{game_version}/{loader_version}` |
| 官方启动器 profile JSON | `GET /v2/versions/loader/{game_version}/{loader_version}/profile/json` |
| profile zip（含 dummy jar） | `GET /v2/versions/loader/{game_version}/{loader_version}/profile/zip` |
| 服务端 profile JSON | `GET /v2/versions/loader/{game_version}/{loader_version}/server/json` |
| 服务端 jar | `GET /v2/versions/loader/{game_version}/{loader_version}/{installer_version}/server/jar` |
| Yarn 版本（按游戏版本） | `GET /v2/versions/yarn/{game_version}` |
| Intermediary 版本（按游戏版本） | `GET /v2/versions/intermediary/{game_version}` |
| 安装器列表 | `GET /v2/versions/installer` |
| 全量数据（很大） | `GET /v2/versions` |

### 3.2 示例响应（实测，2026-11-16）

**`GET /v2/versions/yarn/1.21`**（完整）：

```json
[
  { "gameVersion": "1.21", "separator": "+build.", "build": 9,
    "maven": "net.fabricmc:yarn:1.21+build.9", "version": "1.21+build.9", "stable": false },
  { "gameVersion": "1.21", "separator": "+build.", "build": 8,
    "maven": "net.fabricmc:yarn:1.21+build.8", "version": "1.21+build.8", "stable": false }
]
```

**`GET /v2/versions/intermediary/1.21`**（完整）：

```json
[ { "maven": "net.fabricmc:intermediary:1.21", "version": "1.21", "stable": true } ]
```

**`GET /v2/versions/loader/1.21/0.15.11`**（截取，省略 libraries 明细）：

```json
{
  "loader": { "separator": ".", "build": 11,
    "maven": "net.fabricmc:fabric-loader:0.15.11", "version": "0.15.11", "stable": false },
  "intermediary": { "maven": "net.fabricmc:intermediary:1.21", "version": "1.21", "stable": true },
  "launcherMeta": {
    "version": 2,
    "min_java_version": 8,
    "libraries": {
      "common": [
        { "name": "org.ow2.asm:asm:9.6", "url": "https://maven.fabricmc.net/", "sha1": "...", "size": 126122 },
        { "name": "net.fabricmc:sponge-mixin:0.13.3+mixin.0.8.5", "url": "https://maven.fabricmc.net/" }
      ]
    },
    "mainClass": {
      "client": "net.fabricmc.loader.impl.launch.knot.KnotClient",
      "server": "net.fabricmc.loader.impl.launch.knot.KnotServer"
    }
  }
}
```

**`GET /v2/versions/loader/1.21/0.20.1/profile/json`**（截取）：

```json
{
  "id": "fabric-loader-0.20.1-1.21",
  "inheritsFrom": "1.21",
  "type": "release",
  "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
  "arguments": { "game": [], "jvm": ["-DFabricMcEmu= net.minecraft.client.main.Main "] },
  "libraries": [
    { "name": "org.ow2.asm:asm:9.10.1", "url": "https://maven.fabricmc.net/", "sha256": "..." }
  ]
}
```

**`GET /v2/versions/installer`**（截取，当前稳定安装器）：

```json
[ { "url": "https://maven.fabricmc.net/net/fabricmc/fabric-installer/1.1.2/fabric-installer-1.1.2.jar",
    "maven": "net.fabricmc:fabric-installer:1.1.2", "version": "1.1.2", "stable": true } ]
```

### 3.3 使用要点

- **`stable` 是全局标记**，不是「该游戏版本的最新」：当前整个 yarn 列表只有 `1.21.11+build.6`
  为 `stable`，整个 loader 列表只有 `0.20.1` 为 `stable`。**取某游戏版本的可用版本时，取列表
  第一个（最新）即可**，不要依赖 `stable`。
- **Fabric API 不在 meta API 里**，它在 Fabric 的 Maven 上：
  `https://maven.fabricmc.net/net/fabricmc/fabric-api/fabric-api/maven-metadata.xml`
  （版本号形如 `0.102.0+1.21`，后缀即游戏版本；当前总最新为 `0.166.2+26.4`）。
- **Intermediary 与游戏版本一一对应**，通常直接等于版本号；Yarn 才需要按 `gameVersion` 查构建号。
- 若要在 Rust 侧调用，最小解析结构示例：

```rust
#[derive(serde::Deserialize)]
struct YarnVersion {
    #[serde(rename = "gameVersion")] game_version: String,
    version: String,
    maven: String,
    stable: bool,
}
// GET https://meta.fabricmc.net/v2/versions/yarn/1.21  -> Vec<YarnVersion>
```

---

## 4. 1.21 与 1.21.1 的差异对客户端重写的影响

### 4.1 相同点（可复用）

| 维度 | 结论 |
|---|---|
| 网络协议 | 都是 **767**，包帧/VarInt/加密/登录流程完全一致 → 仓库 `protocol.rs` 无需为 1.21 改协议号 |
| 资源包 / 数据包格式 | 都是 **34 / 48** → 资源管线、数据包解析、注册表解析逻辑可共用 |
| Java 版本 | 都是 **Java 21** |
| 同族版本 | 1.21 / 1.21.1 同属「Tricky Trials」代际，核心玩法（试炼密室、怒目怪、风弹等）一致 |

### 4.2 差异点（需处理）

1. **数据版本不同**：1.21 = 3953，1.21.1 = 3955。注册表/配方/战利品表等**数据内容有少量变化**，
   但仓库 `assets`/`registry` 是按版本下载并索引客户端 jar 的，天然按版本隔离，无需改代码。
2. **版本清单与资产索引不同**：两者有各自的 `version.json`、asset index、client/server jar，
   `assets fetch` 已按 `--version` 区分，直接传 `1.21` 即可。
3. **1.21.1 修复的问题**：1.21 存在「恶意 target selector 导致服务器栈耗尽崩溃」等问题，
   1.21.1 修复。若涉及服务器交互/反作弊逻辑，应以 1.21.1 行为为准。
4. **mod 侧完全分层**：Fabric API（0.102.0 vs 0.116.18）、Yarn（build.9 vs build.3）、
   Intermediary（1.21 vs 1.21.1）、NeoForge（21.0.x vs 21.1.x）都按版本独立。管理面解析时
   必须**按加载器 + 游戏版本**二维匹配，不能只按加载器。
5. **生态偏好**：1.21.1 才是 mod 生态的事实标准（绝大多数 1.21.x mod 只发 1.21.1），1.21 基本被取代。

### 4.3 建议

- **以 1.21.1 为协议/资源基线**：仓库现有 `protocol.rs`（767）与 1.21.1 资产管线可直接复用；
  1.21 作为「同协议兼容目标」验证一次即可。
- 第二阶段 Fabric 落地顺序：解析 `fabric.mod.json` → 归一化依赖模型 → 用 meta API 做
  版本/依赖校验 → 自研运行时提供注册/事件/HUD 钩子。
- 映射默认走 **mojmap**（与 NeoForge 一致），Yarn 仅作旧 mod 兼容查询。

---

## 5. 参考链接

**Fabric 官方**

- Fabric for Minecraft 1.21 & 1.21.1（博客，含发布时 Loader 版本与破坏性变更）
  <https://fabricmc.net/2024/05/31/121.html>
- Fabric Meta API 文档 <https://github.com/FabricMC/fabric-meta>
- Fabric Meta 路由总览 <https://meta.fabricmc.net/>
- Fabric Loader 文档 <https://docs.fabricmc.net/develop/loader/>
- `fabric.mod.json` 规范 <https://docs.fabricmc.net/develop/loader/fabric-mod-json>
- Fabric Wiki：fabric.mod.json <https://wiki.fabricmc.net/documentation:fabric_mod_json>
- Fabric Maven（Fabric API / Yarn / Intermediary / Loader）<https://maven.fabricmc.net/>
- Fabric API maven-metadata <https://maven.fabricmc.net/net/fabricmc/fabric-api/fabric-api/maven-metadata.xml>
- Fabric Loader Releases <https://github.com/FabricMC/fabric-loader/releases>

**NeoForge 官方**

- Mod 文件（`neoforge.mods.toml`）<https://docs.neoforged.net/docs/gettingstarted/modfiles>
- 事件与 mod 生命周期（1.21.1）<https://docs.neoforged.net/docs/1.21.1/concepts/events/>
- NeoForge Maven 元数据 <https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml>
- NeoForge 版本号规则（MC 版本编码进 NeoForge 版本）
  <https://mcreference.com/guides/neoforge-version-numbers-explained>

**Minecraft / 协议 / 映射**

- Minecraft Wiki：Java Edition 1.21 <https://minecraft.wiki/w/Java_Edition_1.21>
- Minecraft Wiki：Java Edition 1.21.1 <https://minecraft.wiki/w/Java_Edition_1.21.1>
- wiki.vg：协议版本号 <https://wiki.vg/Protocol_version_numbers>
- Minecraft Wiki：pack format <https://minecraft.wiki/w/Pack_format>
- Fabric Wiki：Mappings（Intermediary / Yarn / 官方映射说明）<https://wiki.fabricmc.net/tutorial:mappings>
