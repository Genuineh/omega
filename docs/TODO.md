# TODO

## Current Priorities

_当前主线已按真实实现状态收口。判断依据：`cargo test` 全工作区通过；`Task 15F-26 ~ 15F-29` 与 `Task 15B-28 ~ 15B-29` 已完成，execute 已收敛到 itemized loop 并具备 subflow visibility；`Task 11A ~ 11F-3` 已完成，上下文装配、长期知识索引与 `ContextDiagnostics` 聚合快照均已成为现行基线。主优先级已回到 `Task 10`。_

_任务编号以 `docs/specs/omega-agent-impl-plan.md` 为准；`8A/8B`、`15A/15B/15C/15D` 等后缀仅用于里程碑拆分。详细历史与完成记录保留在后文。_

### High

- **Task 10**: `omega-subagent` 是当前主线，但要建立在已完成的上下文管理基线之上，避免调度再次被上下文膨胀拖垮。
- **Task 11A ~ 11F-3**: 已完成并转入基线能力，不再作为独立主线推进；原 Task 11 的上下文压缩需求已被这条子任务链替代。

### Medium

- **2026-04-02 document backend default enablement**: `omega-app` 默认 feature 现已包含 `document-backend`，因此默认 `cargo run -p omega-app` 会直接接通 `omega-document` / LanceDB / Tantivy 后端；`.omega/store/` 仍按需惰性生成，但不再需要额外 `--features document-backend` 才能使用 `search_codebase` 与 `manage_document`。
- **2026-04-02 context supervision follow-up**: 下一轮 TUI/context 工作应为 document system 与 memory system 规划专门监管面板，统一回答“是否启用、总量/大小、当前命中摘要”三类问题；详细方案见 `docs/specs/omega-tui-document-memory-supervision.md`。
- **2026-04-02 runtime maintenance**: 默认只读分析已拆为两条 child workflow：`research = explore -> report` 与 `deep-research = explore -> plan -> execute -> report`；root workflow 也已收敛为单步 `select-workflow`，并在一次结构化输出中同时写入 `recognized_scene_id` 与 `selected_workflow_id`。对系统性、全局性、深入式分析优先提升到 `deep-research`，实现类请求仍优先提升到 `feature`。
- **2026-04-02 interrupt + provider maintenance**: `omega-tui` 手动中断时会立即把仍在 `streaming` 的 response/thinking section 与运行中的 tool run 收口为 failed；`omega-client` provider transport 现默认启用 pacing（`100ms` 全局节流、并发 `1`、`10s` 的 `429` retry floor）并支持 `Retry-After`，`.omega/model.toml` 的 `[provider]` 也已暴露这些覆盖项。
- **2026-04-02 documentation hygiene**: `docs/README.md` 与本文件的首屏摘要已改为分组入口和精简状态说明；同时将已完成的 `observability-logging` PRD 与 `omega-tui-non-ui-extraction` 设计基线迁入 `docs/archive/`，避免历史材料继续混入 active spec 路径。
- **Tool + Prompt optimization follow-up**: 当前任务链为 `Task 8J.0 ~ 8U`。推荐顺序：`8J.0` 先行；manifest track 为 `8J -> 8K -> 8L -> 8M/8N/8O -> 8R -> 8S -> 8T -> 8U`；`8P` 继续等待存储 API 稳定。关键决策保持不变：Manifest-wraps-Handler、`ToolHandler` 签名不变、remediation 结构化、UI effects 复用 `RuntimeUiEffect`、profiles 大多 optional。另：`.omega/workflows/root.toml` 必须与内建默认同步为 `max_iterations = 4`，否则单步 `select-workflow` 会在一次非 JSON 偏航后直接耗尽预算。
- **Task 15F-30 ~ 15F-35**: deterministic test foundation 仍应作为 `Task 10 / 12 / 13` 的安全网推进，优先把 LLM、runtime event、process、fs 等外部边界收敛成稳定 mock seam，同时保持 workflow/session/core 尽量 real-tested。
- **Task 4 / Task 12 / Task 3 / Task 13**: 这些能力仍保持中优先级，但都应建立在 `Task 10`、runtime message boundary 与当前 app-owned policy 路径之上，而不是前置插队。
- **Task 15B-40 ~ 15B-46**: 已完成，`omega-tui` response panel 已支持 lightweight Markdown spans、代码块容器、消息角色标识、Final Answer 强化、Tool Lane 折叠与 Thinking 摘要增强。`Task 15B-8` 已被替代并标记为 Replaced。详见 `docs/specs/omega-tui-message-display-polish.md`。

### Low

- **Task 6**: `omega-worktree` 对后期自治执行很重要，但当前还不是主瓶颈。
- **Task 15B-9 ~ 15B-12**: 高级 TUI 能力（代码语法高亮、输入历史、面板搜索、可调宽度）继续后移。
- **Task 16**: 最终整合测试保留为收尾任务，不应提前占用主线优先级。

---

## Detailed Tasks

_以下保留详细里程碑与历史记录；待办项的 `Priority` 字段已按上面的新顺序同步更新。_

### ── M1.7: TUI 基础美化 ──

> 验证方式：`cargo run -p omega-tui` → 焦点指示清晰、消息自动滚动、输入流畅、视觉分层明确
> 对标：最小可用级 TUI 体验
> 前置：M1.5 (可观察性) 已完成

_将 M11 中基础级体验优化前移，确保在开发后续功能时有可用的日常交互界面。_

### Task 15B-1: omega-tui — 焦点指示器
- **Status**: Completed
- **Completed**: 2026-03-18
- **Priority**: High
- **Description**: 活跃面板边框高亮（加粗 + 颜色变化），非活跃面板边框暗淡；状态栏显示当前焦点面板名
- **Complexity**: S
- **Summary**: 活跃面板边框 #4ec9b0 + BOLD 修饰符 + ◆ 标记标题；非活跃面板 #303030 暗淡边框；状态栏新增 Focus: Response/Logs 标签；ColorScheme 增加 focus_border/border_dim 两色；cargo build + clippy 零警告

### Task 15B-2: omega-tui — 自动滚动与滚动优化
- **Status**: Completed
- **Completed**: 2026-03-18
- **Priority**: High
- **Description**: 新消息到达时自动滚动到底部；用户手动上滚后锁定位置不跳动；鼠标滚动用实际布局 Rect 边界判断面板，移除硬编码 column<80
- **Complexity**: M
- **Summary**: App 新增 response_pinned/logs_pinned 布尔标志控制自动滚动；add_output/add_log 在未 pinned 时 select(last) 自动跟随新内容；scroll_panel_up 上滚时设置 pinned=true 锁定视图；scroll_panel_down 下滚到底时 pinned=false 重启自动滚动；新增 panel_at(col) 用实际 response_rect/logs_rect 判断鼠标所在面板，移除硬编码 column<80；同时修复了原有 Up/Down 事件处理双重加锁潜在死锁；cargo build + clippy 零警告

### Task 15B-3: omega-tui — 输入行增强
- **Status**: Completed
- **Completed**: 2026-03-18
- **Priority**: High
- **Description**: 支持光标左右移动（←→键）、Home/End 跳转、光标位置可视化（闪烁 cursor）、输入行超宽时水平滚动
- **Complexity**: M
- **Summary**: App 新增 cursor_pos: usize 字段（Unicode char index，非字节）；insert_char/delete_char_before/delete_char_at/move_cursor_left/right/home/end/take_input 方法；输入区改用 Span-based 渲染，光标处 reverse-video 块状显示（fg/bg 互换 + BOLD），空缓冲区显示占位块；水平滚动：avail_w = widget_width - 5，scroll_offset = max(0, cursor_pos - avail_w + 1) 保持光标可见，左滚时前缀变 ◂>；Left/Right/Home/End/Delete 键全部接入；is_running 状态从覆盖输入区改为显示在 block title；运行中 spinner 覆盖 bug 消除；cargo build + clippy 零警告

### Task 15B-4: omega-tui — 状态栏与快捷键提示
- **Status**: Completed
- **Completed**: 2026-03-18
- **Priority**: Medium
- **Description**: 底部增加快捷键提示栏（Tab=切换面板 | ↑↓=滚动 | Ctrl+C=退出）；状态栏显示 agent 状态（Idle/Running/Error）和连接信息
- **Complexity**: S
- **Summary**: 布局增加第 4 个区块 Constraint::Length(1) 作为 hint bar；状态栏 ● Connected 改为动态显示 ◫ Idle / ◌ Running… 反映 is_running 状态；hint bar 显示 Tab=切换面板 ↑↓=滚动 ←→=光标 Ctrl+A/E=行首/尾 Del=删字 Ctrl+C=退出，使用 #646464 暗色文本不干扰主内容；ColorScheme 增加 hint_dim 颜色；cargo build + clippy 零警告
### Task 15B-5: omega-tui — 布局优化与文本换行
- **Status**: Completed
- **Completed**: 2026-03-18
- **Priority**: Medium
- **Description**: 面板内长文本自动 wrap；响应式面板比例（终端窄时 Logs 面板可隐藏或缩小）；border 间距和 padding 视觉优化
- **Complexity**: M
- **Summary**: 新增 wrap_text(line, width) 辅助函数对每条存储行按面板实际内宽硬换行，渲染时展开为多个 ListItem；App 新增 response_displayed_count/logs_displayed_count 记录渲染后行数，scroll_panel_down 用这两个字段作 max 边界（替代原来的 source 行数），scroll_panel_up 同步更新 fallback；自动滚动逻辑从 add_output/add_log 移至 render，每帧 !pinned 时自动 select(total-1)；响应式横向分割：< 60 列时 Logs 隐藏（0 %）、60-99 列 70/30、≥ 100 列 60/40，Logs 面板宽度为 0 时跳过渲染；状态栏由 Constraint::Length(2)+Borders::ALL 简化为 Constraint::Length(1) 纯色背景行（无 Block 边框），视觉更整洁；cargo build + clippy 零警告

### Task 15B-6: omega-tui — 消息样式分层
- **Status**: Completed
- **Completed**: 2026-07-14
- **Priority**: Medium
- **Description**: 用户消息前缀 `>` 绿色高亮；Agent 回复白色；工具调用命令黄色 `$ cmd`；错误消息红色；分隔不同轮次的对话
- **Complexity**: M
- **Summary**: 新增 MsgKind 枚举 (User/Agent/Tool/Error/Separator) + Msg 结构体；output_lines: Vec<String> 替换为 output_msgs: Vec<Msg>；push_msg(kind, text) 方法多行拆分存储；响应面板渲染通过 MsgKind 映射颜色（User→绿/Agent→白/Tool→黄/Error→红/Separator→暗）后调 wrap_text 展开为 ListItem；用户提交时插入 Separator + User Msg；LogUpdate 匹配分支更新为 push_msg(Tool/Agent)；移除旧 add_output 方法；cargo build + clippy 零警告

### Task 15B-7: omega-tui — Loading 动画
- **Status**: Completed
- **Completed**: 2026-03-18
- **Priority**: Low
- **Description**: Agent 运行时 spinner 动画（⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ 旋转），不覆盖输入区而是在状态栏或 Response 面板底部显示
- **Complexity**: S
- **Summary**: App 新增 spinner_tick: u8 字段；主循环每帧递增（wrapping_add(1)）；render() 中用 SPINNER_FRAMES[tick/2 % 10] 选取当前帧字符，is_running 时状态栏显示 "⠋ Running…" 动画，否则保持 "● Idle"；每帧约 50 ms，tick/2 使帧率降为 ~10 fps 视觉更平滑；状态栏/输入区不受影响；cargo build 零警告

### Task 15B-14: omega-tui — Todo/Logs 侧栏分栏
- **Status**: Completed
- **Completed**: 2026-03-19
- **Priority**: High
- **Description**: 将右侧辅助区从单一 Logs 面板改为上方 Todos、下方 Logs；todo 工具成功执行后在 TUI 中持续展示当前任务列表
- **Complexity**: M
- **Related**: docs/specs/omega-tui-todo-sidebar-layout.md
- **Summary**: 该任务已作为剩余高级 TUI 能力之前的最高优先级可见性工作提前落地；`omega-session::SessionUpdate` 新增 `TodoSnapshot`，在 `todo` 工具成功执行后把渲染结果推送到 TUI；`omega-tui` 新增 `Todo` 面板与本地 `todo_lines` 状态，右侧栏改为 `Todos 38% / Logs 62%` 纵向切分，焦点顺序变为 `Response -> Todos -> Logs`，鼠标滚轮与键盘滚动均按新面板命中区域工作；窄终端下仍按原规则隐藏整个右侧栏；相关单测与 `cargo test -p omega-session -p omega-tui` 通过

### Task 15B-15: omega-tui — Todo 面板联调与交互完善
- **Status**: Completed
- **Completed**: 2026-03-19
- **Priority**: High
- **Description**: 围绕新 Todo 面板补齐真实日常使用所需的交互与联调，包括空状态文案、运行中刷新体验、窄终端退化策略验证，以及与后续搜索/统计类能力的布局兼容性检查
- **Complexity**: M
- **Related**: docs/specs/omega-tui-todo-sidebar-layout.md
- **Summary**: Todo 面板从“原始字符串列表”补齐为带状态的 UI：未同步时显示可操作引导文案，空列表时显示明确 empty state，运行中新 turn 尚未刷新 todo 时在面板标题和状态栏摘要中标记 stale；同时把 `(x/y completed)` 汇总提炼为紧凑摘要，作为未来搜索/统计能力的兼容基础；窄终端隐藏右侧栏时焦点会自动回落到 `Response`，`Tab` 不再落到不可见面板；新增相关单测并以 `cargo test -p omega-tui` 验证通过

### Task 15B-16A: omega-tui — 浮动弹窗 / Overlay 基础设施
- **Status**: Completed
- **Completed**: 2026-03-19
- **Priority**: Medium
- **Description**: 引入统一浮动弹窗层，支持搜索框、确认框、详情查看和轻量 picker 等短时交互；建立 overlay 焦点捕获、键盘/鼠标路由、遮罩与尺寸退化规则
- **Complexity**: M
- **Related**: docs/specs/omega-tui-overlay-popups.md, docs/specs/omega-tui-modal-keymap.md, docs/specs/omega-tui-runtime-experience.md
- **Blocked by**: Task 15B-13
- **Blocks**: Task 15B-11, Task 15B-16
- **Summary**: `omega-tui` 新增单活动 overlay 基础设施，包含 `Search` / `Confirm` / `Detail` / `Picker` / `InputPrompt` 五类本地弹窗状态、遮罩与居中尺寸退化规则、overlay-first 键盘/鼠标路由以及焦点恢复；默认 keymap 新增 `leader /` 打开搜索弹窗，搜索窗会捕获独立输入并给出当前面板匹配数摘要，中断动作改为先进入确认弹窗再执行；补充相关单测并以 `cargo test -p omega-keymap -p omega-tui` 与 `cargo clippy -p omega-keymap -p omega-tui --all-targets -- -D warnings` 验证

### Task 15B-16: omega-tui — Runtime Activity 面板与状态徽章基础
- **Status**: Completed
- **Completed**: 2026-03-19
- **Priority**: Medium
- **Description**: 将当前 `Todos` + `Logs` 组合升级为统一可收起的右侧 `Sidebar`，支持整体收起快捷键、顶部图标轨、section 折叠/切换，以及面向 `skills/subagent/tasks/background/message/team/worktree` 的 `Activity` 面板基础
- **Complexity**: M
- **Related**: docs/specs/omega-tui-runtime-experience.md, docs/specs/omega-tui-collapsible-sidebar.md, docs/specs/omega-tui-modal-keymap.md, docs/specs/omega-tui-overlay-popups.md
- **Summary**: `omega-tui` 新增统一 `Sidebar` shell 与本地 `SidebarState`，把右侧辅助区重构为 `Response | Sidebar` 布局，并在侧边栏顶部加入轻量可聚焦 rail；当前 rail 直接以 `Todos | Logs` 呈现，后续 workflow 接入后又进一步收敛出清晰语义分工：`Response` 展示用户输入、各 step 的文本结果与最终 assistant 回复，右侧 `Activity & Logs` 承接 workflow phase、tool preview、todo 刷新与 tracing 日志，避免把纯运行态事件伪装成对用户的回答；支持 `leader b` 整体收起/展开、rail 左右切换、`x` 折叠 section、`Enter` 激活展开内容，同时显式禁止把最后一个 section 收起成空 sidebar；窄终端会自动隐藏侧边栏并回退焦点，状态栏同步增加 sidebar/logs 徽章信息；相关布局、事件和状态单测已覆盖，并以 `cargo test -p omega-tui`、`cargo test -p omega-keymap -p omega-tui`、`cargo clippy -p omega-keymap -p omega-tui --all-targets -- -D warnings` 验证

### Task 15B-17: omega-tui — 输入上下文带与底部状态带重构
- **Status**: Completed
- **Completed**: 2026-03-19
- **Priority**: Low
- **Description**: 将原输入框下方的提示/短消息区域上移到输入框上方、主体区下方的固定上下文带；将原顶部 header 中真正需要持续展示的模型名与运行态下移到输入框下方固定状态带，并抽象出便于后续扩展的底部状态槽
- **Complexity**: M
- **Related**: docs/specs/omega-tui-input-status-layout.md, docs/specs/omega-tui-runtime-experience.md
- **Blocked by**: Task 15B-16
- **Blocks**: Task 15B-12
- **Summary**: `omega-tui` 主布局重排为 `Main content -> Input context bar -> Input box -> Bottom status bar`，原输入框下方的 leader/notice/overlay 提示被统一上移到固定上下文带；原顶部 header 被移除，底部状态带现通过 slot 化 segment 渲染保留模型名与 `Idle / Running` 摘要，为后续会话统计和更多 runtime badge 扩展提供稳定入口；随后又统一引入 rounded 边框、取消输入区与状态条的厚重背景分层，改用线框 / 非线框关系区分结构，并让输入框边框跟随 `NORMAL` / `INSERT` 模式使用语义色联动；补充布局与视觉语义单测，并以 `cargo test -p omega-tui`、`cargo clippy -p omega-tui --all-targets -- -D warnings` 验证通过

### ── M2: 文件工具 (s02) ── ✅

> 验证方式：`cargo run` → 让 Agent 读/写/编辑文件
> 对标：learn-claude-code s02_tool_use.py

### Task 8B: omega-tools-builtin — ReadHandler + WriteHandler + EditHandler
- **Status**: Completed
- **Completed**: 2026-03-18
- **Priority**: High
- **Description**: 新增文件读取、写入、编辑三个 handler，含路径安全校验（不允许逃逸工作目录）
- **Summary**: ReadHandler (read_file) 支持 path + 可选 limit，截断 50,000 字符，路径安全检验；WriteHandler (write_file) 支持 path + content，自动创建父目录；EditHandler (edit_file) 支持 path + old_text + new_text，替换第一次出现；三个 handler 共享 safe_path_within_root 安全辅助函数；create_default_tools 注册全部四个 handler（bash/read/write/edit）；新增 23 项测试，总计 44 项测试全通过；clippy 零警告
- **Related**: learn/learn-claude-code/agents/s02_tool_use.py

### ── M2A: Client Follow-up ──

> 验证方式：`cargo test -p omega-client` 覆盖 Messages / Streaming / Prompt Caching / Count Tokens / Models / Message Batches，并补 mock HTTP/SSE 与 live Minimax acceptance tests
> 对标：Anthropic 官方 API + `anthropic-sdk-go` 的 service 组织方式，但保持本仓库自有 typed client

### Task 2A: omega-client — Anthropic SDK 风格 API 抽象与独立测试
- **Status**: Completed
- **Started**: 2026-03-23
- **Completed**: 2026-03-23
- **Priority**: Medium
- **Description**: 在保留现有 `LlmClient` 兼容面的前提下，为 `omega-client` 抽出 provider-neutral 的 Anthropic API 层，并把 Minimax 下沉为 Anthropic-compatible provider 适配器。
- **Summary**: `omega-client` 已新增 provider-neutral 的 `anthropic` 服务层与 `AnthropicMessagesCompatClient`，保留现有 `LlmClient` / `ChatRequest` / `ChatResponse` 兼容面不变；`MinimaxClient` 现通过该兼容层运行，并补齐 `AnthropicProviderConfig` / capability matrix、Messages create/create_stream、SSE 解析与消息累积、prompt caching typed fields、`count_tokens` / `models` / `message_batches` service、`OMEGA_*` / `ANTHROPIC_*` env fallback，以及独立的 request/stream/provider/capability/live-ignored 测试矩阵。验证已通过 `cargo fmt --all`、`cargo test -p omega-client`、`cargo clippy -p omega-client --all-targets -- -D warnings` 与 `cargo test -p omega-core -p omega-session -p omega-subagent -p omega-app`。
	2026-03-24 补充修正：当 Anthropic-compatible streaming SSE 缺失 `message_start` 或起始事件序列非法时，`omega-client` 现在会附带 frame/event preview 诊断并在 `MinimaxClient::chat_stream()` 中自动回退到非流式 `chat()`，避免上层 workflow 因 provider 坏流直接失败。
	2026-04-01 补充修正：Anthropic-compatible transport 现在会对 provider `5xx` 响应做有限自动重试，避免 root routing / workflow step 因短暂上游错误直接失败。
- **Related**: docs/specs/omega-client-anthropic-api-abstraction.md

### ── M2B: Tool System Follow-up ──

> 验证方式：针对仓库分析与 workflow chat 主路径，常见探索动作默认走 `list_dir/glob_search/grep_search/read_file/apply_patch/batch` 等结构化工具，而不再依赖 `bash` 拼接 shell；tool result 与 runtime UI 保持结构化；相关 cargo test / clippy 通过
> 对标：当前 Omega runtime contract + 参考实现中的结构化 read/glob/grep/edit/write/batch/task 工具分层

### Task 8C: omega-tools / omega-core / omega-session — Tool Contract V2
- **Status**: Completed
- **Completed**: 2026-03-24
- **Priority**: High
- **Description**: 为 `ToolHandler` 建立统一的 typed result contract，首轮补齐 `output/preview/metadata/truncated/error_kind` 五个核心字段（`title` 与 `artifacts` 延后），同时保留旧字符串返回值的兼容包装。
- **Complexity**: M
- **Planning Note**: session 已有完整 `ToolRun`/`ToolRunDetail`/`ToolRunStatus`，不需要重新设计 UI contract；compat adapter 是标准模式。
- **Summary**: `omega-tools` 已新增 `ToolResult` / `ToolErrorKind` 与 `ToolHandler::execute_v2` compat adapter，旧 `execute -> Result<String>` handler 会自动包装进 typed result；`omega-tools-builtin` 的 `bash/read_file/write_file/edit_file` 已补齐 preview、metadata、truncated 与错误分类；`omega-core` agent loop 现向上游回调 typed tool result，并继续以 text-first tool result block 回注模型；`omega-session` 的 `ToolRun` lifecycle 现直接消费 tool-provided preview/metadata/error_kind，不再只靠 `Error:` / 文本截断猜测结果。验证已通过 `cargo test -p omega-tools -p omega-tools-builtin -p omega-core -p omega-session` 与 `cargo clippy -p omega-tools -p omega-tools-builtin -p omega-core -p omega-session --all-targets -- -D warnings`。
- **Related**: docs/specs/omega-tool-system-upgrade.md, docs/specs/omega-runtime-ui-message-contract.md

### Task 8D: omega-tools-builtin — Structured Workspace Inspection Tools
- **Status**: Completed
- **Completed**: 2026-03-24
- **Priority**: High
- **Description**: 新增 `list_dir`、`glob_search`、`grep_search`，并为 `read_file` 增加 `start_line`/`end_line` 范围读取语义，覆盖常见 repo inspection 场景。同时统一 `BashHandler.validate_path_within_root` 与文件 handler 区域的 `safe_path_within_root` 为模块级共享函数。
- **Complexity**: L
- **Planning Note**: 不依赖 Task 8C——用现有 `Result<String>` 即可工作，8C 落地后再回补结构化返回值。这是当前稳定性收益最高、应第一个落地的任务。
- **Summary**: `omega-tools-builtin` 已新增 `list_dir`、`glob_search`、`grep_search` 三个结构化只读工具，并为 `read_file` 增加 `start_line` / `end_line` 的 inclusive range 读取与更严格的参数校验；builtin 路径安全辅助函数已统一，`omega-core` 默认工具注册已接入新工具，`omega-session` / `omega-workflow` 也已同步收紧 root routing step 的 tool block 集，避免 scene/workflow selection 泄露 repo inspection 能力。验证已通过 `cargo test -p omega-tools-builtin -p omega-core -p omega-session -p omega-workflow` 与 `cargo clippy -p omega-tools-builtin -p omega-core -p omega-session -p omega-workflow --all-targets -- -D warnings`。
- **Related**: docs/specs/omega-tool-system-upgrade.md

### Task 8E: omega-tools-builtin — Patch-Centric Editing Toolset
- **Status**: Completed
- **Completed**: 2026-03-24
- **Priority**: Medium
- **Description**: 新增 `apply_patch`、`create_file`，并增强现有 `edit_file` / `write_file` 的 diff 与诊断反馈，让执行态写路径更稳定。
- **Complexity**: M
- **Planning Note**: 当前失败模式集中在读路径而非写路径，优先级从 High 降为 Medium；8C 落地后再做。
- **Summary**: `omega-tools-builtin` 已新增 `create_file`（只创建不覆盖）与 `apply_patch`（单文件、原子、多段 exact-text replacement patch）两个写工具；`write_file` / `edit_file` 现会返回结构化 diff metadata、字节/行数变化与更清晰的 validation diagnostics。`omega-core` 默认工具集、`omega-session` 默认 tool catalog 与 `omega-workflow` 的 root/chat/feature 默认 block 集也已同步更新：root routing 继续显式屏蔽所有写工具，非 execute step 也会默认屏蔽 `apply_patch/create_file/edit_file/write_file`。验证已通过 `cargo test -p omega-tools-builtin -p omega-core -p omega-session -p omega-workflow` 与 `cargo clippy -p omega-tools-builtin -p omega-core -p omega-session -p omega-workflow --all-targets -- -D warnings`。
- **Related**: docs/specs/omega-tool-system-upgrade.md

### Task 8F: omega-tools-builtin / omega-session — Batch Read-Only Tool
- **Status**: Completed
- **Completed**: 2026-03-24
- **Priority**: Medium
- **Description**: 提供受限的 `batch` 只读工具并行执行能力，减少目录浏览、glob、grep、read 等组合任务对 loop budget 的消耗。
- **Complexity**: M
- **Blocked by**: Task 8D
- **Summary**: `omega-tools-builtin` 已新增受限的 `batch` 只读工具，允许在单次调用中并行聚合 `list_dir`、`glob_search`、`grep_search` 与 `read_file` 请求，并以稳定输入顺序返回结构化 preview / metadata / per-item output；非法子请求会在 batch 内按项报告 validation/policy 失败而不放大为整批崩溃。`omega-core` 默认工具集已注册 `batch`，`omega-session` 复用现有 `ToolRun` detail 渲染显示 batch summary 与嵌套 metadata，`omega-workflow` 也已继续在 root routing 默认 block 集中显式屏蔽 `batch`，避免 scene/workflow selection 泄露 repo inspection 能力。验证已通过 `cargo test -p omega-tools-builtin -p omega-core -p omega-session -p omega-workflow` 与 `cargo clippy -p omega-tools-builtin -p omega-core -p omega-session -p omega-workflow --all-targets -- -D warnings`。
- **Related**: docs/specs/omega-tool-system-upgrade.md

### Task 8G: omega-tools-builtin — Bash V2
- **Status**: Completed
- **Completed**: 2026-03-24
- **Priority**: Medium
- **Description**: 为 `bash` 增加 `workdir`、`description`、更清晰的 policy error 分类。AST-based 校验评估移出本轮 scope——8D 补齐后 bash 压力大幅下降，当前字符串归一化策略够用。
- **Complexity**: M
- **Blocked by**: Task 8D
- **Summary**: `omega-tools-builtin` 中的 `bash` 已升级为 Bash V2：新增可选 `workdir`（必须解析到 workspace 内现存目录）与 `description` 输入字段，执行与路径校验会基于解析后的工作目录而不是固定 workspace root；同时把原先依赖错误文本匹配的 policy/validation 分类改为显式 typed parsing，并在 structured metadata 中稳定返回 `error_code`、`workdir`、`description` 与 `timeout_seconds`。`omega-session` 也已同步让 bash 的 invocation preview 显示 `description` 与非 root `workdir`，使 step tool summary 更可读。验证已通过 `cargo test -p omega-tools-builtin -p omega-session` 与 `cargo clippy -p omega-tools-builtin -p omega-session --all-targets -- -D warnings`。
- **Related**: docs/specs/omega-tool-system-upgrade.md

### Task 8H: omega-tools / omega-workflow / omega-app — Tool Policy Surface
- **Status**: Completed
- **Completed**: 2026-03-24
- **Priority**: Medium
- **Description**: 统一 `.omega` 下的 tool policy 配置、workflow 默认工具组和 prompt 对齐策略，让配置、runtime 与提示词不再互相打架。
- **Complexity**: M
- **Summary**: `omega-workflow` 已新增 `ToolPolicyConfig`，从 `.omega/model.toml` 统一加载 bash allowlist、batch 最大请求数和命名工具组，并把这些 policy 同时用于 builtin workflow 默认 tool request、repo-local `.omega/workflows/*.toml` 解析和 app/session runtime wiring；`omega-tools-builtin` / `omega-core` 现支持可配置 batch request limit，`omega-app` 默认 system prompt 与 repo-local step prompt 也已统一改为 structured-tools-first、bash-fallback 语义。repo 中的 `.omega/model.toml`、`.omega/workflows/*.toml` 与关键 step prompt 已同步收口，避免配置、runtime 与提示词继续漂移。验证已通过 `cargo test -p omega-tools-builtin -p omega-core -p omega-workflow -p omega-session -p omega-app` 与 `cargo clippy -p omega-tools-builtin -p omega-core -p omega-workflow -p omega-session -p omega-app --all-targets -- -D warnings`。
- **Related**: docs/specs/omega-tool-system-upgrade.md, docs/specs/omega-workflow-package.md

### Task 8I: omega-tools-builtin — Builtin Tool Module Split
- **Status**: Completed
- **Completed**: 2026-03-24
- **Priority**: Medium
- **Description**: 将 `omega-tools-builtin` 中单体 `lib.rs` 的 builtin tool 实现拆分为按工具分文件的模块结构，并把测试改为按工具独立组织。
- **Complexity**: M
- **Summary**: `omega-tools-builtin` 已把原先集中在 `src/lib.rs` 中的 builtin tool 实现拆分为 `bash`、`read_file`、`list_dir`、`glob_search`、`grep_search`、`batch`、`create_file`、`write_file`、`edit_file`、`apply_patch` 等独立模块，并额外抽出 `shared` 与 `path_safety` 复用层；crate 根现仅保留模块声明和公共 re-export，外部 API 保持不变。同时新增 `tests/` 下按工具拆分的集成测试，覆盖各 handler 的核心行为与结构化 metadata。验证已通过 `cargo test -p omega-tools-builtin` 与 `cargo clippy -p omega-tools-builtin --tests`。
- **Related**: crates/omega-tools-builtin/src/lib.rs, crates/omega-tools-builtin/tests/

### ── M2B.1: Tool Capability System Upgrade ──

> 验证方式：工具定义不再只包含 handler + schema，而是统一具备 prompt、UI、context、permission、storage、monitoring 能力；`omega-session` / `omega-app` / `omega-tui` 对工具结果的消费不再依赖自由文本猜测；高价值工具族（FileEdit、WebResearch、Todo/Interaction）都能沿同一 contract 扩展
> 对标：`docs/specs/omega-tool-prompt-optimization.md` 中的 tool manifest、tool strategy、tool outcome、tool bridges 与 observability 设计

### Task 8J.0: omega-session — Lightweight Tool Strategy Prompt (Quick Win)
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: High
- **Description**: 基于现有 `ToolDefinition` 和 `SessionToolCatalog`，把 prompt 中的 `"Visible tools: x, y, z"` 升级为包含 family-level guidance 和 when_to_use/when_not_to_use 的简洁 prompt block。不依赖 manifest，直接硬编码现有 11 个工具的策略。可与 Task 8J 并行推进，立即改善工具选择质量。
- **Complexity**: S
- **Summary**: live prompt path 现已在 `omega-context::render_visible_tools()` 输出 `Visible tools` + `Tool strategy` 组合块，为当前默认可见工具集补齐 family-level guidance 和 `when_to_use` / `when_not_to_use` 提示。首版覆盖 `Workspace inspection`、`Knowledge and governance`、`Editing`、`Planning`、`Escape hatch` 五类，并对 `none` 情况显式提示本 step 不可调用工具、应直接作答。由于 `omega-session` 当前通过 `omega-context` 组装 step system blocks，这个 quick win 实际落在 live assembler path，而不依赖 manifest 迁移。
- **Validation**: `cargo test -p omega-context --lib render_visible_tools`
- **Related**: docs/specs/omega-tool-prompt-optimization.md (Workstream B, Task B0)

### Task 8J: omega-tools / omega-session / omega-app — Tool Manifest Layer
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: High
- **Description**: 为工具定义增加统一 `ToolManifest` 层，采用 Manifest-wraps-Handler 方案：`ToolDispatcher` 持有 manifest map，manifest 内含 `handler: Box<dyn ToolHandler>`，现有 `ToolHandler` trait 签名不变。只有 Tool Core + Prompt Strategy 两类 profile 必填，其余七类为 `Option<T>`。
- **Complexity**: L
- **Blocks**: Task 8K, Task 8L, Task 8M, Task 8N, Task 8O, Task 8R, Task 8S, Task 8T, Task 8U
- **Summary**: `omega-tools` 已引入正式 `ToolManifest` / `ToolManifestMetadata` 层与 `ToolFamily`、`ToolStability`、`ToolPromptProfile` 等 capability profile；`ToolDispatcher` 现改为持有 manifest map，但保留 `register(Box<dyn ToolHandler>)` 兼容包装，旧 handler trait 签名未变。`omega-core` 默认 built-in tools 已迁移为 manifest 注册，并为当前默认工具集补齐第一版硬编码 prompt profiles；`omega-session` 的 `SessionToolCatalog` / `ResolvedToolSet` 也已开始保留 manifest metadata，而不再只剩 `ToolDefinition`，为后续 `8K` 的 manifest-based prompt builder 提供直接输入。`omega-app` 本轮无需额外接线，因为 app 侧当前不直接消费 tool catalog internals；下游兼容通过 `omega-core` re-export 和包级测试覆盖确认。
- **Validation**: `cargo test -p omega-tools manifest`; `cargo test -p omega-core tests::default_tools_expose_manifest_metadata -- --exact`; `cargo test -p omega-session tests::session_tool_catalog_matches_current_default_tool_set -- --exact`; `cargo test -p omega-session tool_catalog::tests::manifest_catalog_preserves_prompt_metadata -- --exact`
- **Related**: docs/specs/omega-tool-prompt-optimization.md, docs/specs/omega-tool-system-upgrade.md

### Task 8K: omega-session / omega-app / omega-workflow — Tool Prompt Strategy Builder
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: High
- **Description**: 基于 manifest 的 `ToolPromptProfile`，在现有 prompt assembly 上新增四层结构（Global/Family/Step/Tool），替代 Task 8J.0 的硬编码版。
- **Complexity**: M
- **Blocked by**: Task 8J
- **Blocks**: Task 8S, Task 8T
- **Summary**: `omega-context` 的 live step system block 现已从 `StepContextRequest.tool_manifests` 直接消费 manifest metadata，并用 `ToolPromptProfile` 输出四层 `Tool strategy` 结构：Global 基线规则、Family 聚合 guidance、Step hints、Tool-specific guidance。`omega-session::runner` 已把 `ResolvedToolSet::tool_manifests()` 接入 context request，旧的硬编码 family 表不再驱动 live prompt；当前 step-specific guidance 会根据 `plan / execute / report / routing` 等 step id 调整 tool-selection 提示，工具级 guidance 则直接读取 manifest 的 `summary / when_to_use / when_not_to_use / fallback_to`。
- **Validation**: `cargo test -p omega-context render_visible_tools --color never`
- **Related**: docs/specs/omega-tool-prompt-optimization.md, docs/specs/omega-workflow-package.md

### Task 8L: omega-tools / omega-core / omega-session — Tool Outcome Remediation Contract
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: High
- **Description**: 在保留 `ToolResult` 基础上补齐结构化 `ToolRemediation` 类型（kind + suggestion + alternative_tools + recoverable），使 `validation/policy/timeout/execution` 失败能稳定输出“主流程下一步该怎么做”。初版可基于 `ToolErrorKind` match 硬编码，后续基于 manifest 的 `fallback_to` 泛化。
- **Complexity**: M
- **Blocked by**: Task 8J
- **Blocks**: Task 8M, Task 8O, Task 8R, Task 8S, Task 8T, Task 8U
- **Summary**: `omega-tools` 现已新增 `ToolRemediation` / `ToolRemediationKind`，并由 `ToolDispatcher` 在 error result 上统一补齐 remediation；manifest 已注册工具会优先使用 `ToolPromptProfile.fallback_to` 生成 `alternative_tools`，未知工具则退回已注册工具列表。`ToolResult` 现在支持结构化 `remediation` 字段与 `as_content_value()` 序列化，`omega-core` 的 agent/tool-result block 已向模型返回带 `output + error_kind + remediation` 的 JSON object 错误结果，`omega-session` 的 tool run detail 也已显式展示 remediation fields，避免 UI/loop 再依赖自由文本猜测下一步动作。
- **Validation**: `cargo test -p omega-tools dispatch_adds_manifest_based_remediation_to_error_results --color never`; `cargo test -p omega-core hidden_tool_calls_return_tool_result_error --color never`; `cargo test -p omega-session tool_run_detail_lines_include_structured_remediation --color never`
- **Related**: docs/specs/omega-tool-prompt-optimization.md, docs/specs/omega-runtime-message-pipeline.md

### Task 8M: omega-app / omega-session / omega-tui — Declarative Tool UI Effects
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: Medium
- **Description**: 为工具结果增加显式 UI effects，作为现有 `RuntimeUiEffect` enum 的新 variant（`RequestInput`、`OpenDiffPreview`、`OpenWebResultView`），不另建独立 effect 体系，让 TUI 只需消费一套来源。
- **Complexity**: M
- **Blocked by**: Task 8J, Task 8L
- **Blocks**: Task 8R, Task 8S, Task 8T, Task 8U
- **Summary**: `omega-session` / `omega-app` / `omega-tui` 现已把工具特定 UI 行为收口到现有 runtime UI contract：新增 `RuntimeUiEffect::OpenDiffPreview` 与 `RuntimeUiEffect::RequestToolApproval`，并通过对应 `StateMessage` 走通 runtime-message policy、legacy UI compatibility 与 TUI reducer。FileEdit family 成功返回 diff 时会声明式打开 diff detail overlay；带 approval requirement 的 policy failure 会打开确认型 approval overlay，而不是继续依赖自由文本或 ad-hoc log parsing。
- **Validation**: `cargo test -p omega-session 'runtime_message::tests' --color never`; `cargo test -p omega-app policy_routes_tool_specific_ui_state_messages_to_surface --color never`; `cargo test -p omega-tui diff_preview_effect_opens_detail_overlay --color never`; `cargo test -p omega-tui request_tool_approval_effect_opens_confirm_overlay --color never`
- **Related**: docs/specs/omega-tool-prompt-optimization.md, docs/specs/omega-runtime-message-pipeline.md

### Task 8N: omega-session / omega-context / omega-tools — Scoped ToolExecutionContext
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: High
- **Description**: 为工具执行提供显式 `ToolExecutionContext`。**不改变 `ToolHandler` trait 签名**，通过构造注入（`Arc<dyn ContextProvider>`）和 dispatcher 层提供上下文。`ToolContextProfile` 为声明式 request descriptor，实际交付由 `OmegaContextFacade` 统一履行。
- **Complexity**: M
- **Blocked by**: Task 8J
- **Blocks**: Task 8P, Task 8S, Task 8T
- **Summary**: `omega-tools` 现已正式引入 `ToolExecutionContext` 运行时快照，`omega-session::runner` 会按 `workspace_root / workflow / step / current execute item / turn_id` 组装该上下文并传给 `ToolRunTracker`，使 tool lifecycle、UI detail 与 follow-up effects 不再只知道 tool name 和自由文本输出。默认 manifest 也已补齐 `ToolContextProfile`，把 `workspace_root`、`step metadata`、`memory_scope` 与 `network_context` 需求变成显式声明；现有 `OmegaContextFacade` 构造注入路径继续保留，handler trait 签名未改。
- **Validation**: `cargo test -p omega-session tool_run_detail_lines_include_structured_remediation --color never`; `cargo test -p omega-core default_tools_expose_manifest_metadata --color never`
- **Related**: docs/specs/omega-tool-prompt-optimization.md, docs/specs/omega-context-management.md

### Task 8O: omega-workflow / omega-session / omega-tui — Tool Permission Profiles And Approval Surface
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: High
- **Description**: 为每个工具引入 permission profile，三层模型：`StepToolRequest`(visibility) > `ToolPolicyConfig`(enablement) > `RuntimeApproval`(approval)。`ask_user_question` 应成为权限确认与用户澄清的结构化工具入口。
- **Complexity**: L
- **Blocked by**: Task 8J, Task 8L
- **Blocks**: Task 8T
- **Summary**: 默认 built-in manifest 现已补齐 `ToolPermissionProfile`，把 `permission_class`、`default_policy_mode`、`requires_approval` 与 `denial_remediation` 变成结构化 contract；FileEdit / bash / manage_document 等写或高风险工具统一声明 approval requirement。`ToolDispatcher` 的 policy remediation 现会优先采用 manifest 的 denial guidance，`omega-session` 会在带 approval requirement 的 policy failure 上发出声明式 approval request，`omega-tui` 则把它渲染为确认型 overlay，形成 step visibility -> permission guidance -> approval surface 的一条稳定路径。
- **Validation**: `cargo test -p omega-core default_tools_expose_manifest_metadata --color never`; `cargo test -p omega-session capability_effects_emit_diff_preview_and_approval_surface --color never`; `cargo test -p omega-tui request_tool_approval_effect_opens_confirm_overlay --color never`
- **Related**: docs/specs/omega-tool-prompt-optimization.md, docs/specs/omega-workflow-package.md

### Task 8P: omega-session / omega-memory / omega-todo / omega-tools — Tool Storage Effects
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: Low
- **Description**: 为工具副作用建立统一 effect 模型。**延迟落地**：应等 `omega-memory` 和 `omega-document` 的 `OmegaContextFacade` API 稳定后再实现，当前只需在 manifest 中预留 `storage: Option<ToolStorageProfile>` 占位。工具通过 `OmegaContextFacade` 执行存储副作用，不直接访问 omega-memory 或 omega-document。
- **Complexity**: M
- **Blocked by**: Task 8J, Task 8N
- **Blocks**: Task 8R, Task 8S, Task 8T, Task 8U
- **Summary**: 当前范围内的 `storage effect` contract 已完成为声明式层：默认 manifest 现已补齐 `ToolStorageProfile`，把 `session_journal / artifact / memory / todo / replayable` 副作用显式挂到能力面；`omega-session` 在 tool completion 后会把这些 storage effects 投影到 ToolRun detail 与 `tool.storage ...` runtime activity，避免 UI 和 diagnostics 再从工具输出文本猜测副作用。真实的 facade-owned 存储写入仍沿现有 handler/manager 路径执行，但后续扩展已不需要重新定义 storage contract。
- **Validation**: `cargo test -p omega-core default_tools_expose_manifest_metadata --color never`; `cargo test -p omega-session capability_effects_emit_diff_preview_and_approval_surface --color never`
- **Related**: docs/specs/omega-tool-prompt-optimization.md, docs/specs/omega-context-management.md

### Task 8R: omega-tools-builtin / omega-session / omega-app — FileEdit Family Consolidation
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: High
- **Description**: 收口 `apply_patch`、`create_file`、`edit_file`、`write_file` 的 capability profile、diff preview、write permission、storage effects 与 prompt guidance，使其对模型与 UI 呈现为统一 FileEdit 家族，而不是一组彼此割裂的写工具。
- **Complexity**: M
- **Blocked by**: Task 8J, Task 8L, Task 8M, Task 8P
- **Blocks**: Task 8U
- **Summary**: `apply_patch`、`create_file`、`edit_file` 与 `write_file` 现已统一挂接 FileEdit family capability profiles：共享 `ToolUiProfile`（含 `open_diff_preview` affordance）、`ToolContextProfile`、`ToolPermissionProfile`、`ToolStorageProfile` 与 `ToolObservabilityProfile`，默认 policy 也统一为 `workspace_write` + approval-aware write surface。`omega-session` 的 ToolRun detail 现会显式展示这些 family-level capability，成功的 file edit tool result 会自动触发 diff preview overlay，policy failure 则会走统一 approval/remediation surface，使这四个写工具对模型与 UI 不再是彼此割裂的孤立入口。
- **Validation**: `cargo test -p omega-core default_tools_expose_manifest_metadata --color never`; `cargo test -p omega-session capability_effects_emit_diff_preview_and_approval_surface --color never`; `cargo test -p omega-tui diff_preview_effect_opens_detail_overlay --color never`
- **Related**: docs/specs/omega-tool-prompt-optimization.md, docs/specs/omega-tool-system-upgrade.md

### Task 8S: omega-tools-builtin / omega-session / omega-app — Web Research Tools
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: Medium
- **Description**: 新增 `web_search` 与 `web_fetch`，补齐 network policy、tool guidance、structured metadata、UI 展示、cache/storage effect 与 observability，使外部资料检索不再只能退回 `bash` 或靠用户手工贴链接。
- **Complexity**: L
- **Blocked by**: Task 8J, Task 8K, Task 8L, Task 8M, Task 8N, Task 8P
- **Blocks**: Task 8U
- **Summary**: `omega-tools-builtin` 已新增真实的 `web_search` 与 `web_fetch` handler：前者抓取公开 HTML 搜索结果并返回结构化候选结果，后者用 blocking HTTP client 拉取已知 URL 并生成结构化摘要；两者都通过 manifest 暴露 `WebResearch` family、network-aware capability profiles 与 `open_web_result_view` affordance。`omega-session` 对成功的 web tool result 现会投影为显式 `OpenWebResultView` runtime state，`omega-app` / `omega-tui` 会统一在 search-results overlay 中展示结果，不再把外部资料检索压回 `bash`。本轮 storage/cache 仍停留在 declarative capability contract 层，尚未接入独立 fetch cache 后端。
- **Validation**: `cargo test -p omega-tools-builtin --color never`; `cargo test -p omega-session capability_effects_emit_input_prompt_and_web_overlay --color never`; `cargo test -p omega-app policy_routes_tool_specific_ui_state_messages_to_surface --color never`
- **Related**: docs/specs/omega-tool-prompt-optimization.md

### Task 8T: omega-tools-builtin / omega-session / omega-tui — Todo And Interaction Tools Formalization
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: Medium
- **Description**: 正式化 `todo_write`、`todo_read`、`ask_user_question` 与 `task` 工具的 capability contract，明确它们在 prompt、UI、approval、storage 和 replay 中的统一行为，避免 todo 更新、用户确认和子任务委托继续依赖上游特例分支。
- **Complexity**: L
- **Blocked by**: Task 8J, Task 8K, Task 8L, Task 8M, Task 8N, Task 8O, Task 8P
- **Blocks**: Task 8U
- **Summary**: `omega-todo` 现已正式拆出 `todo_write` 与 `todo_read`，同时保留兼容别名 `todo`；`omega-tools-builtin` 新增 `ask_user_question` 与 `task`，统一落在 `Interaction` family manifest/profile 下。`omega-session` 对 `todo_write` 成功结果会沿用已有 todo snapshot 刷新路径，对 `ask_user_question` 成功结果则投影为显式 `RequestInput` runtime state，`omega-tui` 会打开 `InputPrompt` overlay 而不是把这类交互埋在普通 assistant 文本里。`task` 当前按本阶段边界只做结构化 delegation request formalization，返回 `recorded_only` 元数据，尚未把 fresh-context child execution 真正接进 runtime loop。
- **Validation**: `cargo test -p omega-todo --color never`; `cargo test -p omega-session tool_run_tracker_accumulates_capability_metrics --color never`; `cargo test -p omega-tui request_input_effect_opens_input_overlay --color never`
- **Related**: docs/specs/omega-tool-prompt-optimization.md, docs/specs/omega-workflow-package.md

### Task 8U: omega-observability / omega-session / omega-tui — Tool Capability Metrics And Stability Matrix
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: Medium
- **Description**: 为工具系统补齐 per-tool / per-family metrics 与回归矩阵，重点跟踪 `bash_fallback_count`、`tool_failure_count_by_kind`、`tool_switch_after_failure`、`same_intent_retry_count`、`question_block_count` 等稳定性指标，验证新工具体系是否真的帮助主流程更稳定推进。
- **Complexity**: M
- **Blocked by**: Task 8L, Task 8M, Task 8P, Task 8R, Task 8S, Task 8T
- **Summary**: `omega-session::ToolRunTracker` 现会累积 `tool_invocations`、`family_invocations`、`tool_failure_count_by_kind`、`bash_fallback_count`、`tool_switch_after_failure`、`same_intent_retry_count` 与 `question_block_count`，并把 snapshot 挂进 `StepDiagnostics.tool_capabilities`。`omega-tui` 的 diagnostics panel 与 detail overlay 已能直接展示这些计数，使新工具体系的稳定性不再只存在于 tracing 文本中，而能作为 step 级 runtime diagnostics 被消费和回归验证。
- **Validation**: `cargo test -p omega-session tool_run_tracker_accumulates_capability_metrics --color never`; `cargo test -p omega-tui open_web_result_view_effect_opens_search_overlay --color never`; `cargo test -p omega-app policy_routes_tool_specific_ui_state_messages_to_surface --color never`
- **Related**: docs/specs/omega-tool-prompt-optimization.md, docs/specs/omega-runtime-message-pipeline.md

### ── M2C: Large File Maintainability Follow-up ──

> 验证方式：按 crate 定向运行 `cargo test` / `cargo clippy`，确保拆分后行为不变；热点文件行数明显下降，生产代码与测试、运行时与 helper、配置模型与默认落盘逻辑边界更清晰
> 对标：先做低风险的测试抽离，再做 `session / workflow / tui / client / core / docs` 的职责收口，避免 God file 继续膨胀

_2026-03-25 补充：已按仓库当前真实体量做过一轮扫描；当前最大热点集中在 `crates/omega-session/src/lib.rs`、`crates/omega-tui/src/{app,event,render}.rs`、`crates/omega-workflow/src/lib.rs`、`crates/omega-client/src/{anthropic,lib}.rs`，以及 `docs/specs/omega-agent-impl-plan.md` / `docs/specs/omega-step-session-asset-model.md`。本组任务用于把这些热点在继续扩张前先拆开。_

### Task 15F-15: omega-session / omega-workflow / omega-tui / omega-client / omega-core — Inline Test Extraction
- **Status**: Completed
- **Priority**: High
- **Description**: 将大体量根文件中的内联测试模块优先迁出到 `tests/` 或按模块拆开的 sibling test 文件，先在不改变生产行为的前提下降低 `lib.rs` / `app.rs` / `event.rs` / `render.rs` 的体量与阅读噪音。
- **Complexity**: L
- **Planning Note**: 这是最低风险、收益最高的第一步；当前主要目标包括 `crates/omega-session/src/lib.rs`、`crates/omega-workflow/src/lib.rs`、`crates/omega-tui/src/app.rs`、`crates/omega-tui/src/event.rs`、`crates/omega-tui/src/render.rs`、`crates/omega-client/src/lib.rs` 与 `crates/omega-core/src/lib.rs`。
- **Progress (2026-03-25)**: `omega-core` 已先行切到 sibling `lib_tests.rs`；本轮继续把 `omega-workflow/src/lib.rs`、`omega-client/src/lib.rs`、`omega-tui/src/event.rs`、`omega-tui/src/app.rs`、`omega-tui/src/render.rs` 与 `omega-session/src/lib.rs` 的内联测试整体抽到 sibling test 文件，验证了“保留私有 helper 可见性、减少主文件体量”的低风险模式。后续如需继续收敛测试边界，可再把更适合 public-contract 验证的测试逐步下沉到 crate-level `tests/`。
- **Blocks**: Task 15F-16, Task 15F-17, Task 15B-24, Task 15B-25, Task 15B-26, Task 2B, Task 14A
- **Related**: docs/specs/omega-agent-impl-plan.md, docs/specs/omega-step-session-asset-model.md, docs/specs/omega-workflow-package.md, docs/specs/omega-tui-runtime-experience.md

### Task 15F-16: omega-session — Session Runtime Decomposition
- **Status**: Completed
- **Priority**: High
- **Description**: 将 `omega-session` 当前单体 `lib.rs` 拆为更稳定的模块边界，至少分离 `session state/context`、`workflow turn runner`、`structured output validation & recovery`、`routing heuristics`、`prompt builders` 与 `runtime UI emitters`，同时保持 `AgentSession` 对外 API 稳定。
- **Complexity**: XL
- **Planning Note**: 当前 `omega-session` 同时承载 `AgentSession` / `SessionContext`、`WorkflowTurnRunner`、output repair/validation、scene/workflow promotion、tool/detail preview、runtime UI envelope emit 与大块测试；继续在这个文件上叠加 Task 10 / Task 11 会持续放大维护成本。
- **Progress (2026-03-25)**: `omega-session` 现已把原单体 `lib.rs` 收敛为稳定入口层，并拆出 `session_state.rs`、`routing.rs`、`output.rs`、`prompt_builder.rs`、`ui_emit.rs` 与 `runner.rs` 六个内部模块，分别承载 session state/context、scene/workflow routing heuristics、structured output validation/recovery、prompt 构建、runtime UI emit/streaming，以及 workflow turn runner 编排。重构后保持 `AgentSession` 外部 API 与原有 sibling tests 可见性不变，并已通过 `cargo fmt -p omega-session`、`cargo build -p omega-session` 与 `cargo test -p omega-session` 验证。
- **Blocks**: Task 10, Task 11
- **Related**: docs/specs/omega-step-session-asset-model.md, docs/specs/omega-scene-routing.md, docs/specs/omega-runtime-ui-message-contract.md

### Task 15F-17: omega-workflow — Workflow Model / Config / Builtin Defaults Split
- **Status**: Completed
- **Priority**: High
- **Description**: 将 `omega-workflow` 当前单体 `lib.rs` 拆为 `tool policy`、`scene/workflow catalog model`、`TOML config parsing`、`builtin workflow/prompt/schema defaults` 与 `filesystem materialization/loading` 等模块，降低 workflow contract 继续演化时的改动面。
- **Complexity**: L
- **Planning Note**: 当前文件同时包含内建 prompt/schema/TOML 常量、catalog model、config parser、默认文件生成与测试；这类“配置模型 + 内建内容 + 文件系统写入”混排已经开始阻碍后续 workflow 扩展。
- **Completed**: 2026-03-25
- **Progress (2026-03-25)**: `omega-workflow` 已将原单体 `lib.rs` 收敛为薄入口层，并拆出 `constants.rs`、`policy.rs`、`defaults.rs`、`model.rs`、`config.rs` 与 `loading.rs` 六个内部模块，分别承载路径/ID 常量、tool policy 归一化、builtin workflow/prompt/schema 默认内容、scene/workflow catalog model、TOML config parsing，以及 filesystem materialization/loading。重构后保留 root 级 public API 与 sibling tests 兼容性，并已通过 `cargo fmt -p omega-workflow`、`cargo build -p omega-workflow` 与 `cargo test -p omega-workflow` 验证。
- **Related**: docs/specs/omega-workflow-package.md, docs/specs/omega-scene-routing.md, docs/specs/omega-agent-impl-plan.md

### Task 15B-24: omega-tui — App State / Diagnostics Helper Split
- **Status**: Completed
- **Completed**: 2026-03-25
- **Priority**: High
- **Description**: 将 `omega-tui/src/app.rs` 中的 `App` 状态机与 `diagnostics formatting`、`text selection/wrap helpers`、`todo / response summarization helpers` 分离，确保主状态容器不再与大量展示辅助逻辑耦合。
- **Complexity**: L
- **Planning Note**: 当前 `app.rs` 同时持有 `App` 主状态、diagnostics line/detail 构造、tool/response/thinking 摘要、文本选择与 wrap helper，以及大块测试；适合先从 helper 抽离开始。
- **Blocked by**: Task 15F-15
- **Summary**: `omega-tui/src/app.rs` 已拆出 `app/diagnostics.rs`、`app/response.rs`、`app/text.rs` 与 `app/todo.rs` 四个内部子模块，分别承接 contract diagnostics 构造、response/tool/thinking timeline helper、panel text selection/wrap，以及 todo snapshot/summary helper；`App` 根文件现主要保留状态定义、overlay/sidebar/focus 流转与少量标题/模式逻辑，对 `render.rs` / `event.rs` 的现有调用面保持兼容。验证已通过 `cargo fmt -p omega-tui` 与 `cargo test -p omega-tui`。
- **Related**: docs/specs/omega-tui-runtime-experience.md, docs/specs/omega-tui-input-status-layout.md

### Task 15B-25: omega-tui — Event Routing Module Split
- **Status**: Completed
- **Completed**: 2026-03-25
- **Priority**: High
- **Description**: 将 `omega-tui/src/event.rs` 拆为 `keyboard`、`overlay intent`、`mouse`、`clipboard` 与输入编辑辅助模块，避免单文件继续承载所有交互入口。
- **Complexity**: M
- **Planning Note**: 当前 `event.rs` 的核心问题不是算法复杂，而是多种输入通道和 overlay 逻辑都堆在一个文件内；后续继续加快捷键、overlay 或 sidebar 交互会更难守住边界。
- **Blocked by**: Task 15F-15
- **Summary**: `omega-tui/src/event.rs` 已收瘦为统一入口，并拆出 `event/key.rs`、`event/overlay_handlers.rs`、`event/mouse.rs` 与 `event/clipboard.rs` 四个内部子模块，分别承接 key routing / action dispatch、overlay 按键与 confirm intent、鼠标命中与选择滚动，以及剪贴板复制 helper；根层保留 `handle_event` 与测试所需 seam，现有 `event_tests.rs` 无需重写即可继续覆盖 submit、overlay、sidebar、mouse selection 与 clipboard 路径。验证已通过 `cargo fmt -p omega-tui` 与 `cargo test -p omega-tui --color never`。
- **Related**: docs/specs/omega-tui-modal-keymap.md, docs/specs/omega-tui-overlay-popups.md, docs/specs/omega-tui-runtime-experience.md

### Task 15B-26: omega-tui — Render Pipeline Split
- **Status**: Completed
- **Completed**: 2026-03-25
- **Priority**: High
- **Description**: 将 `omega-tui/src/render.rs` 拆为 `main layout`、`sidebar`、`status bar`、`overlay` 与 `style helpers` 模块，降低 UI 扩展时的渲染耦合。
- **Complexity**: M
- **Planning Note**: 当前 `render.rs` 同时处理主布局、底部状态带、侧栏、overlay、样式辅助与测试；继续把更多 runtime 面板或视觉语义叠加在同一文件内会让改动难以定位。
- **Blocked by**: Task 15F-15
- **Summary**: `omega-tui/src/render.rs` 已收瘦为模块根，并拆出 `render/layout.rs`、`render/sidebar.rs`、`render/status.rs`、`render/overlay.rs` 与 `render/style.rs` 五个内部子模块，分别承接主布局、侧栏、上下文/状态带、overlay 与 response/thinking 样式逻辑；根层继续保留 `render` 入口和 `render_tests.rs` 依赖的 helper seam，因此现有渲染测试无需重写即可继续覆盖 wrap、status/context bar 与 style 行为。验证已通过 `cargo fmt --all` 与 `cargo test -p omega-tui --color never`。
- **Related**: docs/specs/omega-tui-runtime-experience.md, docs/specs/omega-tui-collapsible-sidebar.md, docs/specs/omega-tui-overlay-popups.md

### Task 2B: omega-client — Provider Client Module Split
- **Status**: Completed
- **Completed**: 2026-03-25
- **Priority**: Medium
- **Description**: 将 `omega-client/src/anthropic.rs` 拆为 `provider models`、`services`、`transport`、`stream parsing` 模块，并把 `omega-client/src/lib.rs` 中的 provider-neutral chat contract、response builder 与 `Minimax` adapter 进一步分离。
- **Complexity**: L
- **Planning Note**: 当前 client 层仍可读，但 `anthropic.rs` 已同时承载 typed models、service surface、transport 与 SSE parser；继续增加 provider 行为差异会快速把这个文件推向第二个 `omega-session`。
- **Blocked by**: Task 15F-15
- **Summary**: `omega-client` 现已把 provider-neutral chat contract、`ChatResponseBuilder` 与 `Minimax` adapter 分别抽到 `src/types.rs`、`src/builder.rs` 与 `src/minimax.rs`，并将 `anthropic.rs` 收瘦为模块入口，拆出 `anthropic/types.rs`、`anthropic/client.rs`、`anthropic/transport.rs` 与 `anthropic/stream.rs`；`omega_client::...` 与 `omega_client::anthropic::...` 现有导出面保持稳定，原有 `lib_tests.rs` seam 也继续保留。验证已通过 `cargo fmt --all --check` 与 `cargo test -p omega-client`。
- **Related**: docs/specs/omega-client-anthropic-api-abstraction.md, docs/specs/omega-agent-impl-plan.md

### Task 14A: omega-core — Agent Loop / Tool Factory Split
- **Status**: Completed
- **Completed**: 2026-03-25
- **Priority**: Medium
- **Description**: 将 `omega-core` 根文件中的 `Agent` loop、tool result glue、默认 tool factory 与测试拆开，保持 `Agent` 对外语义稳定，但让核心循环与工具装配不再同文件演化。
- **Complexity**: M
- **Planning Note**: `omega-core` 目前还没有 `omega-session` 那么紧急，但已经出现“核心 loop + 默认 tool wiring + 大量测试”共存的趋势，应该在继续扩展前先做边界收口。
- **Blocked by**: Task 15F-15
- **Summary**: `omega-core` 现已将原单体 `lib.rs` 收敛为薄入口层，并拆出 `agent.rs`、`tool_factory.rs` 与 `helpers.rs` 三个内部模块，分别承接 `Agent` loop、默认 builtin tool 装配与 tool-result/todo reminder 辅助逻辑；root 级 public API 与 sibling `lib_tests.rs` seam 保持兼容。验证已通过 `cargo fmt --all --check` 与 `cargo test -p omega-core`。
- **Related**: docs/specs/omega-agent-impl-plan.md, docs/specs/omega-runtime-ui-message-contract.md

### Task 15F-18: docs/specs — Large Spec Split And Index Cleanup
- **Status**: Completed
- **Completed**: 2026-03-25
- **Priority**: Medium
- **Description**: 将 `docs/specs/omega-agent-impl-plan.md` 与 `docs/specs/omega-step-session-asset-model.md` 拆为“索引页 + 主题子文档”，把实现计划、runtime contract、session asset 演进、routing/repair/diagnostics 等主题从单体 spec 中分离出来。
- **Complexity**: M
- **Planning Note**: 当前两份 spec 已明显超出“单主题文档”规模；如果不先拆，后续在实现 Task 10 / Task 11 和 runtime follow-up 时会继续把设计历史、当前契约与未来计划混写在一起。
- **Summary**: 原路径已保留为稳定入口页，并新增 `docs/specs/omega-agent-impl-plan/` 与 `docs/specs/omega-step-session-asset-model/` 两组主题子文档，分别收敛实现计划、session asset/context contract 与 routing/repair/diagnostics 细节；同时已更新 `docs/README.md` 与相关 spec 链接，避免继续由单体大文件承担导航与细节双重职责。
- **Related**: docs/specs/omega-agent-impl-plan.md, docs/specs/omega-step-session-asset-model.md, docs/README.md

### ── M2D: Step Lifecycle Hooks And Gate Follow-up ──

> 验证方式：通过 deterministic mock LLM harness 稳定复现 no-progress execute、partial progress、hook deny/allow advance、structured output retry、tool side effects 等场景；`feature` / `research` execute 的完成判据不再依赖散落在 session 中的特例分支
> 对标：把当前“prompt 说要围绕 todo 执行”提升为 runtime-owned step lifecycle contract，同时允许 repo-local Rust hooks 在 `.omega/hooks/` 下参与工程控制

_2026-03-25 复审补充：相关方案已先记录到 `docs/specs/omega-step-lifecycle-hooks.md`。`Task 15F-15 ~ 15F-18` 已解除原先的结构阻塞，因此后续应按当前真实模块边界推进：`15F-19` 先把 `omega-session/src/lib_tests.rs` 中现有 scripted mock client 抽成稳定 test-support，`15F-20` 只负责 workflow contract 与 manifest 约定，`15F-21` 再落地独立 hook host / storage，`15F-22` 最后替换 `runner.rs` 中现有 execute repeat stopgap。_

### Task 15F-19: omega-session / omega-client — Deterministic Scripted Workflow Harness
- **Status**: Completed
- **Completed**: 2026-03-25
- **Priority**: High
- **Description**: 将当前集中在 `omega-session/src/lib_tests.rs` 的 `SequencedClient` / `IdleClient` 与 response 向量脚本提炼为稳定的 scripted workflow test-support，首轮优先服务 `omega-session::runner` 与 `omega-client` streaming/compat 路径，支持脚本化模拟 response、streaming、structured output repair/regenerate、tool_use、execute repeat，以及后续 hook deny/allow advance 等场景。
- **Complexity**: M
- **Planning Note**: 该能力不是附属测试优化，而是后续 hook/gate 方案的前置安全网；当前已有大量 repair/repeat 测试覆盖，但 mock 支撑仍散落在 sibling tests 中。首轮不强求新 crate，先收敛出可复用 test-support API，再视跨 crate 复用程度决定是否提升为共享测试包。
- **Blocks**: Task 15F-21, Task 15F-22, Task 10, Task 11
- **Related**: docs/specs/omega-step-lifecycle-hooks.md, docs/specs/omega-step-session-asset-model.md
- **Summary**: `omega-client` 已新增 feature-gated 的 `test_support` 模块，提供可录制请求元数据的 `ScriptedLlmClient`、支持 stream/script 混合脚本的 step builder，以及共享 `IdleLlmClient`；`omega-session` 原本散落在 `lib_tests.rs` 中的 `SequencedClient` / `IdleClient` 已切到该共享 harness，`omega-tui` event tests 也复用同一 idle client。验证已通过 `cargo test -p omega-client -p omega-session -p omega-tui`。

### Task 15F-20: omega-workflow / .omega — Hook-Aware Step Lifecycle Contract
- **Status**: Completed
- **Completed**: 2026-03-25
- **Priority**: High
- **Description**: 在 `omega-workflow/src/model.rs`、`config.rs` 与 `defaults.rs` 中为 step 配置增加 `hooks[]` 与 `max_step_repeats` 这类最小生命周期字段，并定义 `.omega/hooks/*/Hook.toml` 的声明式 manifest contract，让“step 绑定哪些 hook”成为 workflow contract 的正式部分。
- **Complexity**: L
- **Planning Note**: 本任务只负责 workflow model / config / defaults 与 repo-local manifest 约定，不负责真正执行 hook；核心目标是把生命周期扩展点从 runtime 特例提升为显式可声明的 step 结构。filesystem discovery、artifact loading 与 ABI 细节留给 Task 15F-21。
- **Blocks**: Task 15F-21, Task 15F-22
- **Related**: docs/specs/omega-step-lifecycle-hooks.md, docs/specs/omega-workflow-package.md
- **Summary**: `omega-workflow` 已为 `WorkflowStep` 新增 `hooks` 与 `max_step_repeats`，`config.rs` 已支持相应 TOML 解析与 hook id 校验，builtin execute step 默认 repeat budget 为 `8`，默认写盘的 `feature/research` workflow TOML 也已显式包含 `max_step_repeats = 8` 与 `hooks = []`，并在注释中固定 `.omega/hooks/<hook-id>/Hook.toml` 的 manifest 约定。验证已通过 `cargo test -p omega-workflow -p omega-session`。

### Task 15F-21: omega-hooks / omega-session — Rust Hook Host And Step-Scoped Storage
- **Status**: Completed
- **Completed**: 2026-03-25
- **Priority**: High
- **Description**: 引入独立的 Rust hook host（必要时同时提供轻量 `omega-hook-sdk` / fixture 支撑），实现 manifest 解析、ABI-safe artifact loading、单方法 lifecycle hook dispatch、step-scoped runtime storage，以及对当前 step 可见工具、session context、todo 系统的受控访问。
- **Complexity**: XL
- **Planning Note**: 由于 `omega-session` 已拆出 `runner.rs` / `session_state.rs` / `ui_emit.rs`，hook host 不应重新并回 session 根模块；更合理的边界是由独立 host crate 提供 loader/dispatch/storage，`omega-session` 只通过窄接口消费 hook 决策与 diagnostics。
- **Blocks**: Task 15F-22, Task 10, Task 11
- **Related**: docs/specs/omega-step-lifecycle-hooks.md, docs/specs/omega-step-session-asset-model.md
- **Summary**: 已新增独立 `omega-hooks` crate，落地 `.omega/hooks/*/Hook.toml` manifest catalog、JSON-over-C-ABI artifact loading、`HookHost` / `HookSession` dispatch 与 step-scoped storage；`omega-session` 侧新增 `hook_adapter` 窄接口并在 `runner` 中接入 `BeforeStep`、`AfterToolCall`、`AfterStep`、`StepFailed` lifecycle dispatch，repeat 期间保留 step storage，`AfterStep/StepFailed` 才清理；同时补齐真实编译 fixture 的 `omega-hooks` loader 测试与 `omega-session` runtime integration test。验证已通过 `cargo test -p omega-hooks -p omega-session`。

### Task 15F-22: omega-session / omega-workflow — Hook-Driven Advance Gate And Step Repeat
- **Status**: Completed
- **Completed**: 2026-03-25
- **Priority**: High
- **Description**: 在 `omega-session/src/runner.rs` 中于 output contract 满足之后统一走 `BeforeAdvance` lifecycle gate，由 hook 决定是否允许进入下一步；若被拒绝，则保持当前 step 重复执行直到 `max_step_repeats` 耗尽，并替换当前 `should_repeat_execute_step()` 与 `EXECUTE_REPEAT_MAX_NO_PROGRESS_ATTEMPTS` 这类 scene-specific execute stopgap。
- **Complexity**: XL
- **Planning Note**: 这是把当前 `feature/research execute` 的 todo-driven repeat 从 localized stopgap 提升为通用 contract 的关键一步；当前特例已经基本收敛在 `runner.rs`，因此本任务的目标不是重写整个 session，而是把 repeat/gate 判定替换成统一 lifecycle dispatch，并同步接入现有 diagnostics / runtime UI contract。
- **Related**: docs/specs/omega-step-lifecycle-hooks.md, docs/specs/omega-step-session-asset-model.md, docs/specs/omega-runtime-ui-message-contract.md
- **Summary**: `omega-session::runner` 现已在 output contract 通过后统一调用 `BeforeAdvance` gate，并按 `WorkflowStep.max_step_repeats` 处理 deny → repeat / exhaust → fail；旧的 `should_repeat_execute_step()` 与 `EXECUTE_REPEAT_MAX_NO_PROGRESS_ATTEMPTS` stopgap 已删除。`omega-hooks` 侧新增内建 `todo_managed_execute` fallback，默认 `feature` / `research` execute workflow 与 repo-local `.omega/workflows/*.toml` 现显式绑定 `hooks = ["todo_managed_execute"]`，从而在没有 repo-local manifest 时仍保留 todo-driven repeat 语义；同时补齐了 `omega-hooks`、`omega-workflow` 与 `omega-session` 的回归测试。验证已通过 `cargo test -p omega-hooks -p omega-workflow -p omega-session`。

### ── M2E: Runtime Message Pipeline ──

> 验证方式：routing / response / thinking / tool / todo / diagnostics 先产出 frontend-neutral 的 `RuntimeMessageEnvelope { turn_id, message }`；`omega-tui` runtime 保持 current-turn 过滤；`omega-app` 装配消息策略；`omega-tui` 通过 `TuiEngine` 执行渲染
> 对标：`omega-session` = 消息生产者，`omega-app` = 策略装配者，`omega-tui` = runtime shell + 渲染引擎

_2026-03-26 实现更新：runtime message pipeline 已按该收敛方案落地。当前主路径由 `RuntimeMessageEnvelope -> current-turn filter -> RuntimeMessagePolicy -> TuiEngine` 组成：session 产出 frontend-neutral message，app 装配消息到渲染的 policy，tui 继续拥有 event loop、turn 过滤和渲染执行；旧 `RuntimeUiEnvelope` 降为 compat surface，供 legacy consumer 与回归测试继续使用。后续 `subagent/background/message/team` 等 runtime-visible producer 应直接接入该消息管道。_

### Task 15F-23: omega-session / omega-app / docs — Runtime Message Pipeline Plan
- **Status**: Completed
- **Completed**: 2026-03-26
- **Priority**: High
- **Description**: 固化更小的 v0.3 收敛方案：frontend-neutral `RuntimeMessageEnvelope { turn_id, message }`、`omega-app` 的消息策略 ownership，以及 `omega-tui` 继续拥有 runtime shell + `TuiEngine` 的边界，替换当前 `RuntimeUiEnvelope` 同时承担 runtime event 与 TUI command 的混合语义。
- **Complexity**: M
- **Summary**: 已固定并实现 v0.3 收敛方案：保留 `turn_id` transport envelope、`omega-tui` runtime shell ownership 与 app-owned message policy，同时同步更新 `TODO`、Task 15 runtime visibility 计划和实现计划入口页，使文档与代码主路径一致。
- **Related**: docs/specs/omega-runtime-message-pipeline.md, docs/specs/omega-runtime-ui-message-contract.md, docs/specs/omega-app-package.md, docs/specs/omega-tui-runtime-experience.md

### Task 15F-24: omega-session / omega-core / omega-workflow — Frontend-Neutral RuntimeMessage Contract
- **Status**: Completed
- **Completed**: 2026-03-26
- **Priority**: High
- **Description**: 将当前 session-owned `RuntimeUiEnvelope` baseline 收敛为 frontend-neutral `RuntimeMessageEnvelope { turn_id, message }`，其中 `message` 拆分为 `ConversationMessage`（response panel timeline 流）和 `StateMessage`（status / sidebar / log 状态更新）；迁移期间保留 compat adapter，避免一次性切断现有 TUI 主路径。
- **Complexity**: XL
- **Summary**: `omega-session` 已新增 frontend-neutral `runtime_message.rs`，主路径改为发出 `RuntimeMessageEnvelope`、`ConversationMessage` 与 `StateMessage`；`LegacyRuntimeUiBridge` 与 `spawn_turn_ui_compat()` 保留 `RuntimeUiEnvelope` 兼容路径，producer tests 已覆盖 streaming sections、tool activity 与 turn-finish transport。
- **Related**: docs/specs/omega-runtime-message-pipeline.md, docs/specs/omega-runtime-ui-message-contract.md, docs/specs/omega-agent-impl-plan/task-15-runtime-visibility.md

### Task 15B-27: omega-tui / omega-app — TuiEngine Surface API And App Policy
- **Status**: Completed
- **Completed**: 2026-03-26
- **Priority**: High
- **Description**: 在保留 `omega-tui` event loop / terminal lifecycle / current-turn 过滤的前提下，引入 `TuiEngine` surface-oriented API，并由 `omega-app` 装配 `RuntimeMessagePolicy` 来组织渲染；逐步替代当前 reducer 中对 surface-heavy envelope 的语义分流。
- **Complexity**: L
- **Summary**: `omega-tui` 已新增 `TuiSurface` / `TuiEngine` 与 `apply_runtime_message_with_policy()` helper，runtime loop 现直接消费 `RuntimeMessageEnvelope`；`omega-app` 已注入 `DefaultRuntimeMessagePolicy`，接管 message → response/activity/status/todo/diagnostics 的渲染策略。
- **Related**: docs/specs/omega-runtime-message-pipeline.md, docs/specs/omega-tui-runtime-experience.md, docs/specs/omega-tui-response-thinking-experience.md

### Task 15F-25: omega-session / omega-app / omega-tui — Runtime Message Pipeline Tests
- **Status**: Completed
- **Completed**: 2026-03-26
- **Priority**: High
- **Description**: 建立 deterministic 的 `RuntimeMessageEnvelope -> current-turn filter -> app policy -> TuiEngine` 测试矩阵，覆盖 routing、step response、thinking、tool run、todo snapshot、diagnostics、stale turn drop 以及 future module stub 的渲染规则，防止收敛后再次回到按 feature 堆特例。
- **Complexity**: M
- **Summary**: `omega-app` 已新增 deterministic matrix tests，锁定 stale turn drop、routing activity、response section、tool lifecycle、todo snapshot、diagnostics 与 `TurnFinished` 映射；`omega-session` 同步补充 producer-path tests，并以 `cargo test -p omega-session -p omega-tui -p omega-app --color never` 验证通过。
- **Related**: docs/specs/omega-runtime-message-pipeline.md, docs/specs/omega-agent-impl-plan/task-15-runtime-visibility.md

### ── M2F: Execute Todo Loop Follow-up ──

> 验证方式：`feature/research.execute` 的 Diagnostics 能稳定显示 `todo_total / completed / open / current_item / repeat_count / no_progress_streak`；当 step 配置为基于 todo 列表循环时，runtime 会把单个 `execute` 展开为稳定的 itemized run（如 `execute-1`、`execute-2`），直到所有 required todo items 完成后才进入 `report`
> 对标：把当前 `todo_managed_execute + max_step_repeats` 的 whole-step repeat 收敛成更稳定的 item-level contract，而不是在 `BeforeAdvance` deny 上继续堆特例

_2026-03-26 规划补充：当前 `feature/research.execute` 已能围绕 todo 重试，但一个 step 完成、失败或耗尽 repeat budget 后，Diagnostics 仍不足以解释“总共有多少 todo、完成了几个、当前卡在哪个 item”，runtime 也仍把整个 `execute` 当作单个 step。下一轮 follow-up 先补 execute progress diagnostics，再引入 itemized execute loop contract，让 `execute` 在 runtime 中拥有稳定的子 id 和 per-item 完成语义。_
_2026-03-26 架构评审后收敛：(1) `BeforeAdvance` 在 item loop 下变为 per-item gate，runtime 接管 item progression，hook 不需要理解 orchestration；(2) `max_step_repeats` 保留为总预算，`loop_contract.max_item_repeats` 为单项上限，双层控制；(3) loop source 解析由 `runner.rs` 拥有，封装为 `resolve_loop_items()` 纯函数；(4) 诊断结构从 15F-26 开始预留 optional item 粒度（`ExecuteProgressDiagnostics` 嵌套），避免 15F-28 重写；(5) `HookDispatchInput` 新增 item context 为 additive-only，不需 api_version bump。_
### Task 15F-26: omega-session / omega-observability / omega-tui — Execute Todo Progress Diagnostics
- **Status**: Completed
- **Priority**: High
- **Description**: 扩展 `Contract Diagnostics` 与 tracing / runtime activity，使 `feature/research.execute` 在每次模型尝试、hook deny、todo sync 与 advance 判定时都显式记录 `todo_total`、`todo_completed`、`todo_open`、`current_todo_ids`、`repeat_count`、`no_progress_streak`、`max_step_repeats` 与最终 completion source，避免用户只看到“Execute repeated/finished”却不知道 todo 维度上的真实进展。
- **Complexity**: M
- **Planning Note**: 该任务只补观测，不改变 loop 语义；目标是先让"为什么继续重试 / 为什么提前结束 / 为什么耗尽预算"在 Diagnostics 中可解释，为后续 itemized loop 改造提供基线。诊断结构须从一开始预留 optional item 粒度：在 `StepDiagnostics` 上新增 `execute_progress: Option<ExecuteProgressDiagnostics>` 嵌套结构，其中 `current_item_id` / `item_index` / `item_total` / `max_item_repeats` 在本任务中为 `None`，15F-28 填充后无需重写路径。
- **Completed Note**: `StepDiagnostics` 已新增 `execute_progress`，`omega-session` 在 input/output diagnostics 中稳定填充 todo totals、open/completed、current item、repeat/no-progress、budget 与 completion source，`omega-tui` sidebar/detail overlay 也已展示该结构。
- **Related**: docs/specs/omega-step-session-asset-model/session-context-and-data-contracts.md, docs/specs/omega-step-lifecycle-hooks.md, docs/specs/omega-runtime-message-pipeline.md

### Task 15F-27: omega-workflow / omega-session — Itemized Execute Loop Contract
- **Status**: Completed
- **Priority**: High
- **Description**: 为 workflow step 增加 itemized execute loop contract，把当前“整个 `execute` step 重复直到 todo 收敛”的语义提升为显式可声明的外层 loop：step 可以绑定某个列表 source（首轮为 todo / plan.tasks），runtime 会把逻辑上的单个 `execute` 展开为共享上下文的 item runs，并为每个 item 提供稳定子 id（如 `execute-1`）与 per-item completion gate。
- **Complexity**: XL
- **Planning Note**: 不建议继续重载现有 `loop_mode` 字符串别名来同时表达"单次 agent/tool loop"和"按列表展开的外层循环"；更稳妥的方向是保留现有内层 agent loop 语义，再新增显式 loop contract / loop source 字段，避免把两个独立维度混进一个开关。设计约束：(1) loop source resolution 由 `runner.rs` 拥有，封装为纯函数 `resolve_loop_items()`；(2) `loop_contract` 新增 `max_item_repeats` 字段与 `max_step_repeats` 构成双层预算；(3) `HookDispatchInput` 新增 `current_item_id / item_index / item_total` 为 additive-only change，不需要 `api_version` bump；(4) v1 loop source 限 `todo_items` + `plan.tasks`。
- **Completed Note**: `WorkflowStep` / workflow TOML 已支持 `loop_contract = { kind = "todo_items", source = "plan.tasks", child_step_prefix = "execute", max_item_repeats = N }`，builtin `feature` / `research` execute 默认开启该 contract，并由 tests 锁定 parse/default 行为。
- **Blocks**: Task 10
- **Related**: docs/specs/omega-step-lifecycle-hooks.md, docs/specs/omega-workflow-package.md, docs/specs/omega-step-session-asset-model/session-context-and-data-contracts.md

### Task 15F-28: omega-session / omega-hooks / omega-tui — Itemized Execute Loop Runtime And Visibility
- **Status**: Completed
- **Priority**: High
- **Description**: 在 runtime 中实现 itemized execute loop：`BeforeAdvance` 不再只对整个 `execute` 给出 allow/deny，而是先基于 loop contract 推进当前 item、同步 todo 与 step-scoped storage，并在前端中把 `execute` 展示为带子 id 的 item runs（例如 `execute-1` / `execute-2`）；只有当 required items 全部完成后，父级 `execute` 才允许进入 `report`。
- **Complexity**: XL
- **Planning Note**: 该任务是 execute 稳定性的主收敛点；实现时必须保留共享 `SessionContext`、structured input/output 与现有 hook storage，同时补齐 deterministic matrix tests，覆盖 no-progress、partial progress、all-complete、item fail、budget exhaustion 与 research read-only execute 路径。`BeforeAdvance` 在 item loop 下变为 per-item gate，runtime 接管 item progression（见 lifecycle hooks spec "BeforeAdvance 双层语义过渡"）。如实现复杂度超出单轮收敛范围，可拆为 15F-28a（runtime loop + tests）和 15F-28b（TUI item-level visibility），但优先尝试一次完成。
- **Completed Note**: `omega-session` 现按当前 todo item 驱动 execute repeat，并在 item 完成后由 runtime 推进到下一个 item；`HookDispatchInput` 已带 `current_item_id / item_index / item_total`；builtin `todo_managed_execute` 改为 per-item gate；新增 deterministic tests 覆盖 execute progress diagnostics 与 `max_item_repeats` exhaustion。2026-03-27 follow-up：`execute` 的 optional structured output contract 现也支持 `max_retries / recovery_mode`，当模型错误地一次关闭 future items 时会先 repair/regenerate，而不是直接整步失败。后续又补了两层容错：itemized execute output 会自动把 future items 从 `completed_tasks` 挪回 `open_tasks`，`parse_json_values()` 也会在保留原数组候选的同时解包 array-of-object repair output，避免 `expected object at $` 这类 schema 失败反复中断 execute。
- **Completed Note**: `omega-session` 现按当前 todo item 驱动 execute repeat，并在 item 完成后由 runtime 推进到下一个 item；`HookDispatchInput` 已带 `current_item_id / item_index / item_total`；builtin `todo_managed_execute` 改为 per-item gate；新增 deterministic tests 覆盖 execute progress diagnostics 与 `max_item_repeats` exhaustion。2026-03-27 follow-up：`execute` 的 optional structured output contract 现也支持 `max_retries / recovery_mode`，当模型错误地一次关闭 future items 时会先 repair/regenerate，而不是直接整步失败。后续又补了两层容错：itemized execute output 会自动把 future items 从 `completed_tasks` 挪回 `open_tasks`，`parse_json_values()` 也会在保留原数组候选的同时解包 array-of-object repair output，避免 `expected object at $` 这类 schema 失败反复中断 execute。2026-03-31 follow-up：hook-managed itemized execute 现把 JSON execute output 视为 canonical contract，repo-local/default workflow 都改为 required；`runner` 会用修正后的 structured output 生成 execute summary，并在 advance gate exhaustion 前补发最终 output diagnostics，避免 UI 停在 `output pending` 或把已 auto-repair 的 future completions 继续写回 session summary。
- **Blocks**: Task 10, Task 11
- **Related**: docs/specs/omega-step-lifecycle-hooks.md, docs/specs/omega-step-session-asset-model/session-context-and-data-contracts.md, docs/specs/omega-runtime-message-pipeline.md

_2026-03-26 TUI follow-up planning：M2F 已把 execute 收敛为 itemized loop，但 `Response` 仍缺“step 内子流程”层级；下一轮需要把 item run 保持为父级 `execute` block 的 nested subflow，而不是新的顶层 workflow step。详细设计见 `docs/specs/omega-tui-step-subflow-visibility.md`。_

### Task 15F-29: omega-session / omega-app / omega-tui — Step-Owned Subflow Presentation Contract
- **Status**: Completed
- **Completed**: 2026-03-26
- **Priority**: Medium
- **Description**: 为 itemized execute 和未来 step 内子流程建立 frontend-neutral 的 presentation contract：response section metadata 需可携带 `subflow_ref`，state channel 需能稳定表达当前 item run 的 identity 与状态，使 app policy / TuiEngine 不必再从 diagnostics 或日志文本推断子流程模型。
- **Complexity**: M
- **Planning Note**: 关键约束是“item run 属于父级 step，而不是新的 workflow step”。推荐新增通用 `StepSubflowStatus` state message，并为 response section metadata 增加 optional `subflow_ref`；`omega-app` 负责把它映射到 step header、bottom status、activity 与 TUI subflow lane。
- **Completed Note**: `omega-session` 已新增 `StepSubflowStatus` / `subflow_ref` contract，并由 runner 在 itemized execute 期间发出稳定子流程状态；`omega-app` policy 与 `omega-tui` surface 已消费该 contract，无需再从 diagnostics 或日志文本反推当前 item run。
- **Blocks**: Task 15B-28
- **Related**: docs/specs/omega-tui-step-subflow-visibility.md, docs/specs/omega-runtime-message-pipeline.md, docs/specs/omega-step-session-asset-model/session-context-and-data-contracts.md

### Task 15B-28: omega-tui / omega-app — Execute Step Nested Subflow Timeline
- **Status**: Completed
- **Completed**: 2026-03-26
- **Priority**: Medium
- **Description**: 在 `Response` 的 `execute` step block 内新增 nested subflow lane，把 `execute-1` / `execute-2` 等 item run 渲染为父级 step 的二级卡片；当前项默认展开、已完成项默认折叠、失败项保持展开，并与底部状态带及 `Todo` 当前项高亮共享同一 identity。
- **Complexity**: M
- **Completed Note**: `Response` 已将带 `subflow_ref` 的 itemized execute section 聚合为父级 `execute` block 下的 nested subflow lane；当前 item 默认展开、已完成项折叠、未开始项显示 queued token，底部状态带与 `Todo` 当前项高亮已对齐同一 item identity。
- **Blocked by**: Task 15F-29
- **Blocks**: Task 15B-29
- **Related**: docs/specs/omega-tui-step-subflow-visibility.md, docs/specs/omega-tui-response-thinking-experience.md, docs/specs/omega-tui-runtime-experience.md

### Task 15B-29: omega-tui — Subflow Detail Overlay And Navigation
- **Status**: Completed
- **Completed**: 2026-03-26
- **Priority**: Low
- **Description**: 为 step 内子流程补齐 detail overlay 与导航能力：允许在 `Response` 中展开某个 item run 的 compact diagnostics / tool summary / completion source，并支持在多个 item run 之间做稳定跳转与选择恢复。
- **Complexity**: S
- **Completed Note**: `Response` 中的 subflow header 已可直接激活 detail overlay，overlay 会展示 item identity、status、repeat/no-progress、completion source 与 tool/body 摘要；现有 response 选择态与滚动即可在多个 item run 之间稳定移动并恢复焦点。
- **Blocked by**: Task 15B-28
- **Related**: docs/specs/omega-tui-step-subflow-visibility.md, docs/specs/omega-tui-overlay-popups.md, docs/specs/omega-tui-modal-keymap.md

### ── M2G: Deterministic Test Foundation Follow-up ──

> 验证方式：通过共享 scripted/mock harness 稳定复现 LLM streaming、structured output retry、runtime envelope 顺序、stale turn drop、hook lifecycle、tool/process failure、temp-dir 并发与 TUI event replay；对外部边界使用 mock，对 workflow/session/core 逻辑保持尽量 real-tested，避免 flaky regression 再次混进主路径
> 对标：把当前散落在 `omega-client` / `omega-session` / `omega-core` / `omega-subagent` / `omega-tui` 的局部 test support 收敛成一致的 deterministic test seam，而不是每个 crate 各自复制一套 mock client / tmp helper / envelope capture

_2026-03-31 规划补充：当前仓库已经具备 `omega-client::test_support::ScriptedLlmClient`、`IdleLlmClient`、hook fixture 编译测试、runtime message matrix tests 与部分 mock embedding / fake clipboard，但 seam 仍然分散，且 temp-dir、process/tool 与 runtime event replay 没有统一支撑。下一轮 follow-up 的目标不是“到处加 mock”，而是明确只在真实外部边界上 mock，并把 deterministic harness 提升为后续 Task 10/12/13 的配套安全网。_

### Task 15F-30: docs/specs / omega-client / omega-session / omega-app / omega-tui — Deterministic Test Seam Contract
- **Status**: Completed
- **Priority**: High
- **Description**: 盘点并固化当前仓库中真正需要 mock 的外部边界，包括 LLM chat/stream/count_tokens、HTTP/SSE、runtime message bridge、process execution、filesystem/temp root、hook artifact loading 与 TUI input event source；明确哪些逻辑必须继续走真实实现测试（如 workflow/session/core/todo/schema/gate/repair）。
- **Complexity**: M
- **Planning Note**: 该任务先定义 contract，再做大规模 harness 收敛。核心约束是“mock only at true external boundaries”：不要为了测试方便把 `runner`、`AgentSession`、todo sync、output validation 这类内核逻辑一起 fake 掉，否则只会得到稳定但低价值的测试。
- **Completion Note (2026-03-31)**: 新增 `docs/specs/omega-deterministic-test-seams.md`，把共享 LLM/runtime/process/temp-root/TUI replay seam 与“保持 real-tested 的内核逻辑”正式写入规格。
- **Blocks**: Task 15F-31, Task 15F-32, Task 15F-33, Task 15F-34, Task 15F-35
- **Related**: docs/specs/omega-step-lifecycle-hooks.md, docs/specs/omega-runtime-message-pipeline.md, docs/specs/omega-tui-runtime-experience.md

### Task 15F-31: omega-client / omega-core / omega-session / omega-subagent — Shared Scripted LLM Harness Consolidation
- **Status**: Completed
- **Priority**: High
- **Description**: 扩展并统一 `omega-client::test_support`，覆盖 response/scripted stream、event 序列、mid-stream failure、`count_tokens` preset、request recording 与 max-token capture；同时替换 `omega-core` / `omega-subagent` / `omega-session` 中重复的 `MockLlmClient` / `SequencedClient` 实现，收敛到共享 harness。
- **Complexity**: L
- **Planning Note**: `Task 15F-19` 已完成首轮抽离，但当前仍存在重复 mock client 与不同粒度的 response builder。该任务的目标是把 LLM 相关不稳定性集中到单一 harness，而不是继续让每个 crate 自行造轮子。
- **Completion Note (2026-03-31)**: `ScriptedLlmClient` 现支持独立的 chat/stream 与 `count_tokens` 队列、mid-stream failure 与 request recording；`omega-core` / `omega-subagent` / `omega-session` 的重复 test client 已迁移到共享 harness。
- **Blocked by**: Task 15F-30
- **Blocks**: Task 15F-34, Task 10, Task 12, Task 13
- **Related**: crates/omega-client/src/test_support.rs, crates/omega-session/src/lib_tests.rs, crates/omega-core/src/lib_tests.rs, crates/omega-subagent/src/lib.rs

### Task 15F-32: omega-session / omega-app / omega-tui — Runtime Envelope Recorder And Event Ordering Matrix
- **Status**: Completed
- **Priority**: High
- **Description**: 在现有 `RuntimeMessageEnvelope -> current-turn filter -> policy -> engine` 基础上补齐共享 recorder / sink test support，稳定断言 envelope 顺序、stale turn drop、tool/diagnostics/todo interleave 与 step/subflow event ordering，而不再依赖 `mpsc::channel` + 手写循环在测试里逐条捞消息。
- **Complexity**: M
- **Planning Note**: 当前 `Task 15F-25` 已覆盖主路径 matrix，但 producer/consumer 两侧仍然各自手搓 message capture。该任务应该沉淀出可复用 recorder，不改变 runtime contract 本身。
- **Completion Note (2026-03-31)**: `omega-session::RuntimeEnvelopeRecorder` 已落地，并通过 recorder 迁移了 cache diagnostics 与 runtime message ordering 的关键测试，减少了手写 `recv_timeout` 循环。
- **Blocked by**: Task 15F-30
- **Blocks**: Task 15F-34, Task 10, Task 12, Task 13
- **Related**: crates/omega-session/src/runtime_ui.rs, crates/omega-session/src/runtime_message.rs, crates/omega-app/src/runtime_message_policy.rs, crates/omega-tui/src/app_tests.rs

### Task 15F-33: omega-tools / omega-tools-builtin / omega-hooks / workspace — External Boundary Mock Adapters And Stable Temp Roots
- **Status**: Completed
- **Priority**: High
- **Description**: 为 bash/process、filesystem-heavy tool 路径与 hook fixture tests 收敛统一的 mock/fixture adapter，并新增稳定的 shared temp-root helper，消除 timestamp-only `/tmp` 命名、环境命令可用性与并发写入导致的 test flake。
- **Complexity**: L
- **Planning Note**: 不应把所有文件工具都切成纯 mock；工具 contract 仍需保留一层 real fs / real process regression。该任务重点是把“跨 crate 的环境不稳定因素”抽到可控边界，而不是完全放弃真实集成测试。
- **Completion Note (2026-03-31)**: 新增 `omega-test-support` 统一 temp-root helper；`omega-hooks`、`omega-tools-builtin`、`omega-session`、`omega-tui` 测试已迁到共享 temp-root；`BashHandler` 新增可注入 `BashCommandRunner` seam，同时保留真实进程集成测试。
- **Blocked by**: Task 15F-30
- **Blocks**: Task 15F-35, Task 10, Task 12
- **Related**: crates/omega-tools-builtin/tests/common.rs, crates/omega-hooks/tests/hook_host.rs, memories/repo/test-temp-dir-collisions.md

### Task 15F-34: omega-tui / omega-session / omega-app — Deterministic User-Event Replay Tests
- **Status**: Completed
- **Priority**: Medium
- **Description**: 建立可复放的 user-input + runtime-envelope replay harness，覆盖输入编辑、overlay 焦点、sidebar/response 切换、streaming response append、todo/diagnostics 刷新与 step subflow 导航，确保 TUI 行为测试不再依赖手工拼装零散 event 顺序。
- **Complexity**: L
- **Planning Note**: 现有 `app_tests.rs` 与 `event_tests.rs` 已覆盖大量 reducer 行为，但缺统一的“时序级”测试面。该任务应复用 Task 15F-32 的 recorder/sequencer，而不是再造一套 UI 专用 transport。
- **Completion Note (2026-03-31)**: `omega-tui` 新增 `EventReplayHarness`，并补充 overlay 搜索的按键序列回放测试；runtime side 复用 Task 15F-32 的 recorder 断言事件顺序。
- **Blocked by**: Task 15F-31, Task 15F-32
- **Blocks**: Task 12, Task 13
- **Related**: crates/omega-tui/src/app_tests.rs, crates/omega-tui/src/event_tests.rs, docs/specs/omega-tui-runtime-experience.md

### Task 15F-35: workspace-wide — Flaky Test Retirement And Scenario Backfill
- **Status**: Completed
- **Priority**: Medium
- **Description**: 基于前述 shared harness，把当前容易抖动或重复造 mock 的测试迁移到统一 test-support，并为 execute loop、subagent、document search、runtime message、tool side effects 与 read-only research 这些高风险场景补齐 deterministic scenario matrix。
- **Complexity**: XL
- **Planning Note**: 这一步是“完善测试”的真正收口点。目标不是追求覆盖率数字，而是把当前已知高风险链路都锁到稳定、可维护、可解释的场景测试上，并把历史 flaky helper 逐步退役。
- **Completion Note (2026-03-31)**: 本轮已把 `omega-core`、`omega-subagent`、`omega-session`、`omega-hooks`、`omega-tools-builtin`、`omega-tui` 的高风险测试面迁移到共享 seam，并补齐 deterministic scenario coverage。后续补充又为 `omega-session` 加上了 workflow routing fallback 回归，覆盖 valid research scene + invalid/root workflow selection，以及 polluted `selected_workflow_id = root` 的 child-workflow guard；同时为 hook-managed research execute 增加了 stale previous-item completion 回归，避免模型重复已完成 todo item 时继续消耗 repeat budget；最新补充再锁定 `research.report` 的 final-answer 语义兜底，当模型把内部 machine JSON 直接当作最终回答时，runtime 会拒绝该输出并触发一次 regenerate，直到产生用户可读的报告文本；此外 `plan` step 现在也显式要求只返回单个 JSON plan object，不再接受“报告 prose + JSON”混合回答，从而避免 child:research Plan 在 UI 中展示报告正文而不是 TODO 分解结果；随后又收敛到更稳妥的 plan 行为：如果模型回复里包了一层说明文字，但仍只包含唯一合法的 plan JSON object，runtime 现在会接受该 JSON 候选继续执行，同时保持 Plan section 只展示验证后的规范化计划摘要；本次继续收紧 itemized execute 语义：如果模型在当前 execute item 仍未完成时错误地把未来 todo 标记为 completed，runtime 不再静默 auto-repair 成无进展结果，而是直接判 invalid 并重试，避免 research execute 在同一 item 上白白耗尽 `max_item_repeats`；最新修复 research plan 的 read-only 验证误报：当 task 描述以分析前缀开头（analyze/review/evaluate 等）但正文提及 "update"、"config"、"code"、"module" 等优化分析概念时，现在只匹配精确写入短语（"update code"、"modify config" 等），不再做 bag-of-words action+target 交叉匹配，避免分析型任务被误判为写操作而导致 plan 连续 3 次验证失败；进一步放宽 plan 多 JSON 块场景：当模型回显 explore JSON + plan JSON 导致 candidates > 1 时，现在会按 plan schema shape（含 `goal` + `tasks` + `validation_targets`）筛选候选，如果恰好只有一个匹配则接受，不再一刀切拒绝多候选响应。
- **Blocked by**: Task 15F-31, Task 15F-32, Task 15F-33, Task 15F-34
- **Blocks**: Task 10, Task 12, Task 13, Task 16
- **Related**: crates/omega-session/src/lib_tests.rs, crates/omega-document/src/lib.rs, crates/omega-tools-builtin/tests/, crates/omega-app/src/runtime_message_policy.rs

### ── M3: Todo 管理 (s03) ──

> 验证方式：`cargo run` → Agent 自动创建/更新/展示 todo
> 对标：learn-claude-code s03_todo_write.py

### Task 9: omega-todo — TodoManager
- **Status**: Completed
- **Completed**: 2026-03-19
- **Priority**: Medium
- **Description**: 实现 TodoManager，支持 update/render/has_open_items，注册为 tool
- **Summary**: 实现 `TodoManager`（`update/render/has_open_items/should_nag`）和共享 `TodoToolHandler`，支持 todo 项校验（最多 20 项、同一时间仅 1 个 `in_progress`、`id/text/status` 必填、空文本拒绝）、状态渲染和 reminder 状态跟踪；`omega-core` 默认工具集新增 `todo` 工具，agent loop 仅在存在未完成 todo 且连续 3 轮未成功调用 todo 时注入 `<reminder>Update your todos.</reminder>`，成功调用 todo 后重置计数；新增 9 项 `omega-todo` 单测和 5 项 `omega-core` 集成测试，`cargo test` 全工作区通过
- **Related**: docs/specs/omega-agent-impl-plan.md

### ── M4: 子智能体 (s04) ──

> 验证方式：`cargo run` → Agent 将子任务委托给 SubAgent 独立完成
> 对标：learn-claude-code s04_subagent.py

### Task 10: omega-subagent — SubAgent
- **Status**: Pending
- **Priority**: High
- **Description**: 实现 SubAgent，独立 message list + run_loop，父 Agent 通过 tool 调度
- **Planning Note**: `Task 15F-9` 已解除阻塞；当前已先落地 fresh-context `SubAgent` loop（独立 message list、tool loop、tool error 回写与定向测试），后续还需补父 Agent 的 `task` tool 接线与 runtime 可见性。
- **Related**: docs/specs/omega-agent-impl-plan.md, docs/specs/omega-tui-runtime-experience.md

### ── M5: Skill 加载 (s05) ──

> 验证方式：`cargo run` → Agent 按任务类型自动加载 skill 到 system prompt
> 对标：learn-claude-code s05_skill_loading.py

### Task 5: omega-skills — SkillLoader
- **Status**: Completed
- **Completed**: 2026-03-19
- **Priority**: High
- **Description**: 实现 SkillLoader，扫描 skills 目录，按关键词匹配加载
- **Related**: docs/specs/omega-agent-impl-plan.md, docs/specs/omega-tui-runtime-experience.md
- **Summary**: `omega-skills` 新增递归扫描 `.claude/skills` 与 `skills/` 的 `SkillLoader`，支持 frontmatter 读取、技能描述汇总、按任务文本做关键词匹配，并提供 `load_skill` 工具按需返回完整 `<skill ...>` 内容；`omega-core::create_default_tools` 已默认注册该工具，当前主路径上的 `omega-session` 会在每轮按当前输入把匹配到的 skill 正文预装进 system prompt，同时始终附带低成本的 skills 描述列表；相关单测已补齐，并以定向 cargo test 验证通过

### ── M6: 上下文管理 (s06) ──

> 验证方式：长对话后观察 token 使用量下降，cache hit 率上升，对话质量不降；文档治理规则检查通过；多维搜索返回精确结果
> 对标：learn-claude-code s06_context_compact.py
> 设计文档：docs/specs/omega-context-management.md (v0.3)

### Task 11 (original): omega-compression — 上下文压缩 (已拆解)
- **Status**: Superseded → 由 Task 11A ~ 11F 替代
- **Priority**: —
- **Description**: 原任务已拆解为六个子任务，优先修复 prompt/context 主路径，再逐步建立 facade、文档治理、向量检索与可观测性。

### Task 11A: Cache Control + Token Estimation
- **Status**: Completed
- **Completed**: 2026-03-27
- **Priority**: High
- **Complexity**: Medium
- **Description**: 在 prompt builder / message assembly 层注入 Anthropic `cache_control: { type: "ephemeral" }` 标记（4 个锚点：tools → system → summaries → last assistant turn）；升级 token estimation 从 `chars/4` 改为优先调用 provider `count_tokens` API / fallback。不新建 crate，仅修改 `omega-session`（prompt_builder、runner）与 `omega-client`（expose count_tokens）。新增 `CacheDiagnostics` 到 `StepDiagnostics`。
- **Blocks**: Task 11B, Task 11F
- **Related**: docs/specs/omega-context-management.md §1, §Observability
- **Summary**: `omega-client` 已补齐 provider-neutral `count_tokens`、structured system blocks 与 cache hint 映射，并将 Anthropic cache usage 透传到通用 `Usage`；`omega-core` / `omega-session` 已把 step prompts 拆成可缓存 system blocks，在 summary budgeting 时优先走 provider `count_tokens`、失败时回退 request-size estimation，同时把 cache hit/breakpoint 信息写入 `CacheDiagnostics` 并展示到 runtime diagnostics / TUI。2026-03-30 进一步补齐 `omega-context` 与 `omega-session` 回归测试，显式锁定四个 cache anchors 与 provider `count_tokens` 失败时的 estimated fallback。验证已通过 `cargo test -p omega-client -p omega-core -p omega-session -p omega-tui -p omega-app`，并额外通过 `cargo test -p omega-context --color never` 与 `cargo test -p omega-session --lib --color never spawn_turn_falls_back_to_estimated_token_count_and_records_all_cache_breakpoints`。

### Task 11B: Prompt Path Stabilization
- **Status**: Completed
- **Completed**: 2026-03-30
- **Priority**: High
- **Complexity**: High
- **Description**: 直接在现有 `omega-session` 执行路径中落地根因修复：slot budget MVP、priority-weighted summary selection、compaction trigger、Critical/High slot 不可裁剪规则。此阶段不引入 document governance、LanceDB 或 TUI 仪表盘，目标是先让 execute 多轮 structured output 稳定。
- **Blocks**: Task 11C, Task 10
- **Related**: docs/specs/omega-context-management.md §Goals, §4, §Migration Path
- **Summary**: 2026-03-27 首轮实现已把 summary ranking / compaction policy 从旧的倒序贪心裁剪切换为 slot-budget MVP，并在 `Task 11C` 后下沉到 `omega-memory` / `omega-context` 主路径；2026-03-30 进一步补齐 `omega-context::DefaultContextAssembler` 的高层回归，显式锁定预算紧张时优先保留 step input summary、以及 summary backlog 触发后的低优先级历史 compact 行为。验证已通过 `cargo test -p omega-context --color never`。

### Task 11C: omega-memory + omega-context facade
- **Status**: Completed
- **Completed**: 2026-03-27
- **Priority**: High
- **Complexity**: High
- **Description**: 在 `Task 11B` 行为验证后，抽离 `omega-memory` 与 `omega-context`。公开边界采用 `OmegaContextFacade` + 聚焦接口（ContextAssembler/MemoryService/KnowledgeQueryService/DocumentGovernanceService/ContextDiagnosticsProvider），避免单一 god trait；tool 注册保留在 omega-context integration adapter，而不是 facade 核心接口。
- **Blocks**: Task 11D, Task 11E, Task 11F
- **Related**: docs/specs/omega-context-management.md §Architecture, §Migration Path
- **Summary**: 已新增 `omega-memory` 与 `omega-context` 两个 crate：`omega-memory` 负责 summary ranking / compaction policy，`omega-context` 负责 `OmegaContextFacade`、`ContextAssembler` 与 output repair context 组装；`omega-session::runner` 现通过 facade 构建 step/repair system blocks，不再直接持有 prompt-path budgeting 细节，`render_output_contract` 也已切到共享 context 层实现。验证已通过 `cargo test -p omega-memory --color never`、`cargo test -p omega-context --color never`、`cargo test -p omega-session --lib --color never`。

### Task 11D: omega-document — FileStore + Governance + Keyword Search
- **Status**: Completed
- **Completed**: 2026-03-27
- **Priority**: High
- **Complexity**: High
- **Description**: 新建 `omega-document` 内部 crate，先实现 `FileStore` manifest（`.omega/store/files.jsonl` 真源）、ChunkManager、PersistentTodoStore、tantivy full-text index 和 Document Governance Engine。`manage_document` 采用 check/plan/apply staged 模式，避免隐式多文件写入。通过 omega-context 注册 `search_codebase`（keyword）和 `manage_document`。
- **Blocks**: Task 11E, Task 11F
- **Related**: docs/specs/omega-context-management.md §3.1, §3.5, §3.7
- **Summary**: 已新增 `omega-document` crate，落地 `.omega/store/files.jsonl` manifest 真源、ChunkManager、`PersistentTodoStore`、tantivy keyword index 与 staged `manage_document` 治理流；`omega-context` 现直接依赖 `omega-client` / `omega-tools` / `omega-document` 并提供 `ContextToolRegistry`，把 `search_codebase` 与 `manage_document` 注册到 `omega-core` 默认工具集中，同时避免 `omega-context -> omega-core` 反向依赖循环。验证已通过 `cargo test -p omega-document --color never`、`cargo test -p omega-context --color never`、`cargo test -p omega-workflow --color never`、`cargo test -p omega-core --color never` 与 `cargo test -p omega-session --lib --color never`。

### Task 11E: LanceDB 向量数据库 + 多维复合查询
- **Status**: Completed
- **Completed**: 2026-03-27
- **Priority**: High
- **Complexity**: High
- **Description**: 接入 LanceDB 作为 `FileStore` 的派生向量索引，而不是主存储。实现 revision-aware files/chunks/turns 向量表、fastembed 本地 embedding、structured filters + keyword + vector 的 hybrid retrieval；当向量索引落后于 manifest revision 时自动降级到 keyword 模式。
- **Blocks**: Task 11F
- **Related**: docs/specs/omega-context-management.md §3.2, §3.2.1, §3.3
- **Summary**: `omega-document` 现已新增 `.omega/store/lance/` 派生向量索引与 `.omega/store/index-commit-log.json` revision commit log，使用 LanceDB 维护 `files/chunks/turns` 表，其中 `chunks` 与 `files` 已接入 embedding 列和本地语义检索；`search()` 现支持 `keyword / semantic / hybrid` 三种模式，并在 Lance revision 落后于 manifest 或向量查询失败时自动降级到 keyword。运行时默认使用 fastembed `AllMiniLML6V2` 生成 embedding，测试路径使用 deterministic mock embedding 避免模型下载；`omega-context` 的 `search_codebase` 工具说明与默认 mode 也已同步改为 hybrid。验证已通过 `cargo test -p omega-document --color never`、`cargo test -p omega-context --color never`、`cargo test -p omega-core --color never` 与 `cargo test -p omega-session --lib --color never`。

### Task 11F: Observability + TUI Integration
- **Status**: Completed
- **Completed**: 2026-03-30
- **Priority**: Medium
- **Complexity**: Medium
- **Description**: 实现 `ContextDiagnostics` 聚合指标（budget + cache + memory + document + store），通过 `ContextRuntimeMessage` 向 TUI 推送状态。默认采用后台索引和 readiness 状态，不阻塞启动。TUI 新增：(1) 最小化 budget/caching 调试视图；(2) document health dashboard；(3) search results overlay；(4) compaction/index event feed。关键操作记入 tracing spans。
- **Blocks**: —
- **Related**: docs/specs/omega-context-management.md §Observability, §TUI Integration
- **Summary**: 2026-03-30 已完成 11F 全链路 observability 收口：`omega-tui` diagnostics sidebar/detail overlay 现会把 `CacheDiagnostics` 转成可读的 context budget 使用率、headroom 与 cache hit 信息；`OverlayTarget::Search` 的 runtime 文本内容现会打开专用 search results overlay，不再被 reducer 丢弃，同时保留原有 focused-panel 搜索弹窗行为；`manage_document health_check` 现会触发 document health detail popup，`search_codebase` / `manage_document` 的 scan metadata 会写入 runtime index feed，而 summary 裁剪也会发出 `context.compact` 日志。验证已通过 `cargo test -p omega-session --lib --color never`、`cargo test -p omega-context --color never`、`cargo test -p omega-tui --color never upserting_step_diagnostics_builds_sidebar_lines`、`cargo test -p omega-tui --color never activating_diagnostics_item_opens_detail_overlay`、`cargo test -p omega-tui --color never search_overlay_target_can_show_runtime_results_and_hide`、`cargo test -p omega-tui --color never detail_overlay_target_can_be_shown_and_hidden` 与 `cargo test -p omega-tui --color never`。

### Task 11F-1: Unified `ContextDiagnostics` Snapshot
- **Status**: Completed
- **Completed**: 2026-03-31
- **Priority**: Medium
- **Complexity**: Medium
- **Description**: 将当前空壳 `ContextDiagnostics` 扩展为真实聚合模型，统一承载 budget、cache、memory、document、store 五类指标，并让 `ContextDiagnosticsProvider` 返回可消费的快照，而不是 `ContextCacheDiagnostics` 与 tool metadata 的分散组合。
- **Blocks**: Task 11F-2, Task 11F-3
- **Related**: docs/specs/omega-context-management.md §Observability, crates/omega-context/src/lib.rs
- **Summary**: `omega-context` 现已把 `ContextDiagnostics` 从空壳扩展为真实聚合快照，统一输出 budget/cache/memory/document/store 五类指标；`LocalDiagnostics` 会在 context assembly、workspace scan 与 document health 流程后更新状态，并按需计算 store 尺寸与 TODO/archive 计数。验证已通过 `cargo test -p omega-context --color never`。

### Task 11F-2: Diagnostics Producer Refactor
- **Status**: Completed
- **Completed**: 2026-03-31
- **Priority**: Medium
- **Complexity**: Medium
- **Description**: 在统一 `ContextDiagnostics` 快照落地后，收敛 `omega-session` / `omega-app` 当前直接拼接 overlay 文本与 runtime log 的实现，让 search/document/index/compaction 事件优先复用 facade diagnostics 输出，减少 metadata key 漂移。
- **Blocks**: Task 11F-3
- **Blocked by**: Task 11F-1
- **Related**: docs/specs/omega-context-management.md §Runtime Message Integration, crates/omega-session/src/ui_emit.rs, crates/omega-app/src/runtime_message_policy.rs
- **Summary**: `omega-session::runner` 现会把 unified snapshot 带进 `StepDiagnostics`，tool observability 也会在 search/document/index overlay/log 中优先消费 `ContextDiagnostics`，`omega-app` runtime message policy 已同步适配。验证已通过 `cargo test -p omega-session --lib --color never` 与 `cargo test -p omega-app --color never`。

### Task 11F-3: Context Dashboard Follow-up
- **Status**: Completed
- **Completed**: 2026-03-31
- **Priority**: Medium
- **Complexity**: Medium
- **Description**: 基于统一 diagnostics 快照补齐 TUI 的 memory/store 维度视图与完整 context dashboard，使现有 budget indicator、search overlay、document popup 不再只是点状入口，而是统一 dashboard 的子视图。
- **Blocked by**: Task 11F-1, Task 11F-2
- **Related**: docs/specs/omega-context-management.md §TUI Integration, crates/omega-tui/src/app/diagnostics.rs, crates/omega-tui/src/render/overlay.rs
- **Summary**: `omega-tui` diagnostics sidebar/detail overlay 现已渲染 context memory/document/store 指标，并把预算、cache、memory、document、store 收敛为单一 dashboard 入口。验证已通过 `cargo test -p omega-tui --color never`。

### Task 11F-4: omega-context / omega-session / omega-app / omega-tui — Document And Memory Supervision Panels
- **Status**: Completed
- **Priority**: Medium
- **Complexity**: L
- **Description**: 在现有 `ContextDiagnostics` dashboard 之上，为 document system 与 memory system 增加专门监管面板，稳定展示 enablement/readiness、总量/大小统计，以及当前 active step 命中的 document results 与 memory summary 摘要。
- **Planning Note**: 不建议再新增顶层永久列；更合理的落点是在现有 `Sidebar / Activity` 架构内新增 `Document` 与 `Memory` 专门视图，并通过 additive-only `ContextSupervisionSnapshot` state message 提供 typed `current hits` 数据。实现时需要补 `turn_archive_size_bytes` 与 selected summary preview，否则 memory 监管仍只能停留在 count 级别。
- **Blocked by**: Task 11F-1, Task 11F-2, Task 11F-3, Task 15B-16
- **Blocks**: Task 15B-12
- **Related**: docs/specs/omega-tui-document-memory-supervision.md, docs/specs/omega-context-management.md, docs/specs/omega-tui-runtime-experience.md
- **Summary**: `omega-context` 已补 `ContextSupervisionSnapshot`、`turn_archive_size_bytes` 与 typed document/memory hit summaries；`omega-session` 已把 selected summary preview 与 `ContextSupervision` state message 接入 runtime；`omega-tui` sidebar 现提供独立 `Document` / `Memory` 监管面板并支持 detail overlay；`omega-app` runtime policy 已转发该状态。验证已通过 `cargo test -p omega-tui --color never` 与 `cargo test -p omega-app --color never`。

### ── M7: 任务系统 (s07) ──

> 验证方式：`cargo run` → Agent 创建/查询/更新持久化任务
> 对标：learn-claude-code s07_task_system.py

### Task 4: omega-tasks — TaskManager
- **Status**: Pending
- **Priority**: Medium
- **Description**: 实现 TaskManager 持久化任务系统，支持 CRUD 操作，注册为 tool
- **Related**: docs/specs/omega-agent-impl-plan.md, docs/specs/omega-tui-runtime-experience.md

### ── M8: 后台任务 (s08) ──

> 验证方式：`cargo run` → Agent 在后台执行长时间任务，不阻塞主循环
> 对标：learn-claude-code s08_background_tasks.py

### Task 12: omega-background — BackgroundManager
- **Status**: Pending
- **Priority**: Medium
- **Description**: 实现 BackgroundManager 后台任务管理，支持 spawn/check/collect
- **Related**: docs/specs/omega-agent-impl-plan.md, docs/specs/omega-tui-runtime-experience.md

### ── M9: 团队协作 (s09-s11) ──

> 验证方式：`cargo run` → 多个 Agent 通过消息总线协作完成任务
> 对标：learn-claude-code s09-s11

### Task 3: omega-message — 消息系统
- **Status**: Pending
- **Priority**: Medium
- **Description**: 实现 MessageBus 消息总线，支持 send/read_inbox/broadcast
- **Related**: docs/specs/omega-agent-impl-plan.md, docs/specs/omega-tui-runtime-experience.md

### Task 13: omega-team — 团队管理
- **Status**: Pending
- **Priority**: Medium
- **Description**: 实现 TeammateManager 团队管理和自治智能体
- **Related**: docs/specs/omega-agent-impl-plan.md, docs/specs/omega-tui-runtime-experience.md

### ── M10: Worktree 隔离 (s12) ──

> 验证方式：`cargo run` → Agent 在独立 worktree 中执行任务，互不干扰
> 对标：learn-claude-code s12_worktree_task_isolation.py

### Task 6: omega-worktree — WorktreeManager
- **Status**: Pending
- **Priority**: Low
- **Description**: 实现 WorktreeManager 管理 git worktree
- **Related**: docs/specs/omega-agent-impl-plan.md, docs/specs/omega-tui-runtime-experience.md

### ── M11: 完整 TUI (高级功能) ──

> 验证方式：`cargo run` → 完整 ratatui 终端界面，支持 Markdown 渲染、代码高亮、输入历史等
> 对标：生产级交互体验
> 前置：M1.7 (TUI 基础美化) 已完成

_基础体验已在 M1.7 完成，此处保留高级特性。_

### Task 15C: 交互层重构 — omega-tui 库化与 UI 边界收敛
- **Status**: Completed
- **Completed**: 2026-03-20
- **Priority**: High
- **Description**: 在继续高阶 TUI 能力前，先将 `omega-tui` 拆为 library-first crate，并完成 UI-only 边界收敛，为后续把应用入口迁往独立 `omega-app` 做准备
- **Related**: docs/archive/omega-interaction-layer-refactor.md, docs/specs/omega-runtime-ui-message-contract.md, docs/specs/omega-app-package.md

### Task 15C-2: omega-app — 单一应用装配包与 main 迁移
- **Status**: Completed
- **Completed**: 2026-03-20
- **Priority**: High
- **Description**: 新增 `omega-app` crate 作为唯一 `main` 入口与顶层装配层，负责 provider/config/bootstrap、tracing、session/runtime bridge 与 `omega-tui` UI runtime 的组装；`omega-tui` 不再保留应用入口
- **Complexity**: M
- **Related**: docs/specs/omega-app-package.md, docs/specs/omega-runtime-ui-message-contract.md, docs/specs/omega-agent-impl-plan.md, docs/decisions/006-omega-tui-ui-boundary.md
- **Blocks**: Task 15B-18, Task 15B-8, Task 15B-9, Task 15B-10, Task 15B-11, Task 15B-12
- **Summary**: 已新增 `crates/omega-app` 作为唯一应用入口 crate，将原 `omega-tui/src/main.rs` 的 provider/config/bootstrap、trace channel 初始化与 `TuiLaunchConfig` 装配迁入 `omega-app`；`omega-tui` 已删除 binary target，仅保留 UI 库与运行时壳层。默认运行命令现为 `cargo run -p omega-app`

### Task 15C-3: omega-app — 启动加载 `.omega/env.toml` 并注入环境变量
- **Status**: Completed
- **Completed**: 2026-03-26
- **Priority**: Medium
- **Description**: 在 `omega-app` 启动期新增 `.omega/env.toml` 加载入口，将仓库/工作目录级环境变量在 provider/client、observability 与 session runtime bootstrap 前注入到进程环境中，避免 `from_env` 类配置继续分散依赖外部 shell 手工导出
- **Complexity**: M
- **Related**: docs/specs/omega-app-package.md, docs/decisions/006-omega-tui-ui-boundary.md
- **Summary**: `omega-app` 已新增独立 `env_config` 模块，在启动早期统一创建/加载 `.omega/env.toml`，并在 tracing、provider client 与 session bootstrap 前把其中声明的环境变量注入到进程环境；缺失文件会自动写入默认模板，非法配置会回退为“无 env override”并产生日志/启动告警，且现有 shell/process 环境变量优先级高于 `env.toml`，避免仓库级默认值意外覆盖显式外部配置。验证已通过 `cargo test -p omega-app`。

### Task 15D: `omega-tui` 非 UI 职责剥离
- **Status**: Completed
- **Completed**: 2026-03-19
- **Priority**: High
- **Description**: 将 `omega-tui` 中不属于 UI 的 turn orchestration 与 observability 逻辑拆为独立 crate；该任务先于剩余主线执行，Phase 1 先实现 `omega-session` 与 `omega-observability`，Phase 2 再按需要评估 `omega-interaction`
- **Related**: docs/specs/omega-app-package.md, docs/specs/omega-runtime-ui-message-contract.md, docs/decisions/006-omega-tui-ui-boundary.md
- **Summary**: 新增 `omega-session` 承接 Agent turn orchestration 与 checkpoint/interrupt/update 协议，新增 `omega-observability` 承接 tracing 初始化、UI sink、JSONL 文件日志与 ANSI 清洗；`omega-tui` 已收敛为 UI 运行时并消费外部 `SessionUpdate`/trace channel，应用入口与顶层装配已迁到 `omega-app`；当前验证基线仍以 `cargo test -p omega-app -p omega-session -p omega-observability -p omega-tui` 为准

### Task 15E-1: omega-theme — 主题与样式令牌包
- **Status**: Completed
- **Completed**: 2026-03-19
- **Priority**: Low
- **Description**: 新增 `omega-theme` crate，集中管理 `omega-tui` 的颜色、边框、状态语义色、间距与后续命名主题，并支持通过 `.omega/theme.toml` 加载用户主题覆盖，避免样式令牌继续散落在 `render.rs` 与未来多个 widget 模块中
- **Complexity**: M
- **Related**: docs/specs/omega-theme-package.md, docs/specs/omega-tui-runtime-experience.md, docs/specs/omega-tui-input-status-layout.md
- **Blocked by**: Task 15D
- **Blocks**: Task 15B-8, Task 15B-9, Task 15B-12
- **Summary**: 新增 `omega-theme` crate，提供内建 `dark` 主题、语义/组件级主题结构、`.omega/theme.toml` 默认模板写入、TOML 解析与覆盖式合并、非法配置告警与安全回退；`omega-tui` 启动时现与 keymap 一样加载主题，并将所有主要面板、输入框、状态带、消息样式和 overlay 视觉令牌统一改为消费 `omega-theme`，为后续 Markdown/高亮/统计类视觉扩展提供集中入口

### Task 15F-1: omega-workflow — 可配置四阶段工作流系统
- **Status**: Completed
- **Completed**: 2026-03-19
- **Priority**: Medium
- **Description**: 新增独立 `omega-workflow` crate，支持通过 `.omega/workflow.toml` 配置单轮执行的 `explore -> plan -> execute -> report` 四阶段工作流，并由 `omega-session` 推送结构化阶段更新，让 `omega-tui` 在底部状态栏显示当前执行阶段
- **Complexity**: M
- **Related**: docs/specs/omega-workflow-package.md, docs/specs/omega-tui-runtime-experience.md, docs/specs/omega-tui-input-status-layout.md, docs/specs/omega-agent-impl-plan.md
- **Summary**: 新增 `omega-workflow` crate，复用 `.omega/*.toml` 约定提供默认 `workflow.toml` 生成、解析、校验与回退，并将四阶段提示词外置到 `.omega/prompt/step/explore.md|plan.md|execute.md|report.md`；`omega-session` 现按真实阶段顺序驱动无工具探索、无工具计划、工具执行与最终报告，而不再在线程启动后直接跳到 `execute`；`omega-tui` 继续消费结构化 `WorkflowStepChanged` 更新，在底部状态带显示当前 `flow <label> <index>/<total>`；相关单测已补齐
- **Blocked by**: Task 15D

### Task 15F-2A: omega-session — 会话资产管理基础
- **Status**: Completed
- **Completed**: 2026-03-20
- **Priority**: Medium
- **Description**: 在 `omega-session` 内建立 `SessionToolCatalog`（独立结构体，纯 resolve 方法，`Arc` 友好）和 `SessionSkillCatalog`；在 `omega-core::Agent` 补齐 `set_visible_tools` + `ToolDispatcher::to_schemas_filtered` 动态工具切换 API；`AgentSession` 通过组合持有 catalog，避免 God Object 膨胀
- **Complexity**: L
- **Related**: docs/specs/omega-step-session-asset-model.md, docs/specs/omega-agent-impl-plan.md
- **Summary**: 已新增 `crates/omega-session/src/tool_catalog.rs` 与 `crates/omega-session/src/skill_catalog.rs`，分别承接 tools/skills 的独立 resolve 逻辑；`omega-core::Agent` 已支持 `set_visible_tools` 动态切换模型可见工具，`omega-tools::ToolDispatcher` 已支持 `to_schemas_filtered` 生成稳定排序的工具子集 schema；当前 `AgentSession` 已通过组合持有两个 catalog，并让现有固定四阶段 runner 经由 catalog 和 visible-tools API 执行，从而在不改变现有四阶段行为的前提下完成 15F-2B 的前置资产层；`cargo test -p omega-tools -p omega-core -p omega-session -p omega-skills` 与对应 `clippy -D warnings` 已通过

### Task 15F-2B: omega-workflow — 通用 Step 编排接入会话资产层
- **Status**: Completed
- **Completed**: 2026-03-20
- **Priority**: Medium
- **Description**: 分步实现：(1) `WorkflowStepKind` enum → `id: String` 内部模型泛化（保持 4 canonical id 配置兼容），`WorkflowPrompts` 改为 `HashMap`；(2) step 定义增加 `StepLoopMode` + `StepToolRequest` + `StepSkillRequest`；(3) `omega-session` 改为通用 step runner，采用 `WorkflowRun` 状态机替代直接 iterator；(4) 事件协议增加 `step_id` 字段与 `step_label` 分离；`context` 本轮仅保留不实现
- **Complexity**: L
- **Related**: docs/specs/omega-step-session-asset-model.md, docs/specs/omega-workflow-package.md, docs/specs/omega-agent-impl-plan.md
- **Summary**: `omega-workflow` 已完成 string-keyed step 内部模型改造，并把 `prompt_path`、`StepLoopMode`、`StepToolRequest`、`StepSkillRequest` 收敛到通用 `WorkflowStep` 定义；`omega-session` 已使用 `WorkflowRun` 驱动 step 编排，按 `loop_mode` 决定无工具单响应或工具循环，并通过 session catalogs 解析每个 step 的 tools/skills；`SessionUpdate::WorkflowStepChanged` 现包含稳定 `step_id`，`omega-tui` 已适配；`cargo test -p omega-workflow -p omega-session -p omega-tui -p omega-core -p omega-skills` 与对应 `clippy -D warnings` 已通过

### Task 15F-3: omega-session — 统一 runtime UI 消息与效果协议
- **Status**: Completed
- **Completed**: 2026-03-20
- **Priority**: High
- **Description**: 为 workflow 与后续 runtime-visible 模块建立统一的 runtime UI message/effect contract，替代继续在 `SessionUpdate` 中按 feature 堆特例；同时定义 session-owned bridge / runtime context 边界，明确哪些输出走 UI 协议，哪些交互继续走 domain event / API
- **Complexity**: L
- **Related**: docs/specs/omega-runtime-ui-message-contract.md, docs/specs/omega-tui-runtime-experience.md, docs/specs/omega-step-session-asset-model.md, docs/specs/omega-agent-impl-plan.md
- **Summary**: `omega-session` 已新增 `RuntimeUiEnvelope`、`RuntimeUiMessage`、`RuntimeUiEffect` 及 `UiTarget/UiSource/UiMessageKind` 等 typed contract，并补齐 `RuntimeUiBridge` / `RuntimeUiSink` trait 与 `SessionRuntimeContext` 边界；workflow step phase、step text、tool preview、todo snapshot、assistant reply 与 turn finish 已全部从 `SessionUpdate` 一步迁移到统一 envelope 通道，`omega-tui` 现直接消费该协议；`cargo test -p omega-session -p omega-tui -p omega-app` 通过

### Task 15F-4: omega-workflow — scene catalog 与 workflow routing 模型
- **Status**: Completed
- **Completed**: 2026-03-20
- **Priority**: High
- **Description**: 在现有 `omega-workflow` 之上新增 `scene` 与 named workflow catalog，定义 `root` / `chat` / `feature` 预设，并让系统先通过 `scene-recognition`、`select-workflow` 两个 step 决定进入哪条 child workflow
- **Complexity**: L
- **Related**: docs/specs/omega-scene-routing.md, docs/specs/omega-workflow-package.md, docs/specs/omega-agent-impl-plan.md
- **Blocks**: Task 15F-5, Task 15B-19
- **Planning Note**: 推荐配置方向为 `.omega/scenes.toml` + `.omega/workflows/*.toml`；旧 `.omega/workflow.toml` 在迁移期继续作为 `feature` workflow 的兼容来源
- **Summary**: `omega-workflow` 已新增 `SceneCatalog`、`WorkflowCatalog` 与 `WorkflowPromptCatalog`，内建 `root/chat/feature` workflow preset 和 `chat/feature` scene preset；默认配置已切换到 `.omega/scenes.toml` + `.omega/workflows/*.toml`，同时保留旧 `.omega/workflow.toml` 作为 `feature` workflow 的兼容来源。`WorkflowDefinition::load()` 继续向现有调用方暴露 `feature` workflow，因此 `omega-app`/`omega-session` 可在不改调用面的前提下平滑进入下一阶段。验证已通过 `cargo test -p omega-workflow`。

### Task 15F-5: omega-session — scene recognition 与 child workflow delegation
- **Status**: Completed
- **Completed**: 2026-03-20
- **Priority**: High
- **Description**: 让 `omega-session` 执行 `scene-recognition -> select-workflow` 主 workflow，并把选中的 child workflow 作为 turn 的稳定执行流；同时为 future step-level subworkflow delegation 预留 workflow stack 与通用 transition 语义
- **Complexity**: L
- **Related**: docs/specs/omega-scene-routing.md, docs/specs/omega-step-session-asset-model.md, docs/specs/omega-runtime-ui-message-contract.md, docs/specs/omega-agent-impl-plan.md
- **Blocked by**: Task 15F-4
- **Blocks**: Task 15B-19
- **Summary**: `omega-session` 已切换为 scene-aware orchestration：turn 先执行 `root` workflow 中的 `scene-recognition -> select-workflow`，再按 `SceneCatalog` 映射委派到 child workflow。root routing step 的内部输出会在 session 内消费并从 agent message history 回滚，避免污染 child workflow 对话上下文；runtime UI 现已通过 `StatusSlot::Session` 和 Activity 日志发出当前 scene、selected workflow 以及 root/child workflow 切换结果。后续又补上三类稳定性修正：当 root routing step 在 JSON contract 下多次输出自然语言时，session 会退回既有 text/default routing fallback 而不是直接终止整轮；chat scene 现在显式承接“代码库说明 / 测试评估 / 优缺点分析”这类只读仓库分析请求，并放开安全只读 bash，避免把这类问题误送进 feature workflow；另外对于明显要求修改代码/文档/配置/测试的实现类请求，若模型仍把 `scene-recognition` 或 `select-workflow` 判成 `chat`，runtime 会提升回 `feature` 路由，且未识别 scene 的默认回退仍保持 `feature` 而不是 `chat`。`omega-app` 也已改为装配 `LoadedWorkflowCatalog`，不再只向 session 传递单个 workflow。验证已通过 `cargo test -p omega-session -p omega-app -p omega-workflow` 与 `cargo clippy -p omega-session -p omega-app -p omega-workflow --all-targets -- -D warnings`。
	2026-03-23 补充修正：root routing JSON 校验现在会从“短说明文字 + 顶层 JSON”响应中提取结构化结果，并在 step system prompt 的 output contract 中统一注入 JSON-only response rules，减少 `scene-recognition` / `select-workflow` 首轮被误记为 invalid structured output 的噪音；验证已补 `cargo test -p omega-session` 与 `cargo test -p omega-app`。
	2026-03-24 补充修正：scene ambiguity policy 已明确为“只有明确只读请求才走 `chat`；未识别时仍回退到 `feature`；实现类请求若被误判为 `chat` 会在 runtime 中提升回 `feature`”。
	2026-03-24 继续扩展：scene catalog 已新增 `research` scene，默认映射到只读 `explore -> plan -> execute -> report` 的 `research` workflow，用于承接深度复杂的综合分析和探索任务；scene recognition / workflow selection prompt 与 runtime promotion 规则也已同步对齐。
	2026-03-25 补充修正：repo-local `.omega/workflows/{research,feature}.toml` 与 `.omega/prompt/step/{explore,plan,execute,report}.md` 已同步到当前 structured contract 版本，并将首阶段从 `analysis` 重命名为 `explore`，补充 `explore.json` 的 `key_findings[]` 字段，用于在 `plan` 前先探索项目、提取关键上下文并提升后续规划质量。

### Task 15B-18: omega-tui — 统一 runtime UI sink / reducer
- **Status**: Completed
- **Completed**: 2026-03-20
- **Priority**: High
- **Description**: 让 `omega-tui` 作为统一 runtime UI 协议的 consumer/sink，按 `target` / `kind` / `source` 路由上游消息到 `Response`、`Activity`、`Todo`、`StatusBar` 与 `Overlay`，为 workflow Response 输出体验和未来多样式扩展建立稳定 reducer 架构
- **Complexity**: M
- **Related**: docs/specs/omega-runtime-ui-message-contract.md, docs/specs/omega-tui-runtime-experience.md, docs/specs/omega-agent-impl-plan.md
- **Planning Note**: 先只收敛当前已接通的 richer shell 主路径（`omega-app -> omega-tui + omega-session -> omega-core`）；对 `subagent/background/message/team/worktree` 仅保留 target/source 语义，不因 Cargo 依赖已存在而提前做 feature-specific reducer 分支
- **Summary**: `omega-tui` 已新增 `TuiUpdateReducer`，把 `RuntimeUiEnvelope` 按 `target / kind / source` 统一路由到 `Response`、`Activity(Log)`、`Todo`、`StatusBar` 与 `Overlay`；`App` 中原先分散的 envelope 处理已收敛为 reducer + 通用 status/overlay helper，底部状态栏也已支持额外 `Session` slot，为后续按 `source` / `kind` 扩展 rendering preset、step block 与 markdown-aware variant 留出稳定入口。验证已通过 `cargo test -p omega-tui -p omega-session -p omega-app` 与 `cargo clippy -p omega-tui --all-targets -- -D warnings`。

### Task 15B-19: omega-tui — scene / workflow routing 可见性
- **Status**: Completed
- **Completed**: 2026-03-20
- **Priority**: High
- **Description**: 在已落地的 `RuntimeUiEnvelope` + `TuiUpdateReducer` 基础上，为当前 scene、selected workflow 以及 root/child workflow 切换结果提供稳定的底部状态带与 Activity 可见性
- **Complexity**: M
- **Related**: docs/specs/omega-scene-routing.md, docs/specs/omega-runtime-ui-message-contract.md, docs/specs/omega-tui-runtime-experience.md, docs/specs/omega-agent-impl-plan.md
- **Summary**: `RuntimeUiEnvelope` 现已补齐结构化 routing 可见性：`omega-session` 通过 `StatusValue::SessionRouting` 发出当前 scene、selected workflow 与 active root/child workflow，workflow step status 与 Activity summary 也已携带 `workflow_id + workflow_role` 元数据；`omega-tui` 底部状态带现稳定显示 `route` 与 `flow` 摘要，Activity/Logs 现能清晰区分 `[route] ...`、`[root:root ...]` 与 `[child:feature ...]` 这类运行轨迹，同时在新 turn 开始时清空上一轮 route，避免残留旧 routing 状态。验证已通过 `cargo test -p omega-session -p omega-tui -p omega-app` 与 `cargo clippy -p omega-session -p omega-tui -p omega-app --all-targets -- -D warnings`。

### Task 15F-6: omega-client / omega-core / omega-session — 流式 response / thinking runtime contract
- **Status**: Completed
- **Completed**: 2026-03-20
- **Priority**: High
- **Description**: 为 provider-exposed text / thinking 建立流式 client event API，并在 `omega-session` 内把这些事件归一为带 section identity、append/finalize 语义的 runtime UI contract，作为 Response timeline 与实时 thinking 可见性的前置路径
- **Complexity**: L
- **Related**: docs/specs/omega-tui-response-thinking-experience.md, docs/specs/omega-runtime-ui-message-contract.md, docs/specs/omega-agent-impl-plan.md
- **Blocks**: Task 15B-20, Task 15B-21
- **Summary**: `omega-client` 新增 typed `ChatEvent` / `chat_stream()` API，并通过默认兼容实现把现有同步 `chat -> ChatResponse` 回放为 one-shot event stream；`omega-core::Agent` 新增 stream-aware response 回调路径，在保留原有 `run_single_response()` / `run_loop_with()` API 的同时支持 session 逐事件消费 text/thinking/tool-use 完成态；`omega-session` 的 runtime UI contract 现新增 `BeginResponseSection` / `AppendResponseSection` / `CompleteResponseSection` 与 `ResponseSectionKind::{Routing, Step, FinalAnswer, Thinking}`，每个 workflow step 都能发出稳定 section id、元数据与 append/finalize 事件，为后续 `Task 15B-20` / `Task 15B-21` 提供已验证的输入面。验证已通过 `cargo test -p omega-client -p omega-core -p omega-session` 与 `cargo clippy -p omega-client -p omega-core -p omega-session --all-targets -- -D warnings`。

### Task 15B-20: omega-tui — 结构化 Agent Response timeline
- **Status**: Completed
- **Completed**: 2026-03-20
- **Priority**: High
- **Description**: 将当前 `Agent Response` 从平铺文本列表升级为按 scene/workflow/step 组织的 turn timeline，清晰区分 root routing、child workflow step、最终回答与后续 thinking block 的落点
- **Complexity**: L
- **Related**: docs/specs/omega-tui-response-thinking-experience.md, docs/specs/omega-tui-runtime-experience.md, docs/specs/omega-agent-impl-plan.md
- **Blocks**: Task 15B-21
- **Summary**: `omega-tui` 已把 `Response` 从旧的平铺文本列表升级为基于 response section effect 的结构化 timeline：`App` 现在持有带 section 元数据的逻辑响应项，`TuiUpdateReducer` 直接消费 `Begin/Append/CompleteResponseSection`，并停止重复写入 legacy workflow/assistant response message；`render.rs` 则把 `Routing / Step / FinalAnswer` 渲染为稳定的 block-like timeline 行，默认压缩 root routing 为摘要，同时保留窄终端可读性和现有搜索/滚动行为。验证已通过 `cargo test -p omega-tui -p omega-session -p omega-app` 与 `cargo clippy -p omega-tui -p omega-session -p omega-app --all-targets -- -D warnings`。

### Task 15B-21: omega-tui — provider-exposed thinking 实时展示
- **Status**: Completed
- **Completed**: 2026-03-20
- **Priority**: High
- **Description**: 为 provider 明确返回的 thinking / reasoning 内容提供实时、可折叠、与最终回答分离的 Response 可见性，并补齐完成态摘要、默认折叠和配置开关
- **Complexity**: M
- **Related**: docs/specs/omega-tui-response-thinking-experience.md, docs/specs/omega-runtime-ui-message-contract.md, docs/specs/omega-tui-runtime-experience.md, docs/specs/omega-agent-impl-plan.md
- **Summary**: `omega-tui` 现已消费 `ResponseSectionKind::Thinking` 并将其接入 `Response` timeline：thinking delta 在流式阶段实时追加、在完成或失败后默认折叠为摘要，并可在 `Response` 焦点下通过 `Enter` / `x` 展开或重新折叠；新增 `.omega/tui.toml` 的 `[response].show_thinking` 开关用于关闭该可见性。实现同时覆盖了 `omega-app` 启动配置装配、`App` 内部 thinking section 状态、低噪音渲染样式与交互测试；验证已通过 `cargo test -p omega-tui -p omega-app -p omega-session` 与 `cargo clippy -p omega-tui -p omega-app -p omega-session --all-targets -- -D warnings`。

### Task 15F-7: omega-session — 结构化 tool-run runtime contract 与 provider markup 清洗
- **Status**: Completed
- **Completed**: 2026-03-20
- **Priority**: Medium
- **Description**: 为 step 内工具调用建立稳定的 typed runtime contract，并在 session 层清洗已知 provider 原始 tool-call markup，避免 `<minimax:tool_call>` 之类文本继续泄漏到 Response / Thinking
- **Complexity**: M
- **Related**: docs/specs/omega-tui-step-tool-thinking-refinement.md, docs/specs/omega-runtime-ui-message-contract.md, docs/specs/omega-agent-impl-plan.md
- **Summary**: `omega-session` 现已新增结构化 `ToolRun` lifecycle contract：`RuntimeUiEffect` 支持 `BeginToolRun / UpdateToolRun / CompleteToolRun`，并以 stable `tool_use_id`、step 归属 `parent_section_id`、status、invocation preview、result preview 与 detail lines 描述每次工具运行；`omega-core` 的工具执行回调也已透传 `tool_use_id` 以保证 session 不再依赖字符串猜测。与此同时，step / thinking 的流式 response 入口已加入已知 provider tool markup 清洗，`<minimax:tool_call>` / `<invoke ...>` 这类包装不再继续污染主阅读区；兼容性的 `[tool] ...` Activity 日志仍然保留。验证已通过 `cargo test -p omega-core -p omega-session -p omega-tui -p omega-app` 与 `cargo clippy -p omega-core -p omega-session -p omega-tui -p omega-app --all-targets -- -D warnings`。

### Task 15F-8: omega-session / omega-workflow / omega-core — 全 step 有界最小 agent loop
- **Status**: Completed
- **Completed**: 2026-03-20
- **Priority**: High
- **Description**: 把 root / chat / feature 内建 step 全部切换到统一的 bounded agent loop，让每个 step 都允许工具调用，只通过工具子集、loop budget 与 step prompt 控制行为，而不再维护 `SingleResponse` 与 `ToolLoop` 双轨模型
- **Complexity**: L
- **Related**: docs/specs/omega-step-session-asset-model.md, docs/specs/omega-agent-impl-plan.md, docs/specs/omega-scene-routing.md
- **Blocks**: Task 15F-9, Task 10
- **Summary**: `omega-session` 已移除 step runner 对 `SingleResponse` / `ToolLoop` 的运行时分叉，所有 root / chat / feature step 现统一通过 bounded agent loop 执行，并在每个 step 上显式设置 `visible_tools + max_iterations`。`omega-workflow` 现将 step loop 语义收敛为 `agent_loop`，新增 `max_iterations`，并把 legacy `single_response` / `tool_loop` 仅保留为兼容解析别名；内建 step 默认策略也已收敛为“execute 继承全工具，其余 step 默认屏蔽写入类工具，只保留只读/技能加载能力”。同时，仓库内 `.omega/workflows/*.toml` 与相关 step prompt 也已同步更新，避免本地配置继续向模型发出旧的 no-tools 语义。验证已通过 `cargo test -p omega-workflow -p omega-session`、`cargo test -p omega-app -p omega-tui` 与 `cargo clippy -p omega-workflow -p omega-session -p omega-app -p omega-tui --all-targets -- -D warnings`。
	2026-03-24 补充修正：为避免 `research` / `feature` / `chat` 的非 root step 在重型仓库分析或多次工具往返时频繁触发 `agent loop exceeded 8 iterations`，非 root workflow step 的默认 `max_iterations` 与 repo-local `.omega/workflows/{chat,research,feature}.toml` 现已统一提升到 200；root routing 仍保持 2 次上限，不放宽 scene 识别预算。

### Task 15F-9: omega-session / omega-workflow — 结构化 step context 与 root-child 生命周期收敛
- **Status**: Completed
- **Completed**: 2026-03-23
- **Priority**: High
- **Description**: 在 `omega-session` 中建立 session-owned `StepSummary` / `SessionContext` / `StepTransition`，让下一个 step 基于 session 资源、之前任务总结、routing state 与当前 step prompt 组装输入；同时明确 session 持续存在、每个用户 turn 先跑 root workflow，再委派 child workflow 的生命周期语义
- **Complexity**: L
- **Related**: docs/specs/omega-step-session-asset-model.md, docs/specs/omega-scene-routing.md, docs/specs/omega-agent-impl-plan.md
- **Blocks**: Task 10, Task 11
- **Summary**: `omega-session` 现已引入持久化 `SessionContext`，在 session 内保留 `latest_user_turn + RoutingContext + StepSummary` 历史，并在每个 step 开始前基于 `context_window - max_output_tokens - safety_margin` 选择可注入的 summary 子集。`build_step_system_prompt` 已收敛到 `StepExecutionInput` 单一输入，root workflow 的 `scene-recognition` / `select-workflow` 现以 JSON 为主路径产出 typed routing handoff，同时保留 token matching fallback；child workflow delegation 与后续 turn 共享同一个 session context。`.omega/model.toml` 已拆分 `context_window` 与 `max_output_tokens`，root step prompt 也已同步升级到 JSON-only 输出约束。验证已通过 `cargo test -p omega-workflow -p omega-session -p omega-app -p omega-tui` 与 `cargo clippy -p omega-workflow -p omega-session -p omega-app -p omega-tui --all-targets -- -D warnings`。
	2026-03-23 补充修正：root routing step 已进一步收紧为 no-tools JSON routing，并把 `max_iterations` 从 6 降到 2，避免多轮追问时在 `scene-recognition` / `select-workflow` 内误触发仓库探索后耗尽迭代预算。

### Task 15F-10: omega-session / omega-workflow — Step Data Contract 框架
- **Status**: Completed
- **Completed**: 2026-03-23
- **Priority**: High
- **Description**: 为 `WorkflowStep` 引入通用的 `StepInputContract` / `StepOutputContract`，让结构化 I/O 成为 step 级能力，并在 `omega-session` 中完成 JSON 提取、校验重试、structured_input 注入与 step_outputs 持久化
- **Complexity**: L
- **Related**: docs/specs/omega-step-session-asset-model.md, docs/specs/omega-agent-impl-plan.md
- **Blocks**: Task 10, Task 11
- **Summary**: `omega-workflow` 已新增 `StepInputContract` / `StepOutputContract` / `DataFormat` 与对应 TOML 解析、默认值和内建 root/feature workflow contract；`omega-session` 已新增 `SessionContext.step_outputs`、`StepExecutionInput.structured_input`、required output 校验失败重试，以及 `<structured_input>` / `<output_contract>` 自动注入。root routing 现优先消费已校验 structured output，而不是只从 raw text 做弱结构化解析。验证已通过 `cargo test -p omega-workflow -p omega-session`。
	2026-03-24 设计补充：当前 required structured output retry 仍偏 blind retry；后续应在 `omega-session` 增加 `RepairThenRegenerate` 恢复策略，在首个 invalid output 后先做 no-tools 的 JSON repair pass，并把 `validation_error + previous_response_preview + extracted_json_preview + required_contract` 作为 machine-readable repair context 注入，再决定是否 full regenerate。

### Task 15F-11: omega-session / omega-todo / omega-workflow — Feature Workflow Schema 绑定与 Todo 集成
- **Status**: Completed
- **Completed**: 2026-03-23
- **Priority**: High
- **Description**: 为 feature workflow 的 explore / plan / execute 绑定具体 JSON schema，更新默认 prompt / workflow 配置，并把 `plan.tasks`、`execute` 结果与共享 `TodoManager` 正式接通
- **Complexity**: L
- **Related**: docs/specs/omega-step-session-asset-model.md, docs/specs/omega-agent-impl-plan.md
- **Blocks**: Task 10
- **Summary**: 内建 feature workflow 现在会生成 `.omega/schema/step/{explore,plan,execute}.json` 与匹配的默认 prompt / TOML contract；`omega-session` 已按 `schema_path` 做轻量 schema 校验，并对 feature outputs 做业务语义校验。`explore` 结构化输出会提供 `objective + key_findings + constraints + risks + affected_paths`，为后续 `plan` 提供更准确的上游上下文；`plan` 结构化输出会自动映射到共享 `TodoManager`，`execute` 的 structured output 会回写 todo 完成状态，`execute` / `report` prompt 也会自动看到 `<todo_state>`。验证已通过 `cargo test -p omega-workflow -p omega-session -p omega-core` 与 `cargo clippy -p omega-workflow -p omega-session -p omega-core --all-targets -- -D warnings`。
	2026-03-25 补充修正：当 `feature` / `research` 的 `execute` 结构化输出仍保留 open tasks、但暂未让 `todo.rendered` 发生文本 diff 时，session 现在仍会按 open todo 状态继续有限次重复 `execute`，避免 research 在首轮“无进展但未完成”结果后直接落到 `report`。

### Task 15F-12: omega-session / omega-observability / omega-tui — 上下文观测与诊断
- **Status**: Completed
- **Completed**: 2026-03-23
- **Priority**: Medium
- **Description**: 为 `SessionContext` / workflow artifacts 增加 tracing-level snapshot/diff 观测，并为 TUI 提供 context diagnostics 落点，避免只能通过 reasoning 文本猜测上下文变化
- **Complexity**: M
- **Related**: docs/specs/omega-step-session-asset-model.md, docs/specs/omega-runtime-ui-message-contract.md, docs/specs/omega-agent-impl-plan.md
- **Summary**: `omega-session` 现在会在 step input/output diagnostics 之外继续记录 `SessionContext` 写入 diff：结构化 `step_outputs.<step_id>` 与 `todo.rendered` 会按 `added / updated / cleared` 生成 before/after preview，并通过 tracing 记录到 `session context snapshot updated`；同一批 diff 也会随 `RuntimeUiEffect::UpsertStepDiagnostics` 进入 `omega-tui` Diagnostics 侧栏与 detail overlay，用户可以直接看到每个 step 对上下文造成的具体变化，而不必再从 reasoning / log 文本反推。验证包含 `cargo test -p omega-session -p omega-tui -p omega-app` 与 `cargo clippy -p omega-session -p omega-tui -p omega-app --all-targets -- -D warnings`。

### Task 15F-13: omega-session / omega-workflow — Required structured output repair-first recovery
- **Status**: Completed
- **Completed**: 2026-03-24
- **Priority**: High
- **Description**: 为 `Required(Json)` step 引入 `RepairThenRegenerate` 恢复策略，并在 step `output_contract` 中补充声明式 recovery policy：首次 invalid structured output 后先运行 no-tools repair pass，注入 `validation_error + previous_response_preview + extracted_json_preview + required_contract` 作为 machine-readable repair context；仅在 repair 失败后才回退到 full regenerate，并保证未写入合法 `step_outputs.<step_id>` 前绝不 advance 到下一 step。
- **Complexity**: L
- **Related**: docs/specs/omega-step-session-asset-model.md, docs/specs/omega-runtime-ui-message-contract.md, docs/specs/omega-agent-impl-plan.md
- **Blocks**: Task 11
- **Summary**: `omega-workflow` 已为 `Required(Json)` output contract 增加声明式 `recovery_mode`，默认内建 workflow 与 repo-local `.omega/workflows/{root,research,feature}.toml` 已统一显式使用 `repair_then_regenerate`；`omega-session` 现已把 blind retry 拆成 `primary -> repair -> regenerate`，首次 invalid output 会切到 no-tools repair pass、注入 machine-readable `<output_repair>` envelope，repair 失败后才回退到 full regenerate，同时继续保证未写入合法 `step_outputs.<step_id>` 前不会 advance。验证已通过 `cargo test -p omega-workflow -p omega-session` 与 `cargo clippy -p omega-workflow -p omega-session --all-targets -- -D warnings`。

### Task 15F-14: omega-session / omega-observability / omega-tui — Structured output recovery diagnostics
- **Status**: Completed
- **Completed**: 2026-03-24
- **Priority**: High
- **Description**: 为 structured output recovery 增加可观察性：在 tracing、`RuntimeUiEffect::UpsertStepDiagnostics`、Activity 和 TUI Diagnostics 中显式展示 `attempt_kind`（primary / repair / regenerate）、`validation_error`、`previous_response_preview`、`extracted_json_preview` 与 repair/fallback decision，避免用户只能看到笼统的 retry 次数。
- **Complexity**: M
- **Related**: docs/specs/omega-step-session-asset-model.md, docs/specs/omega-runtime-ui-message-contract.md, docs/specs/omega-tui-runtime-experience.md, docs/specs/omega-agent-impl-plan.md
- **Blocked by**: Task 15F-13
- **Summary**: `omega-session` 现已把 structured output recovery 的 `attempt_kind`、`validation_error`、`previous_response_preview`、`extracted_json_preview` 与 `recovery_decision` 统一写入 `RuntimeUiEffect::UpsertStepDiagnostics`，并同步输出到 tracing 与 Activity；`omega-tui` Diagnostics 侧栏会显示当前 attempt 和 next decision，detail overlay 则展开 recovery decision、前一轮响应预览、提取 JSON 预览与 validation error，使用户可以区分当前是在 repair、regenerate 还是 fallback/abort。验证已通过 `cargo test -p omega-session -p omega-tui` 与 `cargo clippy -p omega-session -p omega-tui --all-targets -- -D warnings`。

### Task 15B-22: omega-tui — step 内工具使用可见性
- **Status**: Completed
- **Completed**: 2026-03-20
- **Priority**: Medium
- **Description**: 在 `Response` 的 step block 内增加结构化工具摘要与 drill-down 入口，让用户无需在正文和 Activity 之间反复跳转即可理解当前 step 的工具使用
- **Complexity**: M
- **Related**: docs/specs/omega-tui-step-tool-thinking-refinement.md, docs/specs/omega-tui-runtime-experience.md, docs/specs/omega-agent-impl-plan.md
- **Summary**: `omega-tui` 现已消费 `ToolRun` lifecycle effect，并在 step / final response block 内渲染轻量 `tools` 摘要 lane：每次工具调用会以 `tool name + status + invocation preview + result preview` 的单行摘要出现，状态色会区分 running / failed / done；用户在 `Response` 面板选中该摘要后可直接通过 `Enter/x` 打开 detail overlay 查看完整 detail lines，而无需回到 `Activity` 翻日志。`Activity` 中的 `[tool] ...` 兼容日志仍保留，因而没有打破既有 `Response / Activity` 边界；同时 `Response / Todo / Logs` 面板现支持鼠标拖拽建立文本选区，并改为显式 `y` / `Ctrl+C` 复制，而不是鼠标松手即自动复制；本轮又补齐了 Wayland clipboard backend，使 Linux 下的外部窗口粘贴不再只依赖 X11 剪贴板链路。验证已通过 `cargo test -p omega-tui -p omega-session -p omega-app` 与 `cargo clippy -p omega-tui -p omega-session -p omega-app --all-targets -- -D warnings`。

### Task 15B-23: omega-tui — thinking 可读性强化
- **Status**: Completed
- **Completed**: 2026-03-20
- **Priority**: Medium
- **Description**: 强化 thinking 的视觉对比、streaming / complete 状态表达与 collapsed 摘要信息量，在不压过 final answer 的前提下解决“太弱、看不清”的问题
- **Complexity**: S
- **Related**: docs/specs/omega-tui-step-tool-thinking-refinement.md, docs/specs/omega-tui-response-thinking-experience.md, docs/specs/omega-agent-impl-plan.md
- **Summary**: `omega-tui` 现已把 thinking block 提升为更清晰的 reasoning 呈现：header 改为 state-aware 的 `Reasoning live / Reasoning / Reasoning failed`，expanded body 改为带导轨的 `|` 行形态，collapsed 摘要会携带状态、行数与预览片段；`render.rs` 也为 streaming / done / failed 分别提供了更强的语义色和摘要样式，并新增样式回归测试，避免 thinking 再次退回深色主题下的一片弱灰。验证已通过 `cargo test -p omega-tui -p omega-session -p omega-app` 与 `cargo clippy -p omega-tui -p omega-session -p omega-app --all-targets -- -D warnings`。

### ── M1.8: TUI 消息呈现优化 ──

> 验证方式：`cargo run -p omega-tui` → Markdown 标题/列表/代码块在 Response 面板正确渲染；消息角色一眼可辨；Final Answer 区块视觉突出
> 对标：面向"美观、有序、阅读容易"的渐进增强
> 前置：M1.7 (TUI 基础美化) 已完成
> 规格：docs/specs/omega-tui-message-display-polish.md

### Task 15B-40: omega-tui — Markdown 基础渲染
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: Medium
- **Description**: 在 response 消息文本渲染管线中引入轻量 Markdown 解析层：标题（H1~H3）加粗+颜色层级、列表自动缩进、行内代码反色/背景色、粗体/斜体 modifier、分隔线水平填充。ResponseDisplayLine 扩展为多 span 模型，新增 `omega-tui/src/render/markdown.rs` 模块，RenderPalette 新增 heading/inline_code/hr 主题色
- **Complexity**: L
- **Summary**: `ResponseDisplayLine` 已支持多 span 渲染；`render/markdown.rs` 已落地轻量逐行 Markdown 解析；`layout.rs` 现可在窄终端对 styled spans 正确换行，覆盖标题、列表、行内代码、粗斜体与水平分隔线。
- **Related**: docs/specs/omega-tui-message-display-polish.md
- **Blocks**: Task 15B-41, Task 15B-43, Task 15B-46

### Task 15B-41: omega-tui — 代码块视觉容器
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: Medium
- **Description**: Markdown 代码块使用独立背景色 + 语言标注 + 首尾视觉边界；代码块内不做 Markdown 解析。RenderPalette 新增 code_block_bg/code_lang_fg/code_border_fg
- **Complexity**: M
- **Summary**: fenced code block 现已渲染语言标签、首尾边界与独立背景色，块内文本不再触发行内 Markdown 解析，并补齐前后段间距。
- **Blocked by**: Task 15B-40
- **Related**: docs/specs/omega-tui-message-display-polish.md

### Task 15B-42: omega-tui — 消息角色标识与分段
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: Medium
- **Description**: User/Assistant/System/Tool 消息增加角色前缀符号（▶/◆/⚠/✗），不同源连续消息间自动插入空行分隔。RenderPalette 新增 badge 颜色
- **Complexity**: M
- **Summary**: User/Error 已加入显式 badge 前缀，Assistant/Final Answer 颜色分层已接入新 palette，turn separator 与 section spacing 共同提升不同来源消息的视觉边界。
- **Related**: docs/specs/omega-tui-message-display-polish.md

### Task 15B-43: omega-tui — Final Answer 视觉强化
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: Medium
- **Description**: Final Answer header 使用更亮前景色 + ━ 顶部装饰线，body 使用 │ 竖线边饰引导阅读
- **Complexity**: S
- **Summary**: Final Answer 现已加入顶部装饰线、强调色 header 与 `│` 边饰；其 body 同时复用 Markdown 渲染与 wrapping 路径。
- **Blocked by**: Task 15B-40
- **Related**: docs/specs/omega-tui-message-display-polish.md

### Task 15B-44: omega-tui — Tool Lane 折叠与密度优化
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: Medium
- **Description**: ≥6 个 tool 时默认折叠为单行摘要，可展开完整列表；tool name 列宽对齐；≤3 个始终展开
- **Complexity**: M
- **Summary**: Step/Final Answer tool lane 现已具备独立折叠状态与 `ToggleToolLane` action；≥6 个 tool 默认折叠并可展开/收起，tool name 列统一左对齐。
- **Related**: docs/specs/omega-tui-message-display-polish.md

### Task 15B-45: omega-tui — Thinking 折叠摘要视觉增强
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: Medium
- **Description**: 折叠态用 ▸ + DIM+ITALIC + 专用色，展开态用 ▾，streaming 态用 spinner 脉冲，完成态 body 用较暗色 + │ 竖线前缀
- **Complexity**: S
- **Summary**: Thinking summary 现已使用 `▸` + `DIM|ITALIC` + 专用色；streaming body 复用 spinner frame，完成/失败态 body 统一为 `│` 前缀和更暗正文色。
- **Related**: docs/specs/omega-tui-message-display-polish.md

### Task 15B-46: omega-tui — 段间距与长回复可扫读性
- **Status**: Completed
- **Completed**: 2026-04-01
- **Priority**: Medium
- **Description**: Markdown 段落间自动插入空行、代码块前后保留空行、列表结束后空行、step 间空行标准化
- **Complexity**: S
- **Summary**: Markdown parser 现会折叠连续空行为单一段间距，并在列表结束、代码块前后自动补足可扫读的空行分隔。
- **Blocked by**: Task 15B-40
- **Related**: docs/specs/omega-tui-message-display-polish.md

### Task 15B-8: omega-tui — Markdown 渲染 (原始)
- **Status**: Replaced
- **Priority**: Low
- **Description**: 原始 Markdown 渲染任务，已由 Task 15B-40 替代并细化
- **Related**: docs/specs/omega-agent-spec.md, docs/specs/omega-tui-message-display-polish.md

### Task 15B-9: omega-tui — 代码语法高亮
- **Status**: Pending
- **Priority**: Low
- **Description**: 代码块内按语言做语法高亮（syntect 或 tree-sitter），至少支持 Rust/Python/Shell
- **Related**: docs/specs/omega-agent-spec.md

### Task 15B-10: omega-tui — 输入历史
- **Status**: Pending
- **Priority**: Low
- **Description**: ↑↓键浏览历史输入、持久化到 ~/.omega/history

### Task 15B-11: omega-tui — 面板内搜索
- **Status**: Pending
- **Priority**: Low
- **Description**: 触发浮动搜索窗，在当前聚焦面板内高亮匹配文本并支持 n/N 跳转

### Task 15B-12: omega-tui — 可调面板与会话统计
- **Status**: Pending
- **Priority**: Low
- **Description**: 拖拽或快捷键调整面板宽度比例；状态栏显示 token 使用量、对话轮次
- **Blocked by**: Task 15B-16

### Task 15B-13: omega-tui — Leader / 模态快捷键基础设施
- **Status**: Completed
- **Completed**: 2026-03-19
- **Priority**: Low
- **Description**: 引入统一 leader 键入口，并扩展为类似 Vim 的 `Normal` / `Insert` 模式交互体系；支持仅在特定模式/焦点/可输入上下文下触发的快捷键；默认使用 `leader j k` 在两种模式间切换；快捷键配置从 `.omega/keymap.toml` 启动加载，并新增独立 `omega-keymap` 包负责解析、校验与匹配
- **Related**: docs/specs/omega-agent-spec.md, docs/specs/omega-tui-modal-keymap.md
- **Summary**: 新增独立 `omega-keymap` crate，负责内置默认映射、`.omega/keymap.toml` 加载、绑定校验、leader 序列解析与 `mode/focus/input_capable` 条件匹配；`omega-tui` 新增 `Normal` / `Insert` 模式、leader pending 状态、超时取消与上下文提示栏，并把现有焦点切换、滚动、输入提交、光标编辑与中断/退出快捷键统一改为经 keymap 解析；默认模式切换使用 `leader j k` 在 `Normal` / `Insert` 间来回切换；自由文本输入现在仅在 `Insert` 模式下接受，配置缺失时会自动生成默认 `.omega/keymap.toml`，配置非法时回退到内置默认 keymap，并在 UI/日志中给出提示；`cargo test -p omega-keymap -p omega-tui` 与 `cargo clippy -p omega-keymap -p omega-tui --all-targets -- -D warnings` 通过

### Task 16: 最终整合测试
- **Status**: Pending
- **Priority**: Low
- **Description**: 完整编译、运行测试并做最终联调验证，确认全部 crate 可以作为完整系统协同工作
- **Related**: docs/specs/omega-agent-impl-plan.md

---

## Completed

### Task 1: 工作空间初始化
- **Status**: Completed
- **Completed**: 2026-03-18
- **Description**: 创建 Cargo.toml 工作空间根配置，定义 14 个 crate，含全部 stub 和依赖声明

### Task 2: omega-client — LLM 客户端
- **Status**: Completed
- **Started**: 2026-03-18
- **Completed**: 2026-03-18
- **Summary**: LlmClient trait、MinimaxClient 适配器、完整类型模型、Usage、stop_reason 常量、builder、from_env、29 项测试
- **Follow-up Task**: `Task 2A`
- **Follow-up Spec**: `docs/specs/omega-client-anthropic-api-abstraction.md` 规划将当前 Minimax 直连实现下沉为 Anthropic API 抽象层上的 provider 适配器，并补齐 Messages / Count Tokens / Models / Message Batches / Prompt Caching / Streaming 的独立测试矩阵。

### Task 7: omega-tools — 工具抽象层
- **Status**: Completed
- **Started**: 2026-03-18
- **Completed**: 2026-03-18
- **Description**: 实现 ToolHandler trait、ToolDispatcher 分发器、to_schemas() 生成工具定义
- **Summary**: ToolHandler trait (name/description/input_schema/execute)、ToolDispatcher (register/dispatch/to_schemas/len/is_empty/has_tool/tool_names)、未知工具返回 `Unknown tool: <name>` 与参考实现对齐、schema 按 name 排序保证确定性、增加与 `omega-client::ToolDefinition` 的跨 crate 兼容测试、12 项测试全通过、clippy 零警告
- **Related**: docs/specs/omega-agent-spec.md

### Task 8A: omega-tools-builtin — BashHandler
- **Status**: Completed
- **Started**: 2026-03-18
- **Completed**: 2026-03-18
- **Description**: 实现 BashHandler（shell 命令执行 + 安全过滤 + 超时控制）
- **Summary**: 支持 shell 管道与复合命令、allowlist 命令集合、安全过滤（危险命令、shell expansion、redirection、工作区外路径、symlink 逃逸）、进程组级 timeout 清理、输出 50,000 字符截断、21 项测试全通过、clippy 零警告
	2026-03-23 补充修正：默认 bash allowlist 已扩到 `find` / `grep`，并新增 `.omega/model.toml` 的 `[tools.bash].allowed_commands` 覆盖配置；同时 `find` 的 `-exec/-ok/-delete/-fprint*` 等危险动作仍保持拦截，验证已补 `omega-tools-builtin`、`omega-app` 与 `omega-session` 相关测试。
	2026-03-23 对齐修正：chat workflow 提示词已同步更新为允许简单单行 `find` / `grep` 查询，但继续明确禁止 shell redirection 与 expansion，避免模型继续生成 `2>/dev/null`、`2>&1` 这类会被策略拦截的命令。
- **Related**: learn/learn-claude-code/agents/s01_agent_loop.py

### Task 14: omega-core — 最小 Agent Loop
- **Status**: Completed
- **Started**: 2026-03-18
- **Completed**: 2026-03-18
- **Description**: 实现 Agent struct + run_loop，对标 s01_agent_loop.py
- **Summary**: Agent 拥有 DynLlmClient + ToolDispatcher、run_loop 循环 client.chat →  stop_reason 判断 → 工具分发 → 结果回注、run_loop_with 回调支持进度显示、max_iterations 防护无限循环、create_default_tools 集成 BashHandler、re-export 下游构造类型、12 项测试全通过、clippy 零警告
- **Related**: docs/specs/omega-agent-spec.md

### Task 15A: omega-tui — 最小 REPL
- **Status**: Completed
- **Started**: 2026-03-18
- **Completed**: 2026-03-18
- **Description**: stdin/stdout REPL 入口，从环境变量读取配置构造 Agent，可 cargo run 交互
- **Summary**: MinimaxConfig::from_env 构造客户端、create_default_tools 注册工具集、run_loop_with 回调显示工具调用（命令预览 + 输出预览）、UTF-8 安全截断、EOF/q/exit 退出；该最小 REPL 里程碑后来一度被拆到独立入口，但当前已结束双路径策略，用户入口统一收敛为 `omega-tui`
- **Related**: learn/learn-claude-code/agents/s01_agent_loop.py

### 非计划项：文档体系建设
- **Status**: Completed
- **Completed**: 2026-03-18
- **Summary**: 完成 docs 体系初始化，包含根索引、技术规格、实现计划、4 个 ADR、开发指南及 TODO 跟踪
