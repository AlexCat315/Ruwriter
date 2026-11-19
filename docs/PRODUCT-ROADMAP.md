# Ruwriter 产品能力地图与演进路线

> 调研时间 2026-11。对标对象：Scrivener、Ulysses、Obsidian、Logseq、Typora、Novelcrafter、番茄作家助手、阅文作家助手。
> 目的不是把功能堆满，而是在「本地文件即真相」的前提下，补齐长篇写作真正用得上的那一层。


> **2026-11-19 方向修正**：核心用户是写小说、做文字工作的人，不是 Markdown 用户。产品默认进入小说模式：纯文本、新稿 `.txt`、章节靠「第一章 雨夜」这类行、编辑器不显示也不插入任何记号；Markdown 降为可选模式。早期围绕 Markdown 的定位作废。码字目标与统计看过真机后被判定为「没用」，入口已移除；继续投入的方向是正文输入、长文性能、章节组织与角色/设定等写作现场的东西。已有落地：段首缩进走排版层，正文每段首行右移两字、折行回左，章节标题行顶格；正文与导出文件保持纯文本，不写入全角空格或 Tab。

---

## 一、这个赛道其实有四条不同的产品线

| 流派 | 代表 | 内核能力 | 它解决的问题 |
| --- | --- | --- | --- |
| 长篇写作套件 | Scrivener | Binder 活页夹、Corkboard 卡片墙、Outliner 大纲、Scrivenings 合并编辑、Compile 编译、目标与写作历史、标签/状态/自定义元数据、集合（虚拟分组）、快照、四窗分屏 | 一本书太大，一屏装不下：先拆成场景写，再拼回整本 |
| 纯 Markdown 写作 | Ulysses、iA Writer、Typora | sheet/group 文库、标记式写作、专注模式、打字机滚动、目标与截止日、语法样式检查、多格式导出、发布到博客 | 让界面消失，只剩文字与节奏 |
| 本地知识库 | Obsidian、Logseq | `[[双链]]` 与反链面板、局部图谱、Canvas、块引用、全文查询、插件生态、本地 vault、版本历史、Sync | 让写下来的东西彼此相连，可被再次找到 |
| 网文平台工具 | 番茄 / 阅文 / 七猫作家助手 | 大纲·角色·设定模板、错别字与内容风险检测、AI 扩写/改写/续写/起名、码字日历与目标、多端同步与历史版本、定时发布、作品数据 | 日更节奏下的产能、合规与读者反馈闭环 |
| AI 小说工作台 | Novelcrafter | Codex 设定库（关系、进展、跨系列共享）、Grid/Matrix 情节布局、场景摘要、Scene Beats、自定义提示词、本地模型（Ollama / LM Studio）、修订历史 | 让 AI 记住整本书的设定，而不是每次从零对话 |

四类产品的交集，就是「一个能写长篇的编辑器的必要能力」：

1. 把长文拆成可管理单元，并能随时拼回整体；
2. 写作时有结构感（大纲、状态、进度），而不是只有一页白纸；
3. 设定与资料伸手可及，且不会写丢；
4. 数据是自己的，格式是开放的；
5. 有节奏与反馈（字数目标、日历、统计）。

---

## 二、能力域拆解（按「谁有」标注）

### A. 文件与资料库
- 以普通文件夹/文件为库，不锁进私有数据库（Obsidian、Typora、Ruwriter）
- 集合/智能列表：全部、置顶、最近、废纸篓（Ruwriter、Scrivener Collections）
- 层级文件夹 + 拖拽重排（Scrivener Binder、Obsidian、Ruwriter 部分）
- 外部文件直接打开并写回原路径（Ruwriter、Typora）
- 文件变更监控与冲突另存（Ruwriter 已有）

### B. 编辑器内核
- Markdown 语法着色 / 可关闭（Ruwriter、iA Writer）
- 格式命令与快捷键、可撤销（Ruwriter、Ulysses、Typora）
- 查找替换、匹配计数、上一个/下一个（Ruwriter、多数产品）
- 回车续行（列表、引用、有序编号）（Ruwriter）
- 所见即所得（Typora）／源码标记（Ulysses）
- 打字机滚动、当前行居中（Scrivener、Ulysses、iA Writer）
- 字号/行距/字体（含无障碍字体）（Scrivener、Novelcrafter；Ruwriter 只有字号）

### C. 长文结构（长篇写作的分水岭）
- 卷 / 章 / 场景三级组织（Scrivener Binder、Novelcrafter Grid）
- 每个单元有摘要（synopsis）与状态（草稿/修订/定稿）（Scrivener、Novelcrafter）
- 合并视图：把多个单元当一篇编辑（Scrivener Scrivenings）
- 大纲视图：层级 + 摘要 + 字数（Scrivener Outliner、Novelcrafter Outline）
- 卡片墙 / 情节矩阵（Scrivener Corkboard、Novelcrafter Matrix、Dabble Plot Grid）
- 拖拽改变顺序即改变正文顺序（Scrivener）
- 大纲模板（三幕、雪花、诸天流、莲花地图法）（阅文作家助手）

### D. 设定库（Story Bible / Codex）
- 人物、地点、组织、物品、概念条目（Novelcrafter Codex、阅文「角色/设定」）
- 别名与提及识别，正文中高亮并可点击（Novelcrafter Mentions）
- 关系网、随剧情推进的「进展」（Progressions）（Novelcrafter）
- 跨书共享（系列 Codex）（Novelcrafter）
- 模板化角色卡/世界观卡（阅文、番茄）

### E. 检索与引用
- 标题 + 正文全文检索（Ruwriter、全部产品）
- 搜索语法（tag: / folder: / status:）（Obsidian、Scrivener）
- 双链与反链面板（Obsidian、Logseq）
- 块级引用与嵌入（Logseq）
- 图谱视图（Obsidian；诊断价值 > 日常价值）

### F. 写作度量
- 字数、词数、阅读时间、选中字数（Ruwriter 已有，中英混排处理正确）
- 单篇 / 全书 / 单次会话目标（Scrivener、Ulysses、番茄）
- 每日码字日历、连续天数、写作历史（Scrivener Writing History、番茄码字日历）
- 进度条与截止日（Ulysses）

### G. 版本与安全
- 即时自动保存 + 原子写入（Ruwriter 已有）
- 手动/自动快照与历史版本恢复（Scrivener Snapshots、Novelcrafter Revision History、番茄历史版本）
- 外部修改检测与双版本保留（Ruwriter 已有，做得比多数产品细）
- 回收站（Ruwriter 已有）
- 备份/同步（Obsidian Sync、iCloud、Dropbox、Git）

### H. 导出与发布
- Markdown（Ruwriter 已有）
- HTML / PDF / DOCX / EPUB（Ulysses、Scrivener Compile、Novelcrafter）
- 提交稿件格式控制（章标题、分隔符、页眉、前置页）（Scrivener Compile）
- 一键发布到博客/平台（Ulysses → WordPress/Ghost/Medium；作家助手 → 平台）

### I. AI 辅助
- 续写 / 扩写 / 改写 / 起名 / 卡文（番茄、阅文「妙笔」）
- 场景摘要、章纲提取（Novelcrafter、阅文）
- 带设定上下文的问答与角色访谈（Novelcrafter Chat + Codex）
- 自定义提示词与参数、提示词预览（Novelcrafter）
- 本地模型（Ollama / LM Studio），隐私不出机器（Novelcrafter）
- 内容风险/敏感词预检（番茄「风险提示」）

### J. 平台与体验
- 多端（桌面 + 移动）与同步（全部商业产品）
- 原生菜单、快捷键、命令面板（Ruwriter 已有）
- 浅色/深色/跟随系统、纸张色（Ruwriter 已有）
- 多窗口 / 分屏参考（Scrivener 四窗、Novelcrafter 分栏）
- 无障碍（字体、行距、对比度）

---

## 三、Ruwriter 现状矩阵

| 能力 | 现状 | 说明 |
| --- | --- | --- |
| 文件即真相、库即文件夹 | ✅ | `state.json` 只存无法写进文件的派生信息，换工具可直接打开 |
| 集合 / 置顶 / 最近 / 回收站 / 搜索 / 排序 | ✅ | 已覆盖主流资料库骨架 |
| 编辑器着色、可关、格式命令、查找替换、回车续行 | ✅ | 比 Typora 弱在所见即所得，比 Ulysses 弱在标记细节 |
| 预览（并排/替换）、专注模式、命令面板、大纲跳转 | ✅ | 大纲目前只在命令面板里，没有常驻面板 |
| 字数/词数/阅读时间/选中字数（中英混排） | ✅ | 基础度量扎实，缺目标与日历 |
| 即时保存、原子写、外部冲突另存、文件监控 | ✅ | 这是本项目最硬的一块，别退化成轮询全量重载 |
| 导出 Markdown、外部文件写回 | ✅ | 导出格式单一 |
| 长文结构：卷/章/场景语义、摘要、状态 | ⚠️ 部分 | 只有文件夹层级，没有写作语义；顺序靠文件名/排序，不能拖拽 |
| 合并编辑（多篇当一篇看） | ❌ | Scrivener 的核心体验，目前完全缺失 |
| 设定库 / 提及 / 关系 | ❌ | 长篇写作最容易被感知的缺口 |
| 写作目标 / 码字日历 / 写作历史 | ❌ | 只差一层增量记账 |
| 版本快照 / 历史恢复 | ❌ | 只有冲突另存，没有「回到两小时前」 |
| 双链 / 反链 / 图谱 | ❌ | 定位是写作软件，可延后 |
| 导出 HTML / DOCX / EPUB | ❌ | 投稿与发布绕不开 |
| AI 辅助 | ❌ | 应做「可选、本地优先」，不做云端绑定 |
| 同步 / 多端体验 | ⚠️ 部分 | 依赖 iCloud/Dropbox/Git，移动端 UI 已有布局但未验收 |

---

## 四、优先级：先补哪三刀

排序依据 = 写作长文时的痛感 × 实现成本 × 与「本地文件即真相」的兼容度。
凡是会把内容锁进私有数据库、或让 Markdown 不再是唯一真相的功能，一律不做或降级为派生缓存。

### P0（0.3）：把「一页白纸」变成「一本书」

1. **文稿元数据：摘要 / 状态 / 标签**（落在 `state.json`，不进正文）— **已落地**
   - 理由：列表已有两行摘要，只需把它变成可编辑的 synopsis；状态与标签是后面大纲、看板、检索的地基。
   - 实现：`Library::Meta` 扩 `synopsis: Option<String>`、`status: DocStatus`、`tags: Vec<String>`；列表行与右键菜单增加「编辑文稿信息」；命令面板增加「设置状态」。
   - 验收：改元数据不触碰正文；文件被外部改名后元数据跟随 id 迁移；旧 `state.json` 平滑升级（`STATE_VERSION` → 3）。

2. **常驻大纲面板（Outline）** — **已落地**
   - 理由：长篇写作必须在「我在第几章」和「整本长什么样」之间来回切换。
   - 实现：`crates/core/outline.rs` 解析 ATX 标题，跳过围栏代码块与缩进代码块，产出字符偏移 `(offset, level, title)`；视图每次重算（十万字级别为毫秒量级），点击条目走 `navigate(offset)` 并交还焦点；当前小节由光标位置反查。
   - 视图：编辑器右侧固定 240pt，可收起；宽度 < 1180pt 自动折叠，命令面板保留跳转入口。
   - 验收：10 万字文稿滚动不卡；标题含 `#` 与代码块不误判；点击定位后光标落在标题行首。

3. **写作目标 + 每日码字日历**
   - 理由：网文/长篇作者的第一需求是「今天写了多少、还差多少」。
   - 实现：`crates/core/src/history.rs`，在 `.ruwriter/history.json` 记 `date → {written, sessions, per_doc}`；增量 = 本次字数 − 上次保存字数，日内累计下限 0；提供 `daily_target`、`session_target`、`streak`。状态栏右侧显示目标进度，命令面板开「写作统计」页（近 30 天柱状图用 Cove 基础图形绘制）。
   - 验收：删字不扣当日进度；跨天按本地时区切分；外部修改文件不计入「自己写的」。

4. **快照与历史恢复**
   - 理由：原子保存解决了「断电丢字」，解决不了「改坏了想退回去」。
   - 实现：`.ruwriter/snapshots/<doc-id>/<ISO8601>.md`；触发点 = 手动（⇧⌘S）、打开前、每小时最多一次；每篇保留最近 30 份 + 每日首份；恢复走与冲突另存相同的写入通道（先写当前版本快照再覆盖）。
   - 验收：恢复后可撤销；快照目录被外部删除时自动重建；不把快照算进字数统计。

### P1（0.4）：设定库与投稿出口

5. **设定库 Codex**：`codex/` 下每个条目一个 `.md`（YAML frontmatter：`type/name/aliases/tags/summary`），库内独立索引；编辑器把条目名与别名做高亮与 ⌘点击跳转；条目详情页可编辑；正文里「提及」列表可反查。
   - 关键约束：设定条目也是普通 Markdown，删掉索引可以重建，不做 SQLite。
6. **卷 / 章 / 场景语义**：文档增加 `kind`（卷/章/场景/笔记）与 `order`，列表切到「结构视图」时按树展示、可拖拽排序（改 `order` 与文件名前缀，不移动文件内容）；合并查看用只读拼接的「全书」视图（写回仍按单篇，避免误改）。
7. **导出 HTML / DOCX / EPUB**：`crates/core/src/export/`，Markdown → 中间 AST（自写解析或引 `pulldown-cmark`），再分别渲染 HTML、最小 OOXML、EPUB 3（zip + mimetype + nav）。Scrivener 式「编译选项」（是否含摘要、章节标题级别、场景分隔符）。
8. **模板与新建流程**：小说 / 短篇 / 博客 / 笔记四套模板（含 `# 卷` `## 章` 骨架与元数据预设），新建时可选。
9. **搜索语法**：`tag:`、`status:`、`folder:`、`title:`、`is:pinned`，与现有标题正文检索共存。

### P2（0.5+）：连接与智能

10. **双链与反链**：解析 `[[标题]]`，建立内存索引，编辑器提供补全与反链面板；不做全库图谱（诊断价值低于维护成本）。
11. **AI 辅助（本地优先）**：`crates/ai` 走 OpenAI 兼容协议，预设 Ollama / LM Studio；功能限定为选区改写、续写、场景摘要、章纲提取；请求前展示将发送的上下文，默认不把全文上传；没有可用端点时功能整体隐藏，不诱导联网。
12. **内容风险预检**：本地词表 + 规则（不做平台审核模拟），逐段高亮，仅提示不阻断。
13. **同步与移动端**：先做「库目录放在 iCloud/Dropbox」的可用性验证与冲突策略，再接 WebDAV；移动端补齐大纲、统计、设定库的窄屏布局。

### 明确不做
- 私有数据库作为唯一真相（破坏可迁移性）。
- 插件系统（Cove 生态内没有插件宿主，成本远大于收益）。
- 云端账号体系与在线协作（与本地优先冲突，且需要服务端）。
- 平台发布对接（番茄/起点接口不对第三方开放，只做导出）。

---

## 五、版本节奏与验收口径

| 版本 | 主题 | 内容 | 验收 |
| --- | --- | --- | --- |
| 0.3 | 一本书 | 元数据、大纲面板、目标与日历、快照 | 10 万字文稿连续编辑 30 分钟无卡顿；`cargo test --workspace` 覆盖新增 core 模块；旧数据目录无损升级 |
| 0.4 | 设定与出口 | Codex、卷章结构、HTML/DOCX/EPUB、模板、搜索语法 | 用一本 20 章样例书走完「建库 → 设定 → 写作 → 导出 EPUB」，EPUB 在 Apple Books 与 Calibre 中可读 |
| 0.5 | 连接 | 双链、AI 本地端点、风险预检、同步验证 | 断网状态下除 AI 外全部可用；AI 关闭时二进制与体积不变 |
| 1.0 | 稳定 | 移动端验收、无障碍、安装包与自动更新 | macOS/Windows/Linux 真机通过；iOS/Android 至少各一台真机 |

每项落地的共同约束（写进代码评审清单）：
1. 新状态要么进 Markdown 自身，要么进 `.ruwriter/` 且可删可重建；
2. core 不依赖 UI，新增能力先在 `crates/core` 写测试；
3. 任何写盘路径继续走 `files::write_document` 的原子写与 Stamp 校验；
4. 编辑器只做「着色 + 导航」，不引入富文本模型，避免与 Markdown 真相分叉。

---

## 六、参考来源

- Scrivener 功能页：https://www.literatureandlatte.com/scrivener/features?os=MacOS
- Scrivener / Ulysses / Dabble 对比：https://www.creativewriting-prompts.com/scrivener-vs-ulysses-vs-dabble/
- Ulysses 官方：https://ulyssesapp.com/index.html
- Obsidian 官方与帮助：https://obsidian.md/ ，https://help.obsidian.md/Obsidian
- Obsidian / Logseq 能力对比：https://anarlog.so/blog/markdown-note-taking-apps/
- Novelcrafter 功能总览：https://www.novelcrafter.com/features
- Novelcrafter 本地模型与 AI 接入：https://www.novelcrafter.com/help/faq/ai-connections/does-novelcrafter-provide-its-own-ai
- 阅文作家助手：https://www.yuewen.com/app/?type=appzj
- 番茄「风险提示」：https://fanqienovel.com/writer/zone/article/7553258797649625113
- 番茄 AI 工具箱（扩写/改写/续写/起名/卡文锦囊）：https://fanqienovel.com/writer/zone/article/7455639656860762137
