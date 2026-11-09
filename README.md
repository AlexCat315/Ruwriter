# Ruwriter

跨平台 Markdown 写作软件，以 [Cove](https://github.com/AlexCat315/Cove) 的 `cove-ui` 为 UI/宿主框架。Rust 2024，桌面、iOS、Android 共用同一套 Rust 视图与文档模型。

## 界面

| 区域 | 内容 |
| --- | --- |
| 资料库侧栏 | 全部文稿 / 置顶 / 最近 7 天 / 废纸篓，带计数；底部切换外观（跟随系统 / 浅色 / 深色）。macOS 使用系统侧栏材质 |
| 文稿列表 | 标题、两行摘要、编辑时间与字数；搜索标题和正文；右键置顶、导出、移到废纸篓，废纸篓内恢复或永久删除 |
| 编辑器 | 居中的可读宽度正文栏，整栏滚动；Markdown 记号淡化、标题/代码/链接着色；底部字数与阅读时间 |
| 预览 | 宽窗口并排，窄窗口替换编辑器 |
| 专注模式 | 只留正文，Esc 退出 |
| ⌘K 命令面板 | 所有命令与所有文稿 |

窗口布局随宽度变化：≥ 900pt 三栏；640–900pt 收起侧栏，列表标题变为资料库菜单；< 640pt（手机）列表与编辑器单栏切换，编辑器带返回。macOS 安装中文原生菜单栏（文件 / 显示 / 资料库 / 编辑 / 窗口）；其他平台由工具栏、命令面板与快捷键提供同样的命令。

## 结构

| 目录 | 内容 |
| --- | --- |
| `crates/core` | 文档模型、资料库（集合、置顶、废纸篓、搜索、设置）、原子写入、会话状态、字数统计；无 UI 依赖，可单独测试 |
| `src/model.rs` | 视图共享的控制器，按变化频率拆分信号，打字只重组显示它的视图 |
| `src/views` | 窗口外壳、侧栏、文稿列表、编辑器、命令面板 |
| `src/markdown_syntax.rs` | 编辑器的 Markdown 着色（接入 Cove `syntax_color_at_utf8_offset`） |
| `src/menu_bar.rs` / `src/shortcuts.rs` | 原生菜单栏与快捷键 |
| `src/theme.rs` | 纸张色浅色主题、暖炭色深色主题、编辑器字体 |
| `Platforms` | macOS / iOS / Windows / Linux / Android 清单与打包资源 |
| `xtask` | 复用 Cove 的 `cove-package` 打包、签名与装机 |

## 构建

`../Cove` 必须存在并完成过一次 `cargo cove setup`。

```sh
cargo build --workspace
cargo run -p ruwriter
cargo test --workspace
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

⌘ 用于 Apple 平台，其他平台为 Ctrl。

| 快捷键 | 动作 |
| --- | --- |
| ⌘N / ⌘O / ⌘S | 新建 / 打开 / 保存 |
| ⇧⌘E | 导出 Markdown |
| ⌘P | 预览 |
| ⇧⌘F / Esc | 进入 / 退出专注模式 |
| ⌘\\ | 侧栏 |
| ⌘K | 命令面板 |
| ⌘= / ⌘- / ⌘0 | 放大 / 缩小 / 实际大小 |
| ⌘1–⌘4 | 全部文稿 / 置顶 / 最近 7 天 / 废纸篓 |

## 数据位置

`library.json` 与 `documents/` 位于各平台的用户文档目录；`RUWRITER_DATA_DIR` 可覆盖根目录。每次编辑即时原子落盘，退出不丢内容。资料库内的文件在保存时以标题命名（`标题-1a2b3c4d.md`）；通过「打开」或系统「打开方式」导入的外部文件写回原路径、保留原名。旧版索引缺少的摘要与字数在启动时补齐。

## 已知限制

- 编辑器只能着色，不能改变字号或字重，标题以颜色区分。
- 外部文件移到废纸篓后永久删除只移出索引，不删除磁盘文件。
- 系统外观在 macOS、Windows、iOS、Android 上跟随；Linux 由 SDL 报告未知时按浅色处理。
- 本机只验证了 macOS；iOS / Android / Windows / Linux 的完整 UI 未在本机交叉编译（Cove 原生依赖需要对应工具链），核心 crate 已在这些目标上通过 `cargo check`。
