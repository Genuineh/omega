# TODO

## Current Priorities

_按当前仓库真实主路径重排。判断依据：`cargo test` 全工作区通过；s02 文件工具与 s03 todo 管理已完成；`Task 15C`、`Task 15C-2`、`Task 15D`、`Task 15F-3`、`Task 15F-4`、`Task 15F-5`、`Task 15B-18`、`Task 15B-19`、`Task 15F-6`、`Task 15B-20`、`Task 15B-21`、`Task 15F-7`、`Task 15F-8`、`Task 15F-9`、`Task 15F-10`、`Task 15F-11`、`Task 15F-12`、`Task 15F-13`、`Task 15F-14`、`Task 15B-22`、`Task 15B-23`、`Task 8C`、`Task 8D`、`Task 8E`、`Task 8F`、`Task 8G` 与 `Task 8H` 已完成，交互层当前主边界已落到 `omega-app -> omega-tui + omega-session + omega-observability`，并已用 `RuntimeUiEnvelope` + `TuiUpdateReducer` 收敛 workflow/tool/todo/runtime 状态通道；scene-aware 主路径现在不仅能在底部状态带与 Activity 中稳定显示 routing，也已经在 `Agent Response` 中按 `route / step / final / thinking` 渲染 turn timeline，其中 provider-exposed thinking 支持实时追加、完成态默认折叠、Response 面板内 `Enter/x` 展开与 `.omega/tui.toml` 开关控制，step block 已消费 `ToolRun` lifecycle 渲染结构化工具摘要并支持 detail overlay drill-down，thinking block 也已补齐更清晰的 reasoning 标识、collapsed 摘要和 streaming / done / failed 视觉区分；本轮又补齐了 step input/output contract 诊断、`SessionContext.step_outputs` / todo snapshot diff 观测，以及 TUI Diagnostics drill-down 对 before/after context write 的呈现，并把 structured output recovery 的 repair / regenerate / fallback 运行态显式暴露到 tracing、Activity 与 Diagnostics 侧栏和 detail overlay 中。主线路径现在已经具备 step-level structured I/O、feature workflow schema 绑定，以及 plan→todo→execute→report 的最小结构化闭环；`feature` / `research` 的 execute 也已能在 runtime 中根据 todo 实际推进情况重复执行，而不是在首轮 partial execute 后直接落到 report；当前下一优先级回到 `Task 10` 与 `Task 11`。_

_任务编号以 `docs/specs/omega-agent-impl-plan.md` 为准；为支持可运行里程碑拆分，`TODO` 中允许使用 `8A/8B`、`15A/15B/15C/15D` 这类子任务后缀。_

### High

- **Task 15F-15 / 15F-16 / 15F-17 / 15B-24 ~ 15B-26**: 大文件拆分维护需要前移；`omega-session/src/lib.rs`、`omega-workflow/src/lib.rs` 与 `omega-tui/src/{app,event,render}.rs` 已成为后续 Task 10 / Task 11 持续放大复杂度的主阻力，应该先抽离内联测试，再收拢 runtime / UI 模块边界。
- **Task 10**: `omega-subagent` 仍重要，但应建立在新的 all-step loop + step context 主路径之上推进，避免继续绑定 v1 workflow 假设；当前 execute 的 runtime repeat 已经补齐，剩余 gap 更集中在跨 step / 跨 workflow 的自治编排。
- **Task 11**: 上下文压缩仍重要，但应建立在新的 session context 边界之上，而不是只对 raw transcript 做早期补丁。

### Medium

- **Task 4**: `omega-tasks` 作为持久化任务层，价值明确，但不应先于 skills/subagent。
- **Task 12**: `omega-background` 排在任务系统附近，属于把 Agent 从同步单轮执行推进到更实用执行模型的下一步。
- **Task 3**: `omega-message` 仍重要，但它真正释放价值要等到 subagent 与 team 机制接入，因此不再放在最前。
- **Task 13**: `omega-team` 保持中优先级，但应建立在 `omega-subagent` 与 `omega-message` 之上推进。

### Low

- **Task 6**: `omega-worktree` 对后期自治执行很重要，但当前尚未到隔离执行成为主瓶颈的阶段。
- **Task 15B-8 ~ 15B-12**: 高级 TUI 能力已解除结构阻塞，但主线仍应优先 skills/subagent；高阶交互体验继续保持后移。
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

### ── M2C: Large File Maintainability Follow-up ──

> 验证方式：按 crate 定向运行 `cargo test` / `cargo clippy`，确保拆分后行为不变；热点文件行数明显下降，生产代码与测试、运行时与 helper、配置模型与默认落盘逻辑边界更清晰
> 对标：先做低风险的测试抽离，再做 `session / workflow / tui / client / core / docs` 的职责收口，避免 God file 继续膨胀

_2026-03-25 补充：已按仓库当前真实体量做过一轮扫描；当前最大热点集中在 `crates/omega-session/src/lib.rs`、`crates/omega-tui/src/{app,event,render}.rs`、`crates/omega-workflow/src/lib.rs`、`crates/omega-client/src/{anthropic,lib}.rs`，以及 `docs/specs/omega-agent-impl-plan.md` / `docs/specs/omega-step-session-asset-model.md`。本组任务用于把这些热点在继续扩张前先拆开。_

### Task 15F-15: omega-session / omega-workflow / omega-tui / omega-client / omega-core — Inline Test Extraction
- **Status**: Pending
- **Priority**: High
- **Description**: 将大体量根文件中的内联测试模块优先迁出到 `tests/` 或按模块拆开的 sibling test 文件，先在不改变生产行为的前提下降低 `lib.rs` / `app.rs` / `event.rs` / `render.rs` 的体量与阅读噪音。
- **Complexity**: L
- **Planning Note**: 这是最低风险、收益最高的第一步；当前主要目标包括 `crates/omega-session/src/lib.rs`、`crates/omega-workflow/src/lib.rs`、`crates/omega-tui/src/app.rs`、`crates/omega-tui/src/event.rs`、`crates/omega-tui/src/render.rs`、`crates/omega-client/src/lib.rs` 与 `crates/omega-core/src/lib.rs`。
- **Blocks**: Task 15F-16, Task 15F-17, Task 15B-24, Task 15B-25, Task 15B-26, Task 2B, Task 14A
- **Related**: docs/specs/omega-agent-impl-plan.md, docs/specs/omega-step-session-asset-model.md, docs/specs/omega-workflow-package.md, docs/specs/omega-tui-runtime-experience.md

### Task 15F-16: omega-session — Session Runtime Decomposition
- **Status**: Pending
- **Priority**: High
- **Description**: 将 `omega-session` 当前单体 `lib.rs` 拆为更稳定的模块边界，至少分离 `session state/context`、`workflow turn runner`、`structured output validation & recovery`、`routing heuristics`、`prompt builders` 与 `runtime UI emitters`，同时保持 `AgentSession` 对外 API 稳定。
- **Complexity**: XL
- **Planning Note**: 当前 `omega-session` 同时承载 `AgentSession` / `SessionContext`、`WorkflowTurnRunner`、output repair/validation、scene/workflow promotion、tool/detail preview、runtime UI envelope emit 与大块测试；继续在这个文件上叠加 Task 10 / Task 11 会持续放大维护成本。
- **Blocked by**: Task 15F-15
- **Blocks**: Task 10, Task 11
- **Related**: docs/specs/omega-step-session-asset-model.md, docs/specs/omega-scene-routing.md, docs/specs/omega-runtime-ui-message-contract.md

### Task 15F-17: omega-workflow — Workflow Model / Config / Builtin Defaults Split
- **Status**: Pending
- **Priority**: High
- **Description**: 将 `omega-workflow` 当前单体 `lib.rs` 拆为 `tool policy`、`scene/workflow catalog model`、`TOML config parsing`、`builtin workflow/prompt/schema defaults` 与 `filesystem materialization/loading` 等模块，降低 workflow contract 继续演化时的改动面。
- **Complexity**: L
- **Planning Note**: 当前文件同时包含内建 prompt/schema/TOML 常量、catalog model、config parser、默认文件生成与测试；这类“配置模型 + 内建内容 + 文件系统写入”混排已经开始阻碍后续 workflow 扩展。
- **Blocked by**: Task 15F-15
- **Related**: docs/specs/omega-workflow-package.md, docs/specs/omega-scene-routing.md, docs/specs/omega-agent-impl-plan.md

### Task 15B-24: omega-tui — App State / Diagnostics Helper Split
- **Status**: Pending
- **Priority**: High
- **Description**: 将 `omega-tui/src/app.rs` 中的 `App` 状态机与 `diagnostics formatting`、`text selection/wrap helpers`、`todo / response summarization helpers` 分离，确保主状态容器不再与大量展示辅助逻辑耦合。
- **Complexity**: L
- **Planning Note**: 当前 `app.rs` 同时持有 `App` 主状态、diagnostics line/detail 构造、tool/response/thinking 摘要、文本选择与 wrap helper，以及大块测试；适合先从 helper 抽离开始。
- **Blocked by**: Task 15F-15
- **Related**: docs/specs/omega-tui-runtime-experience.md, docs/specs/omega-tui-input-status-layout.md

### Task 15B-25: omega-tui — Event Routing Module Split
- **Status**: Pending
- **Priority**: High
- **Description**: 将 `omega-tui/src/event.rs` 拆为 `keyboard`、`overlay intent`、`mouse`、`clipboard` 与输入编辑辅助模块，避免单文件继续承载所有交互入口。
- **Complexity**: M
- **Planning Note**: 当前 `event.rs` 的核心问题不是算法复杂，而是多种输入通道和 overlay 逻辑都堆在一个文件内；后续继续加快捷键、overlay 或 sidebar 交互会更难守住边界。
- **Blocked by**: Task 15F-15
- **Related**: docs/specs/omega-tui-modal-keymap.md, docs/specs/omega-tui-overlay-popups.md, docs/specs/omega-tui-runtime-experience.md

### Task 15B-26: omega-tui — Render Pipeline Split
- **Status**: Pending
- **Priority**: High
- **Description**: 将 `omega-tui/src/render.rs` 拆为 `main layout`、`sidebar`、`status bar`、`overlay` 与 `style helpers` 模块，降低 UI 扩展时的渲染耦合。
- **Complexity**: M
- **Planning Note**: 当前 `render.rs` 同时处理主布局、底部状态带、侧栏、overlay、样式辅助与测试；继续把更多 runtime 面板或视觉语义叠加在同一文件内会让改动难以定位。
- **Blocked by**: Task 15F-15
- **Related**: docs/specs/omega-tui-runtime-experience.md, docs/specs/omega-tui-collapsible-sidebar.md, docs/specs/omega-tui-overlay-popups.md

### Task 2B: omega-client — Provider Client Module Split
- **Status**: Pending
- **Priority**: Medium
- **Description**: 将 `omega-client/src/anthropic.rs` 拆为 `provider models`、`services`、`transport`、`stream parsing` 模块，并把 `omega-client/src/lib.rs` 中的 provider-neutral chat contract、response builder 与 `Minimax` adapter 进一步分离。
- **Complexity**: L
- **Planning Note**: 当前 client 层仍可读，但 `anthropic.rs` 已同时承载 typed models、service surface、transport 与 SSE parser；继续增加 provider 行为差异会快速把这个文件推向第二个 `omega-session`。
- **Blocked by**: Task 15F-15
- **Related**: docs/specs/omega-client-anthropic-api-abstraction.md, docs/specs/omega-agent-impl-plan.md

### Task 14A: omega-core — Agent Loop / Tool Factory Split
- **Status**: Pending
- **Priority**: Medium
- **Description**: 将 `omega-core` 根文件中的 `Agent` loop、tool result glue、默认 tool factory 与测试拆开，保持 `Agent` 对外语义稳定，但让核心循环与工具装配不再同文件演化。
- **Complexity**: M
- **Planning Note**: `omega-core` 目前还没有 `omega-session` 那么紧急，但已经出现“核心 loop + 默认 tool wiring + 大量测试”共存的趋势，应该在继续扩展前先做边界收口。
- **Blocked by**: Task 15F-15
- **Related**: docs/specs/omega-agent-impl-plan.md, docs/specs/omega-runtime-ui-message-contract.md

### Task 15F-18: docs/specs — Large Spec Split And Index Cleanup
- **Status**: Pending
- **Priority**: Medium
- **Description**: 将 `docs/specs/omega-agent-impl-plan.md` 与 `docs/specs/omega-step-session-asset-model.md` 拆为“索引页 + 主题子文档”，把实现计划、runtime contract、session asset 演进、routing/repair/diagnostics 等主题从单体 spec 中分离出来。
- **Complexity**: M
- **Planning Note**: 当前两份 spec 已明显超出“单主题文档”规模；如果不先拆，后续在实现 Task 10 / Task 11 和 runtime follow-up 时会继续把设计历史、当前契约与未来计划混写在一起。
- **Related**: docs/specs/omega-agent-impl-plan.md, docs/specs/omega-step-session-asset-model.md, docs/README.md

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

### ── M6: 上下文压缩 (s06) ──

> 验证方式：长对话后观察 token 使用量下降，对话质量不降
> 对标：learn-claude-code s06_context_compact.py

### Task 11: omega-compression — 上下文压缩
- **Status**: Pending
- **Priority**: High
- **Description**: 实现 estimate_tokens 和 microcompact，超阈值时压缩历史消息；压缩策略应建立在 session context + raw transcript 的组合边界上，而不是只面向原始消息列表
- **Related**: docs/specs/omega-agent-impl-plan.md, docs/specs/omega-tui-runtime-experience.md

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

### Task 15D: `omega-tui` 非 UI 职责剥离
- **Status**: Completed
- **Completed**: 2026-03-19
- **Priority**: High
- **Description**: 将 `omega-tui` 中不属于 UI 的 turn orchestration 与 observability 逻辑拆为独立 crate；该任务先于剩余主线执行，Phase 1 先实现 `omega-session` 与 `omega-observability`，Phase 2 再按需要评估 `omega-interaction`
- **Related**: docs/specs/omega-tui-non-ui-extraction.md, docs/decisions/006-omega-tui-ui-boundary.md
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

### Task 15B-8: omega-tui — Markdown 渲染
- **Status**: Pending
- **Priority**: Low
- **Description**: Agent 回复中解析 Markdown，标题加粗、列表缩进、行内代码反色、代码块区分背景色
- **Related**: docs/specs/omega-agent-spec.md

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
