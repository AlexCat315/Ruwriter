# Ruwriter v0.5.0

这一版加入 AI 写作助手，并修掉 Windows 安装包缺 VC++ 运行库导致的启动失败。

## AI 助手

- 入口在编辑器工具栏「AI 助手」，也能从命令面板打开。直接写要求，或选润色、续写、摘要、提纲。
- 结果可以复制、插入或替换原选区；写入走可撤销通道，草稿写回前能恢复初稿。
- **模型**：可保存多个模型。本机用 Ollama / LM Studio，或填 OpenAI 兼容服务的 HTTPS 地址；「获取模型列表」直接从服务拉取。DeepSeek 预设会填好地址，同一地址下的模型共用当前会话的密钥。
- **密钥不落盘**：密钥只存在于本次会话，模型配置与选择会保存；重启后每个需要密钥的服务输入一次。认证失败可直接从结果打开对应配置。
- **并行选模型**：最多四个服务或模型同时生成；各自显示进度与结果，切换预览后只插入选中的版本。
- **续写**：显示接写位置，下一步剧情可留空或指定动作与停点。参考前文时先整理最多 1,800 字的已发生事件与下一步，再写正文；长篇检索只读光标之前的内容。明显重复会尝试修正一次，仍重复则在草稿里提示。
- **查阅文稿**：模型支持工具调用且开启「允许模型查阅文稿」时，可限定本文稿、作品文件夹或整个资料库。助手能分页查目录、按关键词找线索、回读原文；答复下「原文来源」列出片段及文稿、章名、行号。搜索在本机运行，模型只收到授权片段和工具返回的资料。
- 上下文默认「仅片段」；选区长度合适时默认附带选区，光标前文按需开启。长篇可用章名或 Markdown 标题建立目录。

## Windows：安装包自带运行库

- 0.4.1 / 0.4.2 的 Windows 包只装了 `ruwriter.exe` 与资源，exe 动态链接 MSVC 运行库，没装过 Visual C++ Redistributable 的机器启动会报「找不到 MSVCP140.dll」。
- 根源在 Cove 打包工具：`cargo xtask package windows` 不产出运行库 DLL。本版从 Cove 侧修复，`bundle_windows` 会把 `msvcp140*.dll`、`vcruntime140*.dll`、`concrt140.dll` 从最新 `Microsoft.VC14*.CRT` 复制到应用目录；app-local 部署，无需用户装 Redistributable 或提权。
- 发布流程新增校验：Windows 构建后用 `dumpbin /dependents` 核对 `ruwriter.exe` 依赖的每个 VC 运行库都在产物目录，缺一个就让打包失败。

## 下载

| 平台 | 文件 |
| --- | --- |
| macOS Apple 芯片 | `Ruwriter-v0.5.0-macos-arm64.dmg` |
| macOS Intel | `Ruwriter-v0.5.0-macos-x86_64.dmg` |
| Windows x64 | `Ruwriter-v0.5.0-windows-x64-setup.exe` |
| Linux x64 | `Ruwriter-v0.5.0-linux-x64.deb` |

### 安装说明

- macOS：打开 `.dmg`，把 `Ruwriter.app` 拖进「应用程序」。当前包没有 Apple 开发者签名与公证，首次打开请右键「打开」，或到「系统设置 → 隐私与安全性」允许。
- Windows：运行 `Ruwriter-v0.5.0-windows-x64-setup.exe` 安装。未签名，SmartScreen 提示时选择「更多信息 → 仍要运行」。
- Linux：`sudo apt install ./Ruwriter-v0.5.0-linux-x64.deb`，需要系统具备 Vulkan 与常用 X11 / Wayland 图形库。
- iOS 与 Android 需要各自的签名材料，本次不提供移动端安装包。

下载后可用同一 Release 里的 `SHA256SUMS.txt` 校验文件完整性。

## 已知限制

- 云同步依赖 GitHub CLI；未安装 `gh` 时面板会给出安装与登录提示。
- Gist 单文件有平台上限，超大文稿或附件不适合作为同步载体。
- AI 助手依赖外部模型服务；本机服务未运行时连不上，服务地址需可访问。密钥只在会话内保存，重启后需重新输入。
- macOS 包最低系统版本由 Cove 打包配置决定，目前为 macOS 26。
- 没有代码签名，首次运行会被系统拦截一次。
- Windows 需要 Vulkan 运行时（通常随显卡驱动安装）；虚拟机或缺显卡驱动可能报 `vulkan-1.dll` 缺失。
