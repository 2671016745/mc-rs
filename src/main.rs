use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mc_rs::{assets, auth, modruntime, modsys, net, protocol, registry, render, server, skin};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "mc-rs",
    version,
    about = "Rust reimplementation of Minecraft Java Edition 1.21.1 (NeoForge)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 下载并校验官方客户端/服务端 jar（默认 1.21.1 client）
    Fetch {
        #[arg(long, default_value = "1.21.1")]
        version: String,
        #[arg(long, default_value = "client")]
        kind: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// 索引客户端 jar 的资产与数据
    Index {
        jar: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// 从 blockstates 构建方块注册表
    Registry {
        jar: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// 解析 NeoForge / 自研 mod 清单
    Mod {
        file: PathBuf,
    },
    /// 离线登录（生成离线 UUID）
    AuthOffline {
        name: String,
    },
    /// 第三方 Yggdrasil 登录
    AuthYggdrasil {
        authserver: String,
        username: String,
        #[arg(long)]
        password: Option<String>,
    },
    /// 微软 device code 登录（显示用户码与验证地址）
    AuthMicrosoftDevice {
        #[arg(long)]
        client_id: String,
    },
    /// 服务器列表管理
    Servers {
        #[command(subcommand)]
        cmd: ServersCmd,
    },
    /// 获取正版皮肤纹理属性
    SkinFetch { uuid: String },
    /// 联机登录（离线模式账号）
    LoginOffline {
        address: String,
        #[arg(long, default_value_t = 25565)]
        port: u16,
        name: String,
    },
    /// 加载自研 mod（Lua）
    Mods { dir: PathBuf },
    /// 打开渲染窗口（wgpu 骨架）
    Render,
}

#[derive(Subcommand)]
enum ServersCmd {
    /// 列出已保存的服务器
    List,
    /// 添加服务器
    Add {
        name: String,
        address: String,
        #[arg(long, default_value_t = 25565)]
        port: u16,
    },
    /// 删除指定序号
    Remove { index: usize },
    /// ping 所有服务器
    Ping,
}

fn read_line_prompt(prompt: &str) -> Result<String> {
    use std::io::Write;
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    let mut s = String::new();
    std::io::stdin().read_line(&mut s)?;
    Ok(s.trim().to_string())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Fetch {
            version,
            kind,
            out,
        } => {
            let manifest = assets::fetch_manifest()?;
            let mv = manifest
                .versions
                .iter()
                .find(|v| v.id == version)
                .with_context(|| format!("version {version} not found in manifest"))?;
            let vj = assets::fetch_version_json(&mv.url)?;
            let dl = vj
                .downloads
                .get(&kind)
                .with_context(|| format!("no download named {kind}"))?;
            eprintln!("downloading {} ({} bytes)", dl.url, dl.size);
            let data = assets::download(&dl.url)?;
            assets::verify_sha1(&data, &dl.sha1)?;
            let path = out.unwrap_or_else(|| PathBuf::from(format!("{kind}-{version}.jar")));
            std::fs::write(&path, &data)?;
            println!(
                "saved {} ({} bytes, sha1 verified, java {})",
                path.display(),
                data.len(),
                vj.java_version
                    .as_ref()
                    .map(|j| j.major_version.to_string())
                    .unwrap_or_else(|| "?".into())
            );
        }
        Cmd::Index { jar, out } => {
            let report = assets::index_jar(&jar)?;
            let s = serde_json::to_string_pretty(&report)?;
            match out {
                Some(p) => std::fs::write(p, s)?,
                None => println!("{s}"),
            }
        }
        Cmd::Registry { jar, out } => {
            let reg = registry::build_registry(&jar)?;
            let s = serde_json::to_string_pretty(&reg)?;
            match out {
                Some(p) => std::fs::write(p, s)?,
                None => println!("{s}"),
            }
        }
        Cmd::Mod { file } => {
            let m = modsys::parse_file(&file)?;
            println!("{}", serde_json::to_string_pretty(&m)?);
        }
        Cmd::AuthOffline { name } => {
            let a = auth::offline(&name);
            println!("{}", serde_json::to_string_pretty(&a)?);
        }
        Cmd::AuthYggdrasil {
            authserver,
            username,
            password,
        } => {
            let password = match password {
                Some(p) => p,
                None => read_line_prompt("password: ")?,
            };
            let client = uuid::Uuid::new_v4().simple().to_string();
            let a = auth::yggdrasil_account(&authserver, &username, &password, &client)?;
            println!("{}", serde_json::to_string_pretty(&a)?);
        }
        Cmd::AuthMicrosoftDevice { client_id } => {
            let d = auth::microsoft_device_code(&client_id)?;
            println!(
                "user code: {}\nopen: {}\nexpires in {}s (poll every {}s)",
                d.user_code, d.verification_uri, d.expires_in, d.interval
            );
        }
        Cmd::Servers { cmd } => match cmd {
            ServersCmd::List => {
                let l = server::ServerList::load()?;
                println!("{}", serde_json::to_string_pretty(&l)?);
            }
            ServersCmd::Add {
                name,
                address,
                port,
            } => {
                let mut l = server::ServerList::load()?;
                l.add(server::ServerEntry {
                    name,
                    address,
                    port,
                    version: None,
                    notes: None,
                });
                l.save()?;
                println!("saved {} servers", l.servers.len());
            }
            ServersCmd::Remove { index } => {
                let mut l = server::ServerList::load()?;
                match l.remove(index) {
                    Some(s) => {
                        l.save()?;
                        println!("removed: {}", s.name);
                    }
                    None => println!("no server at index {index}"),
                }
            }
            ServersCmd::Ping => {
                let l = server::ServerList::load()?;
                for (i, r) in l.ping_all() {
                    let name = l
                        .servers
                        .get(i)
                        .map(|s| s.name.clone())
                        .unwrap_or_default();
                    match r {
                        Ok((s, ms)) => {
                            let online = s.players.as_ref().map(|p| p.online).unwrap_or(0);
                            let max = s.players.as_ref().map(|p| p.max).unwrap_or(0);
                            let ver = s
                                .version
                                .as_ref()
                                .map(|v| v.name.clone())
                                .unwrap_or_default();
                            println!("[{i}] {name} | {ms}ms | {ver} | {online}/{max} | {}", s.motd_text());
                        }
                        Err(e) => println!("[{i}] {name} | error: {e}"),
                    }
                }
            }
        },
        Cmd::SkinFetch { uuid } => {
            let u = uuid::Uuid::parse_str(&uuid)?;
            let t = skin::fetch_textures(&u)?;
            println!("{}", serde_json::to_string_pretty(&t)?);
        }
        Cmd::LoginOffline {
            address,
            port,
            name,
        } => {
            let s = net::login_offline(&address, port, &name)?;
            println!("{}", serde_json::to_string_pretty(&s)?);
        }
        Cmd::Mods { dir } => {
            let mut rt = modruntime::ModRuntime::new()?;
            let mods = if dir.join("mcmod.toml").exists() {
                vec![rt.load_mod(&dir)?]
            } else {
                rt.load_dir(&dir)?
            };
            println!("loaded {} mod(s)", mods.len());
            for m in mods {
                println!("- {} v{}", m.manifest.id, m.manifest.version);
            }
        }
        Cmd::Render => {
            render::run()?;
        }
    }
    Ok(())
}
