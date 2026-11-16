# mc-rs 版本矩阵与推进顺序

> 用途：为 mc-rs（Rust 重写 Minecraft Java 客户端）的「游戏版本 × 加载器」扩展提供路线图。
> 排列规则：**先新后旧**；同一游戏版本内，**各加载器取最新可用版本**。
> 数据来源：FabricMC meta API / maven、NeoForged Maven、Forge Maven、wiki.vg、Minecraft Wiki。
> 快照时间 **2026-11-16**。详细 Fabric 1.21 调研见 [`docs/fabric-1.21.md`](fabric-1.21.md)。

---

## 1. 矩阵

| 游戏版本 | 协议 | Fabric Loader | Fabric API | Yarn（最新构建） | NeoForge | Forge |
|---|---|---|---|---|---|---|
| 1.21.11 | 774 | 0.20.1 | 0.141.6 | 1.21.11+build.6 | 21.11.48 | 1.21.11-61.2.5 |
| 1.21.10 | 773 | 0.20.1 | 0.138.4 | 1.21.10+build.3 | 21.10.64 | 1.21.10-60.1.18 |
| 1.21.9 | 773 | 0.20.1 | 0.134.1 | 1.21.9+build.1 | 21.9.16-beta | 1.21.9-59.0.5 |
| 1.21.8 | 772 | 0.20.1 | 0.136.1 | 1.21.8+build.1 | 21.8.57 | 1.21.8-58.1.26 |
| 1.21.7 | 772 | 0.20.1 | 0.129.0 | 1.21.7+build.8 | 21.7.25-beta | 1.21.7-57.0.3 |
| 1.21.6 | 771 | 0.20.1 | 0.128.2 | 1.21.6+build.1 | 21.6.20-beta | 1.21.6-56.0.9 |
| 1.21.5 | 770 | 0.20.1 | 0.128.2 | 1.21.5+build.1 | 21.5.101 | 1.21.5-55.1.18 |
| 1.21.4 | 769 | 0.20.1 | 0.119.4 | 1.21.4+build.8 | 21.4.161 | 1.21.4-54.1.22 |
| 1.21.3 | 768 | 0.20.1 | 0.114.1 | 1.21.3+build.2 | 21.3.100 | 1.21.3-53.1.16 |
| 1.21.2 | 768 | 0.20.1 | 0.106.1 | 1.21.2+build.1 | 21.2.1-beta | —（未发布） |
| **1.21.1** | **767** | 0.20.1 | **0.116.18** | **1.21.1+build.3** | **21.1.266** | 1.21.1-52.1.21 |
| **1.21** | **767** | 0.20.1 | **0.102.0** | **1.21+build.9** | 21.0.167 | 1.21-51.0.33 |
| 1.20.6 | 766 | 0.20.1 | 0.100.8 | 1.20.6+build.3 | 20.6.144 | 1.20.6-50.2.15 |
| 1.20.4 | 765 | 0.20.1 | 0.97.3 | 1.20.4+build.3 | 20.4.253 | 1.20.4-49.2.13 |
| 1.20.1 | 763 | 0.20.1 | 0.92.12 | 1.20.1+build.10 | 1.20.1-47.1.106 | 1.20.1-47.4.29 |
| 1.19.4 | 762 | 0.20.1 | 0.87.2 | 1.19.4+build.2 | —（不支持） | 1.19.4-45.4.5 |
| 1.18.2 | 758 | 0.20.1 | 0.77.0 | 1.18.2+build.4 | —（不支持） | 1.18.2-40.3.12 |
| 1.16.5 | 754 | 0.20.1 | 0.42.0+1.16 | 1.16.5+build.10 | —（不支持） | 1.16.5-36.2.42 |

### 读表说明

- **Fabric Loader 列恒为 0.20.1**：Loader 与游戏版本解耦，0.20.1 是当前唯一 `stable` 且兼容表内
  所有版本；「每层取最新」在 Fabric 列即取 0.20.1。
- **`stable` 语义**：meta API 的 `stable` 是**全局**标记（当前 yarn 仅 `1.21.11+build.6`、
  loader 仅 `0.20.1` 为 stable）。取某游戏版本的可用版本时**取列表第一个（最新）**即可。
- **Fabric API**：版本号后缀 = 游戏版本（1.16.x 用 `+1.16`）。它是模块化聚合包，可按需引入子模块。
- **Yarn**：1.21 时代仍是 Fabric 默认映射；Yarn 已停止维护，新项目建议用 Mojang 官方映射（mojmap）。
- **NeoForge 版本号编码了 MC 版本**：`21.1.x` = 1.21.1、`21.0.x` = 1.21、`20.6.x` = 1.20.6；
  `1.20.1` 仍沿用 Forge 的 `1.20.1-47.1.x` 编号。表中标 `-beta` 者表示该 MC 版本只有测试版。
- **协议号**：1.21 / 1.21.1 同为 767；1.21.2 / 1.21.3 同为 768；1.21.7 / 1.21.8 同为 772；
  1.21.9 / 1.21.10 同为 773。**同协议号 = 网络层可复用**。
- **Quilt** 兼容 Fabric mod，可用同一套 Fabric 管理面，矩阵不再单列。
- mc-rs 当前只支持 **Fabric / NeoForge** 两种加载器的管理面；Forge 列仅作参考。

---

## 2. 建议推进顺序

总原则：**先新后旧**排列矩阵；**每层先各加载器最新版**。实际工程推进以「协议代际」和
「当前阶段目标」为主，先闭环 1.21 家族，再按协议代际向外扩展。

| 阶段 | 游戏版本 × 加载器 | 协议 | 理由 |
|---|---|---|---|
| ✅ 已完成 | 1.21.1 + NeoForge | 767 | 阶段一：资产管线、注册表、协议基础、mod 管理面 |
| 🎯 下一阶段 | **1.21 + Fabric** | 767 | 与 1.21.1 同协议，复用 `protocol.rs`；新增 `fabric.mod.json` 管理面 |
| 3 | 1.21.1 + Fabric | 767 | 同协议，验证 Fabric 管理面与自研运行时 |
| 4 | 1.21 + NeoForge | 767 | 补齐 1.21 家族，验证「同版本 × 双加载器」抽象 |
| 5 | 1.21.4 → 1.21.5 | 769 / 770 | 渲染相关重写（1.21.2 实体渲染状态、1.21.4 客户端物品、1.21.5 模型/渲染管线） |
| 6 | 1.21.8 → 1.21.11 | 772 / 774 | 图形/API 进一步重构（1.21.11 又一次渲染重写） |
| 7 | 1.20.6 → 1.20.1 | 766 / 763 | 旧代际兼容（1.20.5/6 为组件系统过渡期，1.20.1 为长期支持版） |
| 8 | 更早（1.19.4 / 1.18.2 / 1.16.5） | 762 / 758 / 754 | 按需，主要用于旧 mod 兼容验证 |

### 每阶段落地清单（以 1.21 + Fabric 为例）

1. **管理面**：解析 `fabric.mod.json`（`id`/`version`/`environment`/`entrypoints`/`depends`/`mixins`），
   归一化到与 `neoforge.mods.toml` 相同的内部模型。
2. **依赖校验**：实现 Fabric 的 SemVer 超集区间（`*`、`>1.0`、`[1,)`、多组件、`-` 预发布后缀）。
3. **版本校验**：调用 meta API（`/v2/versions/loader/1.21`、`/v2/versions/yarn/1.21`、
   `/v2/versions/intermediary/1.21`）确认加载器/映射可用版本。
4. **加载面**：自研运行时提供注册/事件/HUD/配置钩子，对齐 NeoForge 生命周期 + Fabric entrypoint 语义。
5. **验证**：1.21 与 1.21.1 各跑一次（同协议 767），确认资产/注册表按版本隔离正确。

---

## 3. 数据来源

- Fabric meta API：<https://meta.fabricmc.net/> （文档 <https://github.com/FabricMC/fabric-meta>）
  - `GET /v2/versions/loader/{game}`、`GET /v2/versions/yarn/{game}`、`GET /v2/versions/intermediary/{game}`
- Fabric API 元数据：<https://maven.fabricmc.net/net/fabricmc/fabric-api/fabric-api/maven-metadata.xml>
- Fabric Loader Releases：<https://github.com/FabricMC/fabric-loader/releases>
- NeoForge 元数据：<https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml>
- Forge 元数据：<https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml>
- 协议号：<https://wiki.vg/Protocol_version_numbers>
- 游戏版本事实：<https://minecraft.wiki/w/Java_Edition_1.21>、<https://minecraft.wiki/w/Java_Edition_1.21.1>
- 加载机制：<https://docs.fabricmc.net/develop/loader/fabric-mod-json>、
  <https://docs.neoforged.net/docs/gettingstarted/modfiles>
