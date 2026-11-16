# Java Mod 桥接可行性调研：让 NeoForge / Forge / Fabric mod 在 Rust 客户端运行

> 适用项目：mc-rs（Minecraft Java 1.21.1 客户端本体的 Rust 重写）。
> 结论基于公开资料与同类项目实测（见文末参考），非本项目实机测量。

## 1. 结论

**混合可行（分层），但「原样加载任意 .jar」不可行，「全生态无缝兼容」不可行。**

- Java mod 的本质是 **classfile + 清单 + Mixin 字节码补丁 + 事件总线**，它假设运行在
  JVM 上、并能改写 Minecraft 的 Java 类（NeoForge 还直接 patch 原版源码、AccessTransformer、
  接口注入）。Rust 本体没有这些 Java 类，也不存在字节码级别的补丁面。
- 已落地的同类项目（Horizon）明确声明 Java mod jar 不能运行；其分支项目 horizon-java
  用「有界字节码解释器（默认无 JVM）+ JNI/JVM 兜底」的分层方案才让一部分 jar 跑起来，
  实测约 500 个 jar 中仅 ~111 个 `ran-clean`（[来源 3/4]）。
- 推荐策略：**能不带 Java 就不带 Java**；按 jar 逐个判定层级；绝不静默回退 JVM。

## 2. 五条技术路径对比

| 路径 | 原理 | 主要依赖 | 工作量 | 致命障碍 |
|---|---|---|---|---|
| A. JNI 内嵌 JVM 跑 NeoForge | Rust 用 JNI Invocation API 起 JVM，classpath 放 NeoForge + mods，Rust 侧实现 Host façade（注册表/渲染/网络回调） | `jni` crate、JDK、NeoForge 发行、ModLauncher/FML、跨语言对象桥 | 大：数人月～年 | façade 需覆盖 mod 触达的全部原版 Java 类（数千）；双实现逻辑漂移；渲染/输入循环归属冲突；又背回 JVM 的 GC 与内存开销；分发须捆 JDK |
| B. Java 客户端 + Rust native 注入 | 保留官方 Java 客户端（带加载器），Rust 以 JNI 本地库或 DLL/LD_PRELOAD 注入，接管渲染/优化层 | LWJGL 本地库机制、MinHook/PLT hook、Vulkan、Java 侧胶水 | 中：数月量级 | 已不是「Rust 重写客户端」：mod 兼容性来自 Java 客户端而非 Rust；对 mc-rs 目标不成立，只对「改造现有 Java 客户端」成立 |
| C. Rust 本体实现 Forge/NeoForge API 兼容层 | 在 Rust 中实现 registry/event/Mixin 语义或等价 API，把 mod 的注册与事件映射到 Rust | Mojang 官方映射资料、ASM 级类模型、逐版本适配 | 极大：多倍于 B；每个「游戏版本 × 加载器版本」都要重做 | Mixin 的 `@Inject`/`@Redirect`/`@Overwrite` 语义必须跑在完整 Java 对象模型上；反射、动态类加载、`invokedynamic` 难以模拟；分发反编译产物受法律限制 |
| D. 进程注入 / Hook 渲染层 | 不改游戏逻辑，注入后 hook `wglSwapBuffers`/`eglSwapBuffers`/glfw 回调，叠加渲染、HUD、输入 | 平台注入技术、OpenGL/Vulkan context 共享 | 小～中：单模块数周 | 只能改渲染/输入表层，管不到注册表与玩法逻辑，**无法满足「运行 mod」目标**；官方渲染转向 Vulkan 后注入点易变；定位为辅助能力（自研 HUD/缩放） |
| E. 自研 JVM / 有界字节码解释器（无 JVM 执行 jar） | Rust 解析 classfile、解释字节码子集，经显式 Host 边界把调用映射到 Rust 游戏对象（注册表/内容/网络） | 自研或第三方 classfile parser、`jni`（仅 oracle）、zip、mod 语料 | 大：horizon-java 用时数周～数月、1000+ commits 仍只覆盖有界子集 | 完整 JVM 语义（GC/线程/反射/`invokedynamic`/动态类加载）工程浩大；解释执行性能差；覆盖率低，需逐 mod 追平 |

## 3. 推荐架构（分层兼容，per-jar 判定）

参考 horizon-java 的 tier 模型（[来源 4]），并与 mc-rs 现状对齐（`assets`/`registry`/`modsys` 已就绪）：

1. **Tier 0｜内容携带**：纯数据/资产 mod（lang、assets、JSON、data pack、方块/物品/效果声明）
   → 直接走原生内容管线，永不启动 Java 执行。这是投入产出比最高的一层。
2. **Tier 1｜有界解释器**（主路径）：解析 `fabric.mod.json` / `neoforge.mods.toml`，
   发现入口（`onInitialize`、`@Mod`、legacy `@Mod`），解释受支持的字节码子集，
   把注册/监听调用导出为**类型化声明**（如 `NativeDeclarations`/`TypedRegistrySink`），
   进入 Rust 注册表与统一 content root，与携带内容共用命名空间校验。
3. **Tier 2｜JVM 兜底**：解释器或必需 hook 无法建模时，经 JNI 起 JVM 跑真实
   Fabric Loader(Knot)/NeoForge FML，并**报告原因**；产品模式下该回退不得默认发生，
   只有显式指定的 jar 才允许。开发期用它做 oracle 找差距。
4. **Tier 3｜人工移植/深度集成**：为头部 mod 单独适配；仅对被证明的热方法保留
   可选的原生编译标记（Tier 5 mark），其余留在 JVM。

要点：兼容粒度是「**jar × 客户端版本**」的兼容结果，不是一次性承诺；被拒绝的 jar
要记录 class/method/pc，不猜、不静默吞掉。

## 4. 验证实验设计

- **语料**：分层抽样——纯内容 mod、仅入口的轻量 mod、依赖 Fabric API、Mixin 重度、
  NeoForge 专属；建议直接复用 horizon-java 的 corpus-xl/mega/NeoForge 语料脚本
  （[来源 4]），在 mc-rs 上重放。
- **指标**：`ran-clean`（宿主零未建模调用）/ `ran-hollow`（跑通但有未建模调用）/
  `refused`（停止并报位置）三档计数；另测启动时间、内存、帧率、崩溃率。
- **门禁**：Phase 1 要求 0 `native-gap`，且每个 `needs-loader` jar 在真实 loader 下
  跑到 `Done(`、打印 mod id、无异常；混合包会话两半都在时间预算内。
- **Oracle 差分**：同一 jar 分别跑真实 NeoForge/Fabric 客户端与桥接运行时，比对
  注册表条目、事件序列、方块/物品行为；再叠加 1.21.1 的差分测试（同种子/同输入）。
- **效率基线**：按 0/3/20/74 个 jar 分档跑服务器与客户端基准（对照官方客户端 +
  NeoForge + Sodium），确认分层方案相对纯 Java 基线没有不可接受的性能损失。
- **合规**：全流程只读用户自有 Minecraft 副本与 mod 语料，不分发 Mojang 代码/资产。

## 5. 风险与开放问题

- Mixin 是最大单点：注入点必须在 Java 类结构上生效，Tier 1/2 都需要 façade 或解释器
  精确复刻目标类层次，工作量和 mod 数量近似线性。
- 1.21.1 生命周期内官方客户端可能切换渲染 API（OpenGL → Vulkan，[来源 9]），
  影响路径 B/D 的注入点选型，也影响 mc-rs 自身渲染层设计。
- JVM 兜底虽然保真，但与「去掉 Java、降低内存/启动时间」的立项目标相悖，
  只宜作验收 oracle 与长尾兜底，不宜成为主路径。

## 6. 参考链接

1. Horizon（Rust 重写 MC 26.3，声明不支持 Java mod jar）：
   https://github.com/tr4m0ryp/Minecraft-rust ｜ 介绍：https://news.9minecraft.net/minecraft-java-26-3-rust-horizon-client
2. horizon-java README（有界解释器 + JNI oracle 双crate）：
   https://github.com/Solenopsisbot/horizon-java
3. horizon-java 兼容状态实测（ledger 计数）：
   https://raw.githubusercontent.com/Solenopsisbot/horizon-java/main/docs/java-mod-compat-status.md
4. horizon-java 设计文档（tier 模型、Host 边界、阶段计划）：
   https://raw.githubusercontent.com/Solenopsisbot/horizon-java/main/docs/java-mod-compat.md
5. jni crate：从 Rust 启动 JVM（Invocation API）：
   https://docs.rs/jni/latest/jni/vm/struct.JavaVM.html
6. 实践文章：Embedding JVM in Rust：
   https://ivanyu.me/blog/2026/01/04/embedding-jvm-in-rust/
7. Fabric Loader 文档（Knot、entrypoint、Mixin 唯一官方变换方式）：
   https://docs.fabricmc.net/develop/loader/
8. Mixin 简介（作用于字节码、类加载前完成）：
   https://wiki.fabricmc.net/tutorial:mixin_introduction
9. NeoForge 事件系统与 mod 生命周期（双事件总线、patch 机制）：
   https://deepwiki.com/neoforged/Documentation/3.3-event-system-and-mod-lifecycle
10. Rust JVM 实现（解释器/字节码覆盖参考）：
    https://github.com/theseus-rs/ristretto ｜ https://github.com/hextriclosan/rusty-jvm ｜ https://github.com/douchuan/jvm
11. 路径 B 先例（Java 客户端 + C++ Vulkan 渲染后端）：
    https://github.com/Minecraft-Radiance/Radiance
12. 路径 D 参考（注入/Hook GL 与 LWJGL3 的通用做法）：
    https://flammablebunny.github.io/tuxinjector/injection ｜ https://github.com/derhass/glx_hook
13. 官方：Java 版渲染将从 OpenGL 迁移到 Vulkan：
    https://www.minecraft.net/en-us/article/another-step-towards-vibrant-visuals-for-java-edition
