# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Flick 是一个 Rust 桌面应用，提供双面板文件管理器界面，通过 SSH/SFTP 进行本地与远程文件传输。UI 框架使用 Slint。

## Build & Run

```bash
# 构建（debug）
cargo build

# 构建（release，启用 LTO + strip）
cargo build --release

# 运行
cargo run

# 运行并传入文件路径参数（右键菜单场景）
cargo run -- <FILE_PATH>

# 检查编译错误
cargo check

# 格式化
cargo fmt

# Lint
cargo clippy
```

## Architecture

### 模块结构

```
src/
├── main.rs          # 入口：CLI 解析 (clap)、初始化 UI 状态、绑定回调
├── config.rs        # ServerConfig / AppConfig，TOML 序列化，配置路径: ~/.config/flick/server.toml
├── ssh_core.rs      # SshUploader：双模式认证（libssh2 / 原生 ssh+scp 回退），FileTransfer trait
├── local_fs.rs      # 本地文件系统列表
├── remote_fs.rs     # 远程文件系统列表（SFTP 或 ssh ls 回退）
├── transfer.rs      # TransferQueue / TransferTask，进度回调
├── utils.rs         # 工具函数
└── ui_bridge/       # Rust ↔ Slint UI 绑定层
    ├── mod.rs
    ├── settings.rs  # 服务器配置 CRUD、连接测试
    ├── explorer.rs  # 双面板文件浏览、上传/下载操作
    ├── convert.rs   # UI 类型 ↔ Rust 类型转换
    └── quick_upload.rs  # Phase 5 待恢复
```

### UI 层 (Slint)

```
ui/
├── app.slint              # 主窗口
├── types.slint            # 共享数据类型
├── components/            # 可复用组件（file_item, server_selector, path_breadcrumb, transfer_item）
├── pages/settings_page.slint
├── panels/                # local_panel, remote_panel, transfer_panel
└── theme/
```

`build.rs` 在编译期调用 `slint_build::compile("ui/app.slint")` 生成 Rust 绑定，`main.rs` 通过 `slint::include_modules!()` 引入。

### 关键设计

- **双模式 SSH**：优先使用 `ssh2` (libssh2)，失败时回退到系统原生 `ssh`/`scp` 命令
- **认证链**：密码 → 指定密钥 → SSH Agent → ~/.ssh 自动探测
- **状态共享**：`Arc<Mutex<AppConfig>>` 在 UI 回调间共享配置
- **Windows 适配**：子进程创建使用 `CREATE_NO_WINDOW` 标志
- **配置存储**：TOML 格式，路径由 `dirs` crate 决定（Windows: `%APPDATA%/flick/`，Unix: `~/.config/flick/`）

### 开发阶段

当前处于 Phase 4（双面板文件管理器）。`quick_upload` 模块已禁用，计划在 Phase 5 恢复。

## Language

代码注释和 UI 文本均为中文。
