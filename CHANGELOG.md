# Changelog

## 未发布

- 文稿信息：每篇文稿可设自己的摘要、状态（草稿 / 修订中 / 定稿 / 归档）与标签，保存在 `.ruwriter/state.json`（状态版本升到 3），Markdown 文件一个字节都不改。列表行显示状态与标签，摘要优先于正文摘录；搜索覆盖摘要与标签。入口：列表右键「文稿信息…」、⌘K 命令面板、macOS「文件」菜单。
- Markdown 格式化：加粗、斜体、删除线、行内代码、链接、标题循环（H1→H2→H3→正文）、无序列表、有序列表、引用。选区包裹可一键取消；行命令作用于选区覆盖的每一行；空选区插入成对记号并把光标放在中间。
- 编辑器工具栏新增格式按钮组（窄窗口收为「格式」菜单，专注模式隐藏）；macOS 菜单栏新增「格式」菜单；命令面板新增格式与排序命令；新增快捷键 ⌘B / ⌘I / ⇧⌘X / ⌘E / ⇧⌘K / ⇧⌘H / ⇧⌘7 / ⇧⌘8 / ⇧⌘9。
- 文稿列表支持排序：按编辑时间 / 创建时间 / 标题 / 字数，选择持久化；置顶始终在前（废纸篓除外）。列表行在集合视图下显示所属文件夹。
- 状态行显示选中字数；中英文混排时同时显示字数与词数。
- 空状态升级：图标 + 说明 + 「新建文稿」按钮（搜索无结果、废纸篓与置顶除外）。
- 回车续行：列表与引用行末回车自动带出下一项前缀（有序列表递增编号）；空项上回车结束列表；行中回车拆分并保留前缀。
- 文内查找替换（⌘F）：查找栏带匹配计数、上一个 / 下一个（⌘G / ⇧⌘G）、替换与全部替换；输入即高亮全部匹配并选中当前匹配，焦点留在查找框；Esc 先关查找栏。替换走可撤销编辑通道。
- 格式化、替换、回车续行均保留选区落点且可撤销（⌘Z 逐步回退）。
- 命令面板新增「跳转到大纲」：按 Markdown 标题列出全文大纲，选中即定位；新增查找与 Markdown 高亮开关命令；macOS「显示」菜单同步新增查找与高亮项。
- Markdown 高亮可关闭（纯文本模式），写小说 / 常规文本时记号不再着色；设置持久化。
- 修复：适配当前 Cove，移除已删除的 `WindowBackdrop` 窗口材质配置。

### Cove 改动

- 修复 `TextField` 的 `on_caret_change` 只写不读的死回调：所有提交点（输入、粘贴、指针、撤销、导航、编辑请求）现在都发布光标与选区。
- 新增 `TextFieldEditRequest`：程序化编辑进入字段自身的撤销括号，外部改文不再清空撤销栈。
- 新增 `TextFieldNavigationRequest::caret_request_anchor` 与 `caret_request_focus`：导航可带选区锚点，也可只移光标不抢焦点（查找栏保持键盘）。
- `TextFieldCodeEditing::occurrences` 此前从未渲染：现在在字形与选区带之下绘制匹配淡色底纹。
- 修复 `TextField` 画布身份缺口：仅匹配底纹 / 高亮开关 / 装饰变化时重算不重绘，替换后留下过期底纹；身份纳入装饰与代码编辑设置。

## 0.2.0 — 2026-11-09

界面按写作软件重新设计：资料库侧栏 · 文稿列表 · 编辑器三栏，随窗口宽度收为两栏和单栏。

- 资料库：全部文稿、置顶、最近 7 天、废纸篓四个集合，侧栏显示计数；列表显示标题、两行摘要、相对编辑时间与字数，置顶在前；搜索标题与正文。
- 文稿操作：右键置顶 / 导出 / 移到废纸篓；废纸篓内恢复、永久删除、清空。移走正在编辑的文稿时打开相邻文稿，没有剩余时新建一篇。
- 编辑器：正文限制在约 40 个全角字的可读宽度并居中，整栏滚动；Markdown 记号淡化，标题、引用、代码、链接着色；字号 13–26pt 可调；每篇文稿独立的光标与撤销历史。
- 预览：宽窗口与编辑器并排，窄窗口替换编辑器。专注模式只留正文。
- ⌘K 命令面板覆盖全部命令与文稿；macOS 原生中文菜单栏；新增快捷键 ⇧⌘E、⌘\\、⌘K、⌘=/-/0、⌘1–4、Esc。
- 外观：纸张色浅色主题与暖炭色深色主题，默认跟随系统；macOS 侧栏使用系统材质。
- 导出 Markdown 使用系统保存面板；系统「打开方式」打开的文件导入资料库。
- 资料库文件保存时以标题命名；设置（侧栏、预览、外观、集合、字号）持久化；旧版索引启动时补齐摘要与字数。
- 手机宽度：列表与编辑器单栏切换，遵守底部安全区与软键盘高度。

### Cove 改动

为本版本补充与修复的 Cove 能力（详见 Cove `CHANGELOG.md`）：`TextField` 正文内边距、`Environment::system_appearance`、macOS 菜单栏安装崩溃与标题、空格溢出换行、软换行字段的横向滚动、`MarkdownView` 圆点与引用换行、`CommandPalette` 行对齐。

### 验证

- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` 通过；核心 29 项、应用 9 项测试。
- `ruwriter-core` 在 iOS、iOS 模拟器、Android、Windows、Linux 目标上 `cargo check` 通过。
- macOS 原生运行经 DevTools 与屏幕截图验收：三栏 / 收起侧栏 / 预览替换与并排 / 专注模式与 Esc / 命令面板 / 浅色与深色（跟随系统）/ 新建并输入 / 移到废纸篓 / iPhone 预设下单栏切换与返回 / 右键菜单显示。

### 未覆盖

- macOS 右键菜单为原生 `NSMenu`，DevTools 注入的点击无法操作，只验证了显示。
- iOS / Android / Windows / Linux 完整 UI 未在本机编译或运行；DevTools 文本注入不验证原生键盘与输入法。

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
