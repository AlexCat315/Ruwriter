# Changelog

## 0.1.0 — 2026-11-09

- 以 Cove `Templates/app` 为基础建立独立工作区，使用本地 `cove-ui` 与 `cove-package`。
- 新增 `ruwriter-core`：文档元数据、资料库索引、跨平台存储根目录、原子写入、会话状态机、中英文混排字数统计。
- 新增 Cove 视图树：工具栏、资料库侧栏、多行编辑器、Markdown 预览、专注模式、状态栏与全局快捷键。
- 新增 macOS / iOS / Windows / Linux / Android 平台清单，补齐模板缺失的 iOS `Info.plist` 与 entitlements，声明 Markdown / 纯文本文档类型。
- 每次编辑即时原子落盘；外部文件保存回原路径。

### 验证

- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --locked`、`cargo build --workspace --locked` 均通过；核心 crate 20 项测试通过。
- `ruwriter-core` 在 `aarch64-apple-darwin`、`x86_64-pc-windows-msvc`、`x86_64-unknown-linux-gnu`、`aarch64-linux-android`、`aarch64-apple-ios`、`aarch64-apple-ios-sim` 六个目标上 `cargo check` 通过。
- macOS 原生运行经 Cove DevTools 验收：文本编辑、即时落盘、标题/字数更新、新建、切换文档、保存、预览、专注模式、⌘P / ⌘⇧F 快捷键与 `capture_ui` 截图，运行日志无错误。

### 未覆盖

- Cove 的 macOS 原生应用菜单栏尚未安装（`crates/hosts/src/PLATFORM_COVERAGE.md`），本版本使用自绘工具栏与全局快捷键。
- Cove 无原生保存面板、无系统「打开方式」回调、无 Wasm 应用入口；相关平台能力受此限制。
- iOS / Android / Windows / Linux 未做真机或模拟器的视觉与交互验收；DevTools 文本注入不验证原生键盘与输入法激活。
