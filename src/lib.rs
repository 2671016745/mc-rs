//! mc-rs：Minecraft Java Edition 1.21.1 客户端本体的 Rust 重写。
//!
//! 库目标（供 CLI 与集成测试使用）：
//! - `assets`/`registry`：官方资源获取、索引、方块注册表
//! - `protocol`/`net`：协议编解码、服务器 ping、登录握手、加密
//! - `auth`/`skin`：离线 / 第三方 / 微软登录、皮肤
//! - `server`：服务器列表
//! - `modsys`/`modruntime`：mod 清单管理面与自研 Lua 运行时
//! - `render`：wgpu 渲染层

pub mod assets;
pub mod auth;
pub mod modruntime;
pub mod modsys;
pub mod net;
pub mod protocol;
pub mod registry;
pub mod render;
pub mod server;
pub mod skin;
