# Ruwriter

跨平台 Markdown 写作软件，以 [Cove](https://github.com/AlexCat315/Cove) 的 `cove-ui` 为 UI/宿主框架。Rust 2024，桌面、iOS、Android 共用同一套 Rust 视图与文档模型。

## 结构

| 目录 | 内容 |
| --- | --- |
| `crates/core` | 文档模型、资料库索引、原子写入、会话状态、字数统计；无 UI 依赖，可单独测试 |
| `src` | Cove 视图树、控制器、快捷键 |
| `Platforms` | macOS / iOS / Windows / Linux / Android 清单与打包资源 |
| `xtask` | 复用 Cove 的 `cove-package` 打包、签名与装机 |

## 构建

`../Cove` 必须存在并完成过一次 `cargo cove setup`。

```sh
cargo build --workspace
cargo run -p ruwriter
cargo test -p ruwriter-core
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

打包：

```sh
cargo xtask package mac --release
cargo xtask package windows --release
cargo xtask package linux --release
cargo xtask package android --release
cargo xtask package ios --simulator
```

## 快捷键

| 快捷键 | 动作 |
| --- | --- |
| ⌘/Ctrl+N | 新建 |
| ⌘/Ctrl+O | 打开 |
| ⌘/Ctrl+S | 保存并刷新索引 |
| ⌘/Ctrl+P | 写作 / Markdown 预览 |
| ⌘/Ctrl+Shift+F | 专注模式 |

## 数据位置

`library.json` 与 `documents/` 位于各平台的用户文档目录；外部文件通过「打开」导入后仍写回原路径。`RUWRITER_DATA_DIR` 可覆盖根目录。每次编辑即时原子落盘，退出不丢内容。

## 已知限制

- Cove 只提供原生「打开」面板，没有「另存为」面板；新建文档落在自有资料库。
- iOS / Android 通过应用内面板打开文件，未接入系统「打开方式」回传。
- Cove 的 `application!` 不提供 Wasm 入口；本版本不覆盖 Web。
