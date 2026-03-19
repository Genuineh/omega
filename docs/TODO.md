# TODO

## Current Priorities

_按当前仓库真实主路径重排。判断依据：`cargo test` 全工作区通过；s02 文件工具与 s03 todo 管理已完成；`Task 15C` 与 `Task 15D` 已完成，交互层主边界已落到 `omega-tui` + `omega-session` + `omega-observability`；`omega-subagent`、`omega-skills`、`omega-message` 等后续 crate 仍基本处于 stub 状态。_

_任务编号以 `docs/specs/omega-agent-impl-plan.md` 为准；为支持可运行里程碑拆分，`TODO` 中允许使用 `8A/8B`、`15A/15B/15C/15D` 这类子任务后缀。_

### High

- **Task 10**: `omega-subagent` 维持高优先级，作为后续 team/background/worktree 能力的直接基础。

### Medium

- **Task 11**: 在多轮对话和后续 agent 能力继续增长前补上上下文压缩，避免先扩功能、再补 token 控制。
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
- **Summary**: `omega-tui` 新增统一 `Sidebar` shell 与本地 `SidebarState`，把右侧辅助区重构为 `Response | Sidebar` 布局，并在侧边栏顶部加入轻量可聚焦 rail；当前 rail 直接以 `Todos | Logs` 呈现，`Logs` 面板不再显示 `Activity[Logs]` 包装标题，支持 `leader b` 整体收起/展开、rail 左右切换、`x` 折叠 section、`Enter` 激活展开内容，同时显式禁止把最后一个 section 收起成空 sidebar；窄终端会自动隐藏侧边栏并回退焦点，状态栏同步增加 sidebar/logs 徽章信息；相关布局、事件和状态单测已覆盖，并以 `cargo test -p omega-tui`、`cargo test -p omega-keymap -p omega-tui`、`cargo clippy -p omega-keymap -p omega-tui --all-targets -- -D warnings` 验证

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
- **Summary**: `omega-skills` 新增递归扫描 `.claude/skills` 与 `skills/` 的 `SkillLoader`，支持 frontmatter 读取、技能描述汇总、按任务文本做关键词匹配，并提供 `load_skill` 工具按需返回完整 `<skill ...>` 内容；`omega-core::create_default_tools` 已默认注册该工具，`omega-repl` 与 `omega-session` 会在每轮按当前输入把匹配到的 skill 正文预装进 system prompt，同时始终附带低成本的 skills 描述列表；新增 `omega-skills`、`omega-repl` 相关单测，并以定向 cargo test 验证通过

### ── M6: 上下文压缩 (s06) ──

> 验证方式：长对话后观察 token 使用量下降，对话质量不降
> 对标：learn-claude-code s06_context_compact.py

### Task 11: omega-compression — 上下文压缩
- **Status**: Pending
- **Priority**: Medium
- **Description**: 实现 estimate_tokens 和 microcompact，超阈值时压缩历史消息
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

### Task 15C: 交互层重构 — omega-tui 库化 + omega-repl 新包
- **Status**: Completed
- **Completed**: 2026-03-19
- **Priority**: High
- **Description**: 在继续高阶 TUI 能力前，先将 `omega-tui` 拆为 library-first crate，并新增 `omega-repl` 承接最小 REPL，降低后续功能继续堆叠在单一入口文件中的风险
- **Related**: docs/specs/omega-interaction-layer-refactor.md

### Task 15D: `omega-tui` 非 UI 职责剥离
- **Status**: Completed
- **Completed**: 2026-03-19
- **Priority**: High
- **Description**: 将 `omega-tui` 中不属于 UI 的 turn orchestration 与 observability 逻辑拆为独立 crate；该任务先于剩余主线执行，Phase 1 先实现 `omega-session` 与 `omega-observability`，Phase 2 再按需要评估 `omega-interaction`
- **Related**: docs/specs/omega-tui-non-ui-extraction.md, docs/decisions/006-omega-tui-ui-boundary.md
- **Summary**: 新增 `omega-session` 承接 Agent turn orchestration 与 checkpoint/interrupt/update 协议，新增 `omega-observability` 承接 tracing 初始化、UI sink、JSONL 文件日志与 ANSI 清洗；`omega-tui` 改为仅保留 UI 运行时并消费外部 `SessionUpdate`/trace channel，`omega-repl` 也接入统一 tracing 初始化；`cargo test -p omega-session -p omega-observability -p omega-tui -p omega-repl` 通过

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
- **Summary**: MinimaxConfig::from_env 构造客户端、create_default_tools 注册工具集、run_loop_with 回调显示工具调用（命令预览 + 输出预览）、UTF-8 安全截断、EOF/q/exit 退出；该功能已在 `Task 15C` 中迁移到独立的 `omega-repl` 包
- **Related**: learn/learn-claude-code/agents/s01_agent_loop.py

### 非计划项：文档体系建设
- **Status**: Completed
- **Completed**: 2026-03-18
- **Summary**: 完成 docs 体系初始化，包含根索引、技术规格、实现计划、4 个 ADR、开发指南及 TODO 跟踪
