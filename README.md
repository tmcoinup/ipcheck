# IPCheck · IP 质量检测工具（SOCKS5）

基于 **Rust + Iced** 的桌面端应用，用于导入 SOCKS5 代理、探测出口真实 IP、调用百度智能云风控接口做行为/设备/恶意等维度评估，并将结果持久化到本地 SQLite。

## 功能概览

- **导入代理**：通过系统 `zenity` 文本框粘贴多行（支持多种格式混贴与说明行过滤）。
- **批量 / 单条**：查询真实 IP、风控检查、删除；支持「风控检查单个」独立弹窗。
- **表格展示**：宽表横向滚动（同步偏移）、操作列固定宽度；无风险行绿色、有风险标红；紧凑布局下风险多列合并展示。
- **启动**：主窗口默认 **最大化**（启动 `Command` 与 `WindowOpened` 双路径调用 `iced::window::maximize`，以适配不同窗口管理器）。

## 技术栈

| 类别 | 选型 |
|------|------|
| UI | iced 0.12（Tokio、SVG） |
| 异步 | tokio |
| HTTP | reqwest（rustls，SOCKS 代理） |
| 存储 | rusqlite（bundled） |
| 错误 | anyhow / thiserror |
| 日志 | tracing |

## 架构说明（简要）

采用 **分层 + DDD 思想**：界面（`app`）→ 应用服务（`service/ip_service`）→ 仓储 trait（`repository`）→ SQLite 实现；领域模型在 `domain`。Handler 仅做消息转发，业务在 Service；HTTP、解析、探针等拆在子模块。详见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 目录结构

```
src/
  main.rs              # 入口：日志、字体、窗口参数、启动 Iced
  app/mod.rs           # 视图、消息、表格与弹窗
  config/              # 应用配置（如 API token 路径）
  domain/models.rs     # Proxy、检查结果等
  repository/          # SQLite 与 trait
  service/ip_service/  # 业务编排、百度 API、探针、代理解析
data/                  # 默认数据库等工作目录（可配置）
assets/icons/          # 工具栏 SVG
packaging/debian/      # deb 打包辅助文件
scripts/               # 发布打包脚本
```

## 配置与数据

- 配置文件与数据库路径由 `config` / `service` 解析（见 `resolve_db_path` 与 `app_config`）。
- 需要 **百度智能云相关 token** 时由配置或服务读取（以代码为准）。

## 环境要求

- **Rust**：需支持 `edition = "2024"` 的工具链（较新版本 nightly/stable，以 `rustc --version` 为准）。
- **Linux**：图形环境、Vulkan/兼容层；导入功能依赖 **`zenity`**（未安装时导入入口会失败，可包依赖中声明）。
- **Windows**：直接运行 `ipcheck.exe`；字体回退为 `sans-serif`（未装 `fc-match` 时）。

## 开发与运行

```bash
cargo run
```

发布优化构建：

```bash
cargo build --release
# 二进制：target/release/ipcheck
```

日志级别（可选）：

```bash
RUST_LOG=debug cargo run
```

## 打包安装包

脚本在 `scripts/` 下，生成物建议输出到 `dist/`（已加入 `.gitignore`）。

| 平台 | 脚本 | 说明 |
|------|------|------|
| Ubuntu / Debian | `./scripts/package-deb.sh` | 生成 `.deb`（需 `dpkg-deb`） |
| 通用 Linux 二进制包 | `./scripts/package-linux-tarball.sh` | `tar.gz` + README |
| Windows | `./scripts/package-windows-zip.sh` | 需在 **Windows** 上 `cargo build --release` 或安装 `x86_64-pc-windows-gnu` 交叉编译链 |

Linux 桌面环境（GNOME/KDE）注意事项：

- `.desktop` 需与窗口应用标识一致：`StartupWMClass=ipcheck`，且 Linux 下窗口 `application_id` 为 `ipcheck`（对应 `ipcheck.desktop` basename），否则可能出现“启动器图标 + 运行窗口图标”分离。
- 若任务栏有“启动中转圈”残留，可在 `.desktop` 使用 `StartupNotify=false` 并通过 `Exec=env DESKTOP_STARTUP_ID= ipcheck` 禁用启动通知跟踪。

详细步骤与依赖见各脚本内注释。若交叉编译 Windows，通常需要：

```bash
rustup target add x86_64-pc-windows-gnu
# 另需 mingw-w64 等，依发行版而定
```

## 许可证

若未单独声明，以仓库内文件为准。
