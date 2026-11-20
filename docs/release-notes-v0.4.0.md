# Ruwriter v0.4.0

面向小说作者与文字工作者的跨平台写作软件。v0.4.0 加上云备份与跨设备同步：正文仍然是本地纯文本，云上只存私有 Gist，不改变任何写作格式。

## 这一版的重点

- **私有 Gist 云同步**：每个资料库自动对应一个私有 Gist，第一次同步创建，之后按资料库标识自动找回；换电脑不需要记 Gist ID。
- **不填 Token**：登录复用本机 GitHub CLI 的登录态（终端执行一次 `gh auth login`）。Ruwriter 每次同步只读取一次凭据，用完即丢，不落盘、不写进资料库。
- **纯文本仍是唯一真相**：Gist 里一篇文稿一个扁平 `doc_<uid>.txt`，文件夹、标题、摘要、状态、标签、置顶写在 `ruwriter-manifest.json`；本地 `documents/**.txt` 不被云同步改写。
- **三方合并**：本地、上次同步基线、云端逐篇比较。只改一边就同步那一边；两边都改时本地版本上传，云端版本另存为「原名（云端冲突 日期 时间）.txt」；云端删除且本地未改时移入废纸篓；本地删除会写 tombstone，避免第三台设备把文稿复活。
- **空文稿边界**：Gist API 不接受空文件内容，空文稿只保留清单条目、不写正文文件，下载时按清单哈希还原为空，不会写入占位字符。
- **入口**：`文件 → 云同步…`，或 `⌘K` 输入「云同步」。面板显示 GitHub CLI 登录账号、上次同步时间与结果。

## 使用前

云同步需要本机安装 [GitHub CLI](https://cli.github.com/) 并完成一次登录：

```sh
gh auth login
```

按提示选择 `GitHub.com` → `HTTPS` → `Login with a web browser`，在浏览器授权即可。`gh auth login` 默认包含 `gist` 权限。Ruwriter 不会保存 Token，也不会要求你粘贴 Personal Access Token。

## 下载

| 平台 | 文件 |
| --- | --- |
| macOS Apple 芯片 | `Ruwriter-v0.4.0-macos-arm64.zip` |
| macOS Intel | `Ruwriter-v0.4.0-macos-x86_64.zip` |
| Windows x64 | `Ruwriter-v0.4.0-windows-x64.zip` |
| Linux x64 | `Ruwriter-v0.4.0-linux-x64.tar.gz` |

### 安装说明

- macOS：解压后把 `Ruwriter.app` 拖进「应用程序」。当前包没有 Apple 开发者签名与公证，首次打开请右键「打开」，或到「系统设置 → 隐私与安全性」允许。
- Windows：解压整个文件夹后运行 `Ruwriter.exe`。未签名，SmartScreen 提示时选择「更多信息 → 仍要运行」。
- Linux：解压后运行目录里的 `Ruwriter`，需要系统具备 Vulkan 与常用 X11 / Wayland 图形库。
- iOS 与 Android 需要各自的签名材料，本次不提供移动端安装包。

下载后可用同一 Release 里的 `SHA256SUMS.txt` 校验文件完整性。资料库默认位于用户数据目录，可用 `RUWRITER_DATA_DIR` 指定；正文是普通 `.txt` / `.md` 文件，`.ruwriter/` 只保存派生状态与快照。

## 已知限制

- 云同步目前依赖 GitHub CLI；未安装 `gh` 时面板会给出安装与登录提示。后续版本会补应用内一键授权。
- Gist 单文件有平台上限；超大文稿或附件不适合作为同步载体。
- macOS 包最低系统版本由 Cove 打包配置决定，目前为 macOS 26。
- 没有代码签名，首次运行会被系统拦截一次。
- 退出前不足约 1.2 秒的光标移动可能来不及写入上次位置。
