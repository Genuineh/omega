# TODO

## Current Priorities

_按可运行里程碑排序。每个里程碑完成后可 `cargo run` 验证效果。_

_任务编号以 `docs/specs/omega-agent-impl-plan.md` 为准；为支持可运行里程碑拆分，`TODO` 中允许使用 `8A/8B`、`15A/15B` 这类子任务后缀。未出现在实现计划中的仓库治理事项，会明确标记为“非计划项”。_

---

## Current Priorities

### ── M1.5: 可观察性与日志系统 ──

> 验证方式：`OMEGA_LOG=debug cargo run -p omega-tui` → TUI Logs 面板实时显示日志 + `~/.omega/logs/` 出现 JSONL 文件
> 对标：生产级可观察性基础设施
> PRD: docs/prds/observability-logging.md | ADR: docs/decisions/005-tracing-observability.md

### Task O1: omega-tui — 初始化 tracing subscriber
- **Status**: Completed
- **Completed**: 2026-03-18
- **Priority**: High
- **Description**: 配置 tracing-subscriber registry：终端层 (compact + EnvFilter via OMEGA_LOG) + 文件层 (JSON → ~/.omega/logs/)，移除裸 eprintln
- **Summary**: 双层 subscriber（UI 面板 compact + JSONL 文件）、OMEGA_LOG 控制级别、OMEGA_LOG_DIR/OMEGA_LOG_FILE 控制文件输出、chrono 日期轮转文件名、mpsc::sync_channel + 自定义 MakeWriter 路由日志到 TUI Logs 面板、每帧最多 20 条防止 UI 卡顿、ratatui ListState 双面板滚动支持（Tab 切换焦点、↑↓ 滚动、鼠标滚轮）、Ctrl+C 退出、workspace 零警告、74 项测试全通过
- **Related**: docs/prds/observability-logging.md

### Task O2: omega-client — LLM 调用追踪
- **Status**: Completed
- **Completed**: 2026-03-18
- **Priority**: High
- **Description**: MinimaxClient::chat() 添加 llm_call span，记录 model/max_tokens/token_usage/stop_reason/duration_ms，TRACE 级记录原始 JSON
- **Summary**: 使用 #[instrument] 宏添加 llm_call span，通过 fields 记录 model/max_tokens/provider/duration_ms/input_tokens/output_tokens/stop_reason，trace! 记录原始 request JSON，debug! 记录 response JSON，Span::current().record() 在返回前填充动态字段，29 项测试全通过，clippy 零警告
- **Related**: docs/prds/observability-logging.md

### Task O3: omega-tools — 工具执行追踪
- **Status**: Completed
- **Completed**: 2026-03-18
- **Priority**: High
- **Description**: ToolDispatcher::dispatch() 添加 tool_exec span，记录 tool_name/duration_ms/success
- **Summary**: 使用 #[instrument] 宏添加 tool_exec span，通过 fields 记录 tool_name/duration_ms/success，Span::current().record() 在返回前填充动态字段，12 项测试全通过，clippy 零警告
- **Related**: docs/prds/observability-logging.md

### Task O4: omega-tools-builtin — BashHandler 追踪
- **Status**: Completed
- **Completed**: 2026-03-18
- **Priority**: High
- **Description**: BashHandler::execute() 添加结构化日志，命令 info、安全拦截 warn、超时 error、输出截断 debug
- **Summary**: 命令执行使用 info!(bash.command)、安全拦截使用 warn!(bash.blocked_reason)、超时使用 error!(bash.timeout_seconds)、输出截断使用 debug!(bash.output_truncated)，21 项测试全通过，clippy 零警告
- **Related**: docs/prds/observability-logging.md

### Task O5: omega-core — Agent Loop 追踪
- **Status**: Completed
- **Completed**: 2026-03-18
- **Priority**: High
- **Description**: run_loop_with 中创建 session span (uuid) + 每次迭代 agent_loop span，记录 iteration/message_count/stop_reason
- **Summary**: session_id (uuid) 在外层 span 记录，每次迭代创建 agent_loop 子 span 记录 iteration/message_count，stop_reason 在返回时记录，info! 记录 started/completed 事件，error! 记录超过最大迭代次数，12 项测试全通过，clippy 零警告
- **Related**: docs/prds/observability-logging.md

### Task O6: 验证与文档
- **Status**: Completed
- **Completed**: 2026-03-18
- **Priority**: High
- **Description**: 端到端验证日志输出，更新开发指南中的日志使用说明
- **Summary**: 74 项测试全通过（omega-client 29 + omega-core 12 + omega-tools 12 + omega-tools-builtin 21），无 println/eprintln 残留（REPL 交互输出除外），更新开发指南添加日志系统使用说明（环境变量、日志级别、Span 结构、代码示例）
- **Related**: docs/prds/observability-logging.md

---

## Backlog

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

### ── M2: 文件工具 (s02) ──

> 验证方式：`cargo run` → 让 Agent 读/写/编辑文件
> 对标：learn-claude-code s02_tool_use.py

### Task 8B: omega-tools-builtin — ReadHandler + WriteHandler + EditHandler
- **Status**: Pending
- **Priority**: High
- **Description**: 新增文件读取、写入、编辑三个 handler，含路径安全校验（不允许逃逸工作目录）
- **Related**: learn/learn-claude-code/agents/s02_tool_use.py

### ── M3: Todo 管理 (s03) ──

> 验证方式：`cargo run` → Agent 自动创建/更新/展示 todo
> 对标：learn-claude-code s03_todo_write.py

### Task 9: omega-todo — TodoManager
- **Status**: Pending
- **Priority**: Medium
- **Description**: 实现 TodoManager，支持 update/render/has_open_items，注册为 tool
- **Related**: docs/specs/omega-agent-impl-plan.md

### ── M4: 子智能体 (s04) ──

> 验证方式：`cargo run` → Agent 将子任务委托给 SubAgent 独立完成
> 对标：learn-claude-code s04_subagent.py

### Task 10: omega-subagent — SubAgent
- **Status**: Pending
- **Priority**: Medium
- **Description**: 实现 SubAgent，独立 message list + run_loop，父 Agent 通过 tool 调度
- **Related**: docs/specs/omega-agent-impl-plan.md

### ── M5: Skill 加载 (s05) ──

> 验证方式：`cargo run` → Agent 按任务类型自动加载 skill 到 system prompt
> 对标：learn-claude-code s05_skill_loading.py

### Task 5: omega-skills — SkillLoader
- **Status**: Pending
- **Priority**: Medium
- **Description**: 实现 SkillLoader，扫描 skills 目录，按关键词匹配加载
- **Related**: docs/specs/omega-agent-impl-plan.md

### ── M6: 上下文压缩 (s06) ──

> 验证方式：长对话后观察 token 使用量下降，对话质量不降
> 对标：learn-claude-code s06_context_compact.py

### Task 11: omega-compression — 上下文压缩
- **Status**: Pending
- **Priority**: Medium
- **Description**: 实现 estimate_tokens 和 microcompact，超阈值时压缩历史消息
- **Related**: docs/specs/omega-agent-impl-plan.md

### ── M7: 任务系统 (s07) ──

> 验证方式：`cargo run` → Agent 创建/查询/更新持久化任务
> 对标：learn-claude-code s07_task_system.py

### Task 4: omega-tasks — TaskManager
- **Status**: Pending
- **Priority**: Medium
- **Description**: 实现 TaskManager 持久化任务系统，支持 CRUD 操作，注册为 tool
- **Related**: docs/specs/omega-agent-impl-plan.md

### ── M8: 后台任务 (s08) ──

> 验证方式：`cargo run` → Agent 在后台执行长时间任务，不阻塞主循环
> 对标：learn-claude-code s08_background_tasks.py

### Task 12: omega-background — BackgroundManager
- **Status**: Pending
- **Priority**: Medium
- **Description**: 实现 BackgroundManager 后台任务管理，支持 spawn/check/collect
- **Related**: docs/specs/omega-agent-impl-plan.md

### ── M9: 团队协作 (s09-s11) ──

> 验证方式：`cargo run` → 多个 Agent 通过消息总线协作完成任务
> 对标：learn-claude-code s09-s11

### Task 3: omega-message — 消息系统
- **Status**: Pending
- **Priority**: Medium
- **Description**: 实现 MessageBus 消息总线，支持 send/read_inbox/broadcast
- **Related**: docs/specs/omega-agent-impl-plan.md

### Task 13: omega-team — 团队管理
- **Status**: Pending
- **Priority**: Medium
- **Description**: 实现 TeammateManager 团队管理和自治智能体
- **Related**: docs/specs/omega-agent-impl-plan.md

### ── M10: Worktree 隔离 (s12) ──

> 验证方式：`cargo run` → Agent 在独立 worktree 中执行任务，互不干扰
> 对标：learn-claude-code s12_worktree_task_isolation.py

### Task 6: omega-worktree — WorktreeManager
- **Status**: Pending
- **Priority**: Medium
- **Description**: 实现 WorktreeManager 管理 git worktree
- **Related**: docs/specs/omega-agent-impl-plan.md

### ── M11: 完整 TUI (高级功能) ──

> 验证方式：`cargo run` → 完整 ratatui 终端界面，支持 Markdown 渲染、代码高亮、输入历史等
> 对标：生产级交互体验
> 前置：M1.7 (TUI 基础美化) 已完成

_基础体验已在 M1.7 完成，此处保留高级特性。_

### Task 15B-8: omega-tui — Markdown 渲染
- **Status**: Pending
- **Priority**: Medium
- **Description**: Agent 回复中解析 Markdown，标题加粗、列表缩进、行内代码反色、代码块区分背景色
- **Related**: docs/specs/omega-agent-spec.md

### Task 15B-9: omega-tui — 代码语法高亮
- **Status**: Pending
- **Priority**: Medium
- **Description**: 代码块内按语言做语法高亮（syntect 或 tree-sitter），至少支持 Rust/Python/Shell
- **Related**: docs/specs/omega-agent-spec.md

### Task 15B-10: omega-tui — 输入历史
- **Status**: Pending
- **Priority**: Medium
- **Description**: ↑↓键浏览历史输入、持久化到 ~/.omega/history

### Task 15B-11: omega-tui — 面板内搜索
- **Status**: Pending
- **Priority**: Low
- **Description**: Ctrl+F 触发搜索模式，高亮匹配文本，n/N 跳转

### Task 15B-12: omega-tui — 可调面板与会话统计
- **Status**: Pending
- **Priority**: Low
- **Description**: 拖拽或快捷键调整面板宽度比例；状态栏显示 token 使用量、对话轮次

### Task 16: 最终整合测试
- **Status**: Pending
- **Priority**: High
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
- **Summary**: MinimaxConfig::from_env 构造客户端、create_default_tools 注册 bash 工具、run_loop_with 回调显示工具调用（命令黄色高亮 + 输出预览 200 字符）、UTF-8 安全截断、EOF/q/exit 退出
- **Related**: learn/learn-claude-code/agents/s01_agent_loop.py

### 非计划项：文档体系建设
- **Status**: Completed
- **Completed**: 2026-03-18
- **Summary**: 完成 docs 体系初始化，包含根索引、技术规格、实现计划、4 个 ADR、开发指南及 TODO 跟踪
