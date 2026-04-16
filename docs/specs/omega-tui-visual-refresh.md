---
content_revision: 101
created: 2026-04-08
generation_id: gen_000017_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 17
related_prds: []
source_doc_id: "spec:docs-specs-omega-tui-visual-refresh"
status: draft
supersedes: []
updated: 2026-04-08
---

# Omega TUI Visual Refresh Specification

## Overview

当前 `omega-tui` 的信息架构已经基本成形：`Response`、`Sidebar`、`Input context bar`、`Bottom status bar` 与 `Overlay` 都有明确职责。但从整体视觉上看，界面仍偏向“功能框堆叠”而不是“可长期使用的工作台”：面板层次较平、右侧辅助区缺少 dashboard 感、标题与正文的视觉权重过近、底部条带虽然信息完整但气质仍偏工程默认值。

本规格定义一轮面向“终端工作台”方向的视觉刷新。参考目标是更克制的深色层次、更强的侧栏卡片感、更清晰的标题 hierarchy，以及把强调色保留给 focus / running / warning 这类真正需要注意的状态，而不是让所有元素都在抢注意力。

补充的审美方向是：把类似 Linear、Vercel、GitHub CLI 与 Apple Dark Mode 的“像素级克制感”迁移到终端环境。这里的重点不是做成炫技色板，而是做成高信噪比的 industrial terminal UI：原始日志退到次级层，可读报告浮到主层；深色背景稳定、亮色克制、区块清晰、排版精确。

## Goals

- 把 TUI 从“多个并列 box”提升为有壳层、分区和节奏的统一控制台界面。
- 让 `Sidebar` 更像任务与运行态摘要面板，而不是一组样式完全相同的列表框。
- 让 `Response`、`Sidebar`、`Input`、`Context/Status` 条带拥有稳定、可区分的 surface 层次。
- 把视觉令牌继续收敛进 `omega-theme`，避免新一轮美化再次回到渲染层硬编码。
- 为后续密度、主题预设和更高质量的 timeline polish 预留稳定任务切面。
- 让最终视觉明确落到 `Modern TUI / Rich CLI` 路线，而不是泛化的“深色主题”。
- 让终端输出默认更像“面向用户的结构化报告”，而不是“带颜色的执行日志”。

## Non-Goals

- 不重写当前 `Response / Sidebar / Overlay` 信息架构。
- 不在本轮引入图形化组件、鼠标拖拽布局器或终端外字体方案。
- 不把 message polish、delivery observability 或 modal keymap 的已实现能力推倒重做。
- 不把高饱和霓虹、彩虹状态色或装饰性渐变引入主工作界面。
- 不为了“Apple-like”而牺牲终端环境下的可读性、稳定性与信息密度。

## Aesthetic Direction

### Style Name

推荐将当前方向显式命名为：`Dark Industrial Report Console`。

它融合三类参考，但不直接复制任何单一产品：

- `Modern TUI / Rich CLI`: Markdown、表格、列表、状态分组、语义高亮。
- `AI Assistant / CLI Tooling`: 高信噪比、结果优先、尽量隐藏底层 JSON 和路由噪音。
- `Dark Minimalist`: 深色分层、克制色彩、精确留白、偏工业级而非玩具化。

### Experience Principles

- `Result-first`: 默认先展示结论、变更、验证、使用方式，再暴露原始细节。
- `Semantic color only`: 颜色只服务结构、状态与重点，不服务装饰。
- `Block before line`: 用户先感知区块，再阅读行内容；先扫读，再深读。
- `Quiet chrome`: chrome 要稳定存在，但不能比正文更吵。
- `Terminal-native precision`: 接受终端限制，用间距、表格、标题、divider 和轻量边框达成高级感，而不是模拟 GUI。

### Reference Traits To Preserve

- 类似 Linear 的清晰信息层级和节制 badge。
- 类似 Vercel / GitHub CLI 的结果导向输出与高信噪比报告感。
- 类似 Apple Dark Mode 的稳定深色背景、温和高对比与长期阅读舒适度。

## Visual Direction

### Surface Hierarchy

- 主响应区使用稳定深色 panel surface，避免正文直接贴在终端默认背景上。
- `Sidebar` shell 比 `Response` 更暗一层，突出“辅助区”角色。
- `Sidebar` 内部 section 使用 card-like surface，与 shell 背景形成第二层分隔。
- `Input context bar`、输入框与 `Bottom status bar` 保持同一视觉家族，形成稳定底部控制带。

### Accent Discipline

- 标题文字使用更高对比的浅色，而不是与正文共享同一亮度。
- focus / active state 使用单一冷色强调，不让多个面板同时高饱和。
- warning / error / running 保留独立语义色，避免被纯装饰性颜色稀释。
- 默认配色应以 graphite / slate / charcoal 为主，accent 只占很小面积。
- 紫色、橙色、绿色等亮色若使用，必须绑定清晰语义：结构标题、核心指标、代码/文件或新增状态。

### Typography Without Fonts

终端没有真正的字体系统，因此“排版风格”必须通过结构模拟：

- section header 的层级要像编辑器里的二级标题，而不是普通加粗行。
- body copy 保持安静、连续、低干扰，避免每行都上色。
- 表格、列表、短命令、文件名与状态 token 要形成稳定的阅读节奏。
- 空行数量要克制且精确，既形成区块感，又不造成屏幕浪费。

### Block And Bento Feel

- `Response` 中的最终结果必须形成清晰的报告块，而不是一段大文本。
- `Sidebar` section 要更像紧凑的 summary cards，而不是堆叠的容器盒子。
- overlay/detail 允许显示更原始的信息，但主界面默认只显示压缩后的高价值摘要。
- 对长 turn，优先通过 section、table、summary row 和 collapsed details 形成类 bento 的浏览感。

### Sidebar Feel

- rail 更接近紧凑 badge strip，而不是纯文本链接列。
- 每个 section card 都应一眼可辨，适合承载 TODO、delivery、skills、document/memory 等运行态摘要。
- 后续 density tuning 应优先提升“单位高度内的信息价值”，而不是继续增加常驻面板数量。

### Report Surfaces

- `Results Summary`、`Changes Made`、`Verification`、`Usage`、`Optional Next Step` 这类 section 必须被当作主阅读对象，而不是附属装饰。
- 性能对比、测试结果、资源消耗等指标默认优先用表格承载。
- 原始 JSON、逐步路由日志、低层 execution trace 默认进入 detail/expanded 态，而不是主时间线。

## Phase 1 Baseline

`Task 15B-51` 对应本轮已落地的第一阶段视觉刷新：

- `omega-theme` 新增 surface 级视觉令牌：`panel_bg`、`sidebar_bg`、`sidebar_rail_bg`、`section_bg`、`title_fg`。
- 内建 dark theme 改为更明确的 slate/charcoal 分层，不再大量依赖 `Color::Reset`。
- `Response`、`Sidebar` shell、`Sidebar` rail、section card、输入框、context/status bar、overlay 全部切换到分层 surface 背景。
- `Sidebar` rail 选中项改为 badge-like 表现，展开但未选中项也保留弱强调。
- `Response` 与 sidebar section title 统一提升为更高权重标题样式。

这一步的目标不是“最终审美完成”，而是先把界面的基础气质从默认 box UI 拉到可继续打磨的 visual system。

## Planned Follow-Ups

### Task 15B-52: Sidebar dashboard density tuning

- 把 `Delivery`、`Todos`、`Skills`、`Document/Memory` 的摘要行进一步收敛为更像 dashboard card 的结构。
- 研究不同 section 的优先级、默认展开策略与 badge 信息密度，减少右侧滚动时的信息噪音。
- 把侧栏视觉方向进一步推进到“quiet bento dashboard”，用更少但更稳定的摘要元素替代重复标签和边框噪音。

### Task 15B-53: Response timeline rhythm refinement

- 提升 `Response` 内 routing / step / command / final answer / delivery summary 的纵向节奏。
- 强化 section 之间的“读写节奏”，让长任务 turn 更像可扫读的执行报告，而不是连续日志块。
- 该方向在下一轮已细化为独立的 `docs/specs/omega-tui-message-cards.md`，因为真正的消息卡片化需要 block-aware view model，而不只是 spacing polish。

### Task 15B-54: Theme presets and density modes

- 把这轮视觉方向沉淀为用户可覆盖的主题令牌基线。
- 评估 compact / comfortable 两档终端密度模式，以及未来多主题预设是否需要进入 `omega-theme`。
- 第一命名主题应明确继承 `Dark Industrial Report Console` 基线，而不是仅以通用 `dark` 语义存在。

## Beauty Tuning From The UI Baseline

`docs/specs/omega-tui-ui-reference.md` 已把当前实现状态盘清，但它也暴露了下一轮美化必须处理的四类差距：

1. **主调色板仍偏 editor theme**：当前同时存在青绿、蓝、黄、橙、紫等多强调色，虽然功能语义清楚，但整体仍更像代码编辑器主题，而不是极简控制台。
2. **区块切分仍偏靠边框而不是靠留白和面层**：当前 shell surface 已建立，但 panel 内部仍可继续做“边框减法”，让 sidebar/overlay 更接近 quiet bento card。
3. **meta / thinking / logs 仍偏显眼**：这些低信号信息虽然已经降噪，但还不够“默认隐身”。
4. **焦点切换与 overlay 景深还不够像窗口系统**：当前 focus 是边框级提示，overlay 也是功能可用但质感仍偏平面。

因此，下一轮视觉工作应显式分成下列子任务，而不是继续用单个“再打磨一下 UI”来吞掉所有细节。

### Task 15B-61: Monochrome foundation and single accent discipline

- **Status**: Completed
- **Completed**: 2026-04-08
- **Priority**: Medium
- **Complexity**: M
- **Dependencies**: Task 15B-51
- **Description**: 把界面收敛成黑白灰基础层 + 单一青绿色 accent 的视觉系统；heading、badge、warning/error、final answer、metric 等当前多彩 token 需要重新归并，避免 editor-theme 噪音。
- **Acceptance criteria**:
	- 主界面不再同时依赖蓝、绿、黄、橙、紫多条主强调线。
	- 重点状态仍可区分，但界面第一眼不再像代码编辑器配色。
	- `focus` / `insert` / `final answer` 的强调语义保持统一，不被次级色抢走注意力。

### Task 15B-62: Bento spacing, padding, and border subtraction

- **Status**: Completed
- **Completed**: 2026-04-08
- **Priority**: Medium
- **Complexity**: M
- **Dependencies**: Task 15B-52, Task 15B-61
- **Description**: 通过统一 panel 内边距、section 呼吸空间、内部边框减法和更稳的 card padding，把 TUI 的区块切分更多建立在“面层与留白”而非“线框”上。
- **Acceptance criteria**:
	- Sidebar/Response/Input/Overlay 在字符网格下仍能感到一致的呼吸感。
	- panel 内部的区块切分更接近 quiet bento card，而不是重复小框。
	- 窄宽度下也不会因为空间压缩而变回“文字紧贴边框”的工程默认值。

### Task 15B-63: Meta/thinking/log de-emphasis and result-first contrast

- **Status**: Completed
- **Completed**: 2026-04-08
- **Priority**: Medium
- **Complexity**: M
- **Dependencies**: Task 15B-58, Task 15B-61
- **Description**: 继续压低 meta、thinking body、log text 与 warning/error 大面积着色的视觉音量，把阅读主次彻底收回到 final answer、关键 metric 和当前 focus 上。
- **Acceptance criteria**:
	- thinking 与原始过程信息在默认阅读路径里显著退后。
	- warning/error 不再通过大面积高饱和正文制造疲劳，而是通过小面积语义前缀/徽标提示。
	- 用户第一眼看到的是结果和关键信号，而不是过程噪音。

### Task 15B-64: Focus dimming and overlay depth polish

- **Status**: Completed
- **Completed**: 2026-04-08
- **Priority**: Medium
- **Complexity**: M
- **Dependencies**: Task 15B-61, Task 15B-62
- **Description**: 把 focus/blur 层次与 overlay 景深继续往窗口系统级反馈推进，为非焦点区域增加更克制的失焦感，并让 overlay 通过细边、遮罩与阴影暗示更清晰的 Z 轴层次。
- **Acceptance criteria**:
	- Response 与 Sidebar 之间切焦时，非焦点区不只是边框变暗，而是整体视觉权重自然退后。
	- Overlay 在深色模式下拥有更明确的浮层感，而不是单纯内容框覆盖。
	- 交互反馈更“丝滑”，但不引入花哨动画或装饰性噪音。

## Readability Gaps After Card Foundation

`Task 15B-55 ~ 15B-57` 已解决 `Response` 侧的 card/section foundation，但界面整体仍有两个明显的后续阅读问题：

1. `Sidebar` 内的大量文本仍偏向“同一种行样式不断堆叠”，当 section 内既有标题、摘要、状态、路径、提示又有空状态文案时，用户很难快速建立扫描规则。
2. `Sidebar` 的默认展开策略、行高与截断策略还不够稳定；当多个 section 同时出现中等长度内容时，容易回到“什么都看得到，但什么都不够清楚”的状态。

这意味着下一轮不应只继续调 shell surface，而要把 `Sidebar` 内部文本当作独立的阅读系统来治理。

### Task 15B-59: Sidebar semantic row taxonomy and styling

- **Priority**: Medium
- **Complexity**: M
- **Dependencies**: Task 15B-51, Task 15B-52
- **Description**: 为 `Sidebar` 内的 `Delivery`、`Todos`、`Skills`、`Document/Memory`、`Logs` 定义统一的 row taxonomy，例如 summary row、metric row、status row、item title、meta note、empty state、action hint、log preview，并给这些行型赋予稳定的样式差异与语义色。
- **Implementation approach**:
	- 在 `omega-tui` 为 sidebar section 内容增加 row-kind-aware view model，而不是继续把所有内容都当普通字符串行渲染。
	- 在 `omega-theme` 增加 sidebar summary / status / muted meta / empty-state / preview 等 token。
	- 对高价值信息行使用更高对比与更稳定的 badge/metric 样式，对低价值辅助说明使用 quiet meta 风格。
- **Acceptance criteria**:
	- 用户可以一眼区分“这是 section 标题、这是当前状态、这是具体条目、这是辅助说明”。
	- `Sidebar` 不再因为大量同样颜色和同样密度的文本而显得混乱。
	- 同一种信息在不同 section 中尽量保持一致的视觉语义，而不是每个 panel 各说各话。

**Implemented 2026-04-08**

- `Sidebar` 渲染已改为 row-kind-aware：empty state、hint、section label、summary、metric、status、preview、codeish、todo state 与 log severity 都有独立样式。
- `Todos`、`Logs`、`Delivery`、`Skills`、`Document/Memory` 不再共享单一文本样式，状态与高价值信息拥有更高对比。

### Task 15B-60: Sidebar density, truncation, and drill-down defaults

- **Priority**: Medium
- **Complexity**: M
- **Dependencies**: Task 15B-52, Task 15B-59
- **Description**: 为 `Sidebar` 设计更稳的默认密度与展开策略，包括条目截断/换行规则、默认展开 section 优先级、长文本 preview 长度，以及何时把细节推到 overlay/detail drill-down，而不是在主面板里一股脑全部展示。
- **Implementation approach**:
	- 为每类 sidebar section 制定默认行数预算、title/preview clamp、summary-first 展示与 overflow 处理策略。
	- 把长路径、长日志和低优先级辅助说明默认压缩为 preview，并用 overlay/detail 承载深读。
	- 调整 section 的默认展开/收起规则，使 `Sidebar` 首屏优先回答“当前最重要的几件事是什么”。
- **Acceptance criteria**:
	- `Sidebar` 首屏信息密度更高，但扫描成本更低，不再出现“文字很多却抓不住重点”的情况。
	- 长文本不会轻易把 section 撑得过高，也不会因为粗暴截断而丢失全部语义。
	- 宽屏与窄屏下都存在稳定的默认阅读路径，而不是完全依赖用户自己滚动找重点。

**Implemented 2026-04-08**

- `Sidebar` section body 不再平均切分，而是根据 section 类型、内容量与当前 focus 使用带权重的高度预算。
- 未聚焦 panel 默认使用 preview line budget；超出预算时插入 overflow hint，并根据面板能力提示 `Enter` detail 或 focus/scroll 深读路径。
- 这让 sidebar 首屏默认先回答“现在哪几块最重要”，而不是把所有 section 压成同密度文本墙。

## Acceptance Signals

- 宽屏下 `Sidebar` 与 `Response` 一眼就能看出主次层级，而不是两个相同权重的大框。
- `Sidebar` section 在视觉上更接近“信息卡片”，而不是单纯列表容器。
- 输入区、context bar、status bar 形成统一的底部控制带。
- 用户即使不读文档，也能感受到当前界面的 focus、active 和 summary hierarchy 更清晰。
- 用户默认看到的是人类可读的结果报告，而不是原始执行噪音。
- 长时间阅读时界面仍保持稳定、克制、耐看，而不是高彩度开发者玩具风。
- `Response` 与 `Sidebar` 都能形成明确的首屏阅读路径，用户不用先适应布局就能抓到重点。

## Change Log

- 2026-04-08: 明确视觉方向为 `Dark Industrial Report Console`，补充 Modern TUI / Rich CLI、AI tooling 与 Dark Minimalist 的审美约束。
- 2026-04-08: 补充 `Task 15B-59 ~ 15B-60`，把 `Sidebar` 内部文本可读性、行型语义和默认密度策略纳入显式规划。
- 2026-04-08: 完成 `Task 15B-59 ~ 15B-60`，引入 sidebar row taxonomy、带权重的 section budget、preview clamp 与 overflow drill-down hint。
- 2026-04-08: 基于 `omega-tui-ui-reference` 补充 `Task 15B-61 ~ 15B-64`，把 monochrome palette 收敛、bento spacing、meta 降噪与 overlay 景深显式拆成下一轮视觉任务。
- 2026-04-08: 完成 `Task 15B-61 ~ 15B-64`，把默认主题收敛为 monochrome foundation + single accent，并在 `Response`、`Sidebar`、`Status` 与 overlay 上落实更弱边框、更 dim 的失焦层次和更清楚的浮层景深。
