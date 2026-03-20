---
status: active
owner: omega-team
created: 2026-03-18
updated: 2026-03-19
version: 1.0
related_prds: []
---

# Omega Agent 实现计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 逐步实现并重构当前工作空间；在既有 15 个独立 crate 基础上，先完成 `Task 15D` 交互层继续拆分，规划新增 `omega-session` 与 `omega-observability`，最终组合成完整的 Omega Agent

**Architecture:** 每个 crate 独立实现，通过 Cargo workspace 组合。底层 crate 无依赖，上层依赖下层。

**Tech Stack:** Rust, tokio, reqwest, ratatui, serde, uuid

---

### Task 1: 工作空间初始化

**Status:** Completed（代码与验证已完成；提交步骤按当前工作流延后）

**Files:**
- Create: `Cargo.toml`

- [x] **Step 1: 创建 Cargo.toml 工作空间配置**

```toml
[workspace]
members = [
    "crates/omega-client",
    "crates/omega-message",
    "crates/omega-tasks",
    "crates/omega-skills",
    "crates/omega-worktree",
    "crates/omega-tools",
    "crates/omega-tools-builtin",
    "crates/omega-todo",
    "crates/omega-subagent",
    "crates/omega-compression",
    "crates/omega-background",
    "crates/omega-team",
    "crates/omega-core",
    "crates/omega-tui",
]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
authors = ["Omega Team"]
license = "MIT"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
ratatui = "0.28"
crossterm = "0.28"
anyhow = "1"
thiserror = "2"
uuid = { version = "1", features = ["v4"] }
```

- [x] **Step 2: 提交不属于完成条件**（按当前工作流延后，待用户明确要求后执行）

```bash
git add Cargo.toml
git commit -m "chore: init workspace"
```

---

### Task 2: omega-client - LLM 抽象与 Minimax 适配器

**Status:** Completed（代码与验证已完成；提交步骤按当前工作流延后）

**Files:**
- Create: `crates/omega-client/Cargo.toml`
- Create: `crates/omega-client/src/lib.rs`

- [x] **Step 1: 创建 Cargo.toml**

```toml
[package]
name = "omega-client"
version.workspace = true
edition.workspace = true

[dependencies]
anyhow.workspace = true
async-trait.workspace = true
reqwest.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tokio.workspace = true
```

- [x] **Step 2: 实现抽象接口与 Minimax Provider**

```rust
// crates/omega-client/src/lib.rs
use async_trait::async_trait;
use reqwest::Client;

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ClientError>;
    fn provider_name(&self) -> &'static str;
}

pub struct ChatRequest {
    pub system: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub max_tokens: u32,
}

pub struct MinimaxConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub anthropic_version: String,
}

pub struct MinimaxClient {
    http_client: Client,
    config: MinimaxConfig,
}

impl MinimaxClient {
    pub fn new(config: MinimaxConfig) -> Self
}

#[async_trait]
impl LlmClient for MinimaxClient {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ClientError>
    fn provider_name(&self) -> &'static str
}
```

- [x] **Step 3: 编译验证**

```bash
cargo build -p omega-client
```

- [x] **Step 4: 提交不属于完成条件**（按当前工作流延后，待用户明确要求后执行）

```bash
git add crates/omega-client/
git commit -m "feat: add omega-client"
```

---

### Task 3: omega-message - 消息系统

**Files:**
- Create: `crates/omega-message/Cargo.toml`
- Create: `crates/omega-message/src/lib.rs`

- [ ] **Step 1: 创建 Cargo.toml**

```toml
[package]
name = "omega-message"
version.workspace = true
edition.workspace = true

[dependencies]
anyhow.workspace = true
serde.workspace = true
serde_json.workspace = true

[dev-dependencies]
omega-client = { path = "../omega-client" }
```

- [ ] **Step 2: 实现 MessageBus**

```rust
// crates/omega-message/src/lib.rs
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub from: String,
    pub content: String,
    pub timestamp: f64,
}

pub struct MessageBus {
    inbox_dir: std::path::PathBuf,
}

impl MessageBus {
    pub fn new(inbox_dir: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(inbox_dir)?;
        Ok(Self { inbox_dir: inbox_dir.to_path_buf() })
    }

    fn inbox_path(&self, name: &str) -> std::path::PathBuf {
        self.inbox_dir.join(format!("{}.jsonl", name))
    }

    pub fn send(&self, from: &str, to: &str, content: &str, msg_type: &str) -> anyhow::Result<String> {
        let msg = Message {
            msg_type: msg_type.to_string(),
            from: from.to_string(),
            content: content.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?.as_secs_f64(),
        };
        let line = serde_json::to_string(&msg)?;
        std::fs::OpenOptions::new().create(true).append(true)
            .open(self.inbox_path(to))?.write_all(line.as_bytes())?;
        Ok(format!("Sent {} to {}", msg_type, to))
    }

    pub fn read_inbox(&self, name: &str) -> anyhow::Result<Vec<Message>> {
        let path = self.inbox_path(name);
        if !path.exists() { return Ok(Vec::new()); }
        let content = std::fs::read_to_string(&path)?;
        let msgs: Vec<Message> = content.lines()
            .filter_map(|l| serde_json::from_str(l).ok()).collect();
        std::fs::write(&path, "")?;
        Ok(msgs)
    }
}
```

- [x] **Step 3: 编译验证**

```bash
cargo build -p omega-message
```

- [ ] **Step 4: 提交**

```bash
git add crates/omega-message/
git commit -m "feat(s09): add omega-message"
```

---

### Task 4: omega-tasks - 任务系统

**Files:**
- Create: `crates/omega-tasks/Cargo.toml`
- Create: `crates/omega-tasks/src/lib.rs`

- [ ] **Step 1: 创建 Cargo.toml**

```toml
[package]
name = "omega-tasks"
version.workspace = true
edition.workspace = true

[dependencies]
anyhow.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
```

- [ ] **Step 2: 实现 TaskManager**

```rust
// crates/omega-tasks/src/lib.rs
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: u32,
    pub subject: String,
    pub description: String,
    pub status: TaskStatus,
    pub owner: Option<String>,
    #[serde(rename = "blockedBy")]
    pub blocked_by: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus { Pending, InProgress, Completed }

pub struct TaskManager {
    dir: std::path::PathBuf,
    cache: RwLock<HashMap<u32, Task>>,
    next_id: RwLock<u32>,
}

impl TaskManager {
    pub fn new(dir: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(dir)?;
        Ok(Self { dir: dir.to_path_buf(), cache: RwLock::new(HashMap::new()), next_id: RwLock::new(1) })
    }

    pub fn create(&self, subject: &str, description: &str) -> anyhow::Result<Task> {
        let id = *self.next_id.write()?;
        *self.next_id.write()? += 1;
        let task = Task { id, subject: subject.to_string(), description: description.to_string(), status: TaskStatus::Pending, owner: None, blocked_by: Vec::new() };
        self.cache.write()?.insert(id, task.clone());
        std::fs::write(self.dir.join(format!("task_{}.json", id)), serde_json::to_string_pretty(&task)?)?;
        Ok(task)
    }

    pub fn get(&self, id: u32) -> Option<Task> { self.cache.read().unwrap().get(&id).cloned() }

    pub fn list_all(&self) -> Vec<Task> {
        let mut tasks: Vec<_> = self.cache.read().unwrap().values().cloned().collect();
        tasks.sort_by_key(|t| t.id);
        tasks
    }
}
```

- [ ] **Step 3: 编译验证**

```bash
cargo build -p omega-tasks
```

- [ ] **Step 4: 提交**

```bash
git add crates/omega-tasks/
git commit -m "feat(s07): add omega-tasks"
```

---

### Task 5: omega-skills - Skill 加载

**Files:**
- Create: `crates/omega-skills/Cargo.toml`
- Create: `crates/omega-skills/src/lib.rs`

- [ ] **Step 1: 创建 Cargo.toml**

```toml
[package]
name = "omega-skills"
version.workspace = true
edition.workspace = true

[dependencies]
anyhow.workspace = true
serde.workspace = true
```

- [ ] **Step 2: 实现 SkillLoader**

```rust
// crates/omega-skills/src/lib.rs
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Skill { pub name: String, pub description: String, pub body: String }

pub struct SkillLoader { skills: HashMap<String, Skill> }

impl SkillLoader {
    pub fn new(skills_dir: &Path) -> anyhow::Result<Self> {
        let mut skills = HashMap::new();
        if skills_dir.exists() {
            for entry in std::fs::read_dir(skills_dir)? {
                let path = entry?.path();
                if path.is_dir() {
                    let md = path.join("SKILL.md");
                    if md.exists() {
                        let content = std::fs::read_to_string(&md)?;
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
                        skills.insert(name.to_string(), Skill { name: name.to_string(), description: String::new(), body: content });
                    }
                }
            }
        }
        Ok(Self { skills })
    }

    pub fn load(&self, name: &str) -> String {
        match self.skills.get(name) {
            Some(s) => format!("<skill name=\"{}\">\n{}\n</skill>", s.name, s.body),
            None => format!("Error: Unknown skill '{}'", name),
        }
    }
}
```

- [ ] **Step 3: 编译验证**

```bash
cargo build -p omega-skills
```

- [ ] **Step 4: 提交**

```bash
git add crates/omega-skills/
git commit -m "feat(s05): add omega-skills"
```

---

### Task 6: omega-worktree - Worktree 隔离

**Files:**
- Create: `crates/omega-worktree/Cargo.toml`
- Create: `crates/omega-worktree/src/lib.rs`

- [ ] **Step 1: 创建 Cargo.toml**

```toml
[package]
name = "omega-worktree"
version.workspace = true
edition.workspace = true

[dependencies]
anyhow.workspace = true
serde.workspace = true
serde_json.workspace = true

[dev-dependencies]
omega-client = { path = "../omega-client" }
```

- [ ] **Step 2: 实现 WorktreeManager**

```rust
// crates/omega-worktree/src/lib.rs
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    pub name: String,
    pub path: String,
    pub branch: String,
    pub task_id: Option<u32>,
    pub status: String,
}

pub struct WorktreeManager {
    repo_root: Path,
    dir: Path,
    index_path: Path,
}

impl WorktreeManager {
    pub fn new(repo_root: &Path) -> anyhow::Result<Self> {
        let dir = repo_root.join(".worktrees");
        std::fs::create_dir_all(&dir)?;
        let index_path = dir.join("index.json");
        if !index_path.exists() { std::fs::write(&index_path, r#"{"worktrees": []}"#)?; }
        Ok(Self { repo_root: repo_root.to_path_buf(), dir, index_path })
    }

    pub fn create(&self, name: &str, task_id: Option<u32>, base_ref: &str) -> anyhow::Result<Worktree> {
        let path = self.dir.join(name);
        let branch = format!("wt/{}", name);
        std::process::Command::new("git").args(["worktree", "add", "-b", &branch, path.to_str().unwrap(), base_ref]).current_dir(&self.repo_root).output()?;
        let wt = Worktree { name: name.to_string(), path: path.to_string_lossy().to_string(), branch, task_id, status: "active".to_string() };
        let mut idx: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&self.index_path)?)?;
        if let Some(arr) = idx.get_mut("worktrees").and_then(|v| v.as_array_mut()) { arr.push(serde_json::to_value(&wt)?); }
        std::fs::write(&self.index_path, serde_json::to_string_pretty(&idx)?)?;
        Ok(wt)
    }
}
```

- [ ] **Step 3: 编译验证**

```bash
cargo build -p omega-worktree
```

- [ ] **Step 4: 提交**

```bash
git add crates/omega-worktree/
git commit -m "feat(s12): add omega-worktree"
```

---

### Task 7: omega-tools - 工具抽象

**Status:** Completed（代码与验证已完成；提交步骤按当前工作流延后）

**Files:**
- Create: `crates/omega-tools/Cargo.toml`
- Create: `crates/omega-tools/src/lib.rs`

- [x] **Step 1: 创建 Cargo.toml**

```toml
[package]
name = "omega-tools"
version.workspace = true
edition.workspace = true

[dependencies]
anyhow.workspace = true
serde.workspace = true
serde_json.workspace = true

[dev-dependencies]
omega-client = { path = "../omega-client" }
```

- [x] **Step 2: 实现工具调度器**

```rust
// crates/omega-tools/src/lib.rs
use anyhow::Result;
use std::collections::HashMap;

pub trait ToolHandler: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    fn execute(&self, input: serde_json::Value) -> Result<String>;
}

pub struct ToolDispatcher { handlers: HashMap<String, Box<dyn ToolHandler>> }

impl ToolDispatcher {
    pub fn new() -> Self { Self { handlers: HashMap::new() } }
    pub fn register(&mut self, handler: Box<dyn ToolHandler>) { self.handlers.insert(handler.name().to_string(), handler); }
    pub fn dispatch(&self, name: &str, input: serde_json::Value) -> Result<String> {
        self.handlers.get(name).map(|h| h.execute(input)).unwrap_or_else(|| Ok(format!("Unknown tool: {}", name)))
    }
    pub fn to_schemas(&self) -> Vec<serde_json::Value> {
        let mut schemas: Vec<_> = self.handlers.values().map(|h| {
            serde_json::json!({
                "name": h.name(),
                "description": h.description(),
                "input_schema": h.input_schema(),
            })
        }).collect();
        schemas.sort_by(|a, b| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")));
        schemas
    }
    pub fn len(&self) -> usize { self.handlers.len() }
    pub fn is_empty(&self) -> bool { self.handlers.is_empty() }
    pub fn has_tool(&self, name: &str) -> bool { self.handlers.contains_key(name) }
    pub fn tool_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.handlers.keys().map(|key| key.as_str()).collect();
        names.sort();
        names
    }
}

impl Default for ToolDispatcher { fn default() -> Self { Self::new() } }
```

- [x] **Step 3: 编译验证**

```bash
cargo build -p omega-tools
```

- [x] **Step 4: 提交不属于完成条件**（按当前工作流延后，待用户明确要求后执行）

```bash
git add crates/omega-tools/
git commit -m "feat(s02): add omega-tools"
```

---

### Task 8: omega-tools-builtin - 内置工具

**TODO Mapping:** `Task 8A` = BashHandler（M1），`Task 8B` = ReadHandler / WriteHandler / EditHandler（M2）

**Progress:** `Task 8A` 已完成（2026-03-18）；`Task 8B` 仍待实现

**Files:**
- Create: `crates/omega-tools-builtin/Cargo.toml`
- Create: `crates/omega-tools-builtin/src/lib.rs`

- [ ] **Step 1: 创建 Cargo.toml**

```toml
[package]
name = "omega-tools-builtin"
version.workspace = true
edition.workspace = true

[dependencies]
omega-tools = { path = "../omega-tools" }
anyhow.workspace = true
serde_json.workspace = true
```

- [ ] **Step 2: 实现内置工具**

```rust
// crates/omega-tools-builtin/src/lib.rs
use omega_tools::ToolHandler;
use std::process::Command;
use std::path::PathBuf;

pub struct BashHandler;
impl ToolHandler for BashHandler {
    fn name(&self) -> &str { "bash" }
    fn description(&self) -> &str { "Run a shell command" }
    fn execute(&self, input: serde_json::Value) -> anyhow::Result<String> {
        let cmd = input["command"].as_str().unwrap_or("");
        let output = Command::new("sh").arg("-c").arg(cmd).output()?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

pub struct ReadHandler { root: PathBuf }
impl ReadHandler { pub fn new(root: PathBuf) -> Self { Self { root } } }
impl ToolHandler for ReadHandler {
    fn name(&self) -> &str { "read_file" }
    fn description(&self) -> &str { "Read file contents" }
    fn execute(&self, input: serde_json::Value) -> anyhow::Result<String> {
        let path = input["path"].as_str().unwrap_or("");
        Ok(std::fs::read_to_string(self.root.join(path))?)
    }
}

pub struct WriteHandler { root: PathBuf }
impl WriteHandler { pub fn new(root: PathBuf) -> Self { Self { root } } }
impl ToolHandler for WriteHandler {
    fn name(&self) -> &str { "write_file" }
    fn description(&self) -> &str { "Write file contents" }
    fn execute(&self, input: serde_json::Value) -> anyhow::Result<String> {
        let path = input["path"].as_str().unwrap_or("");
        let content = input["content"].as_str().unwrap_or("");
        let full = self.root.join(path);
        if let Some(p) = full.parent() { std::fs::create_dir_all(p)?; }
        std::fs::write(full, content)?;
        Ok(format!("Wrote {} bytes", content.len()))
    }
}
```

- [ ] **Step 3: 编译验证**

```bash
cargo build -p omega-tools-builtin
```

- [ ] **Step 4: 提交**

```bash
git add crates/omega-tools-builtin/
git commit -m "feat(s02): add omega-tools-builtin"
```

---

### Task 9: omega-todo - Todo 管理

**Files:**
- Create: `crates/omega-todo/Cargo.toml`
- Create: `crates/omega-todo/src/lib.rs`

- [ ] **Step 1: 创建 Cargo.toml**

```toml
[package]
name = "omega-todo"
version.workspace = true
edition.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
```

- [ ] **Step 2: 实现 TodoManager**

```rust
// crates/omega-todo/src/lib.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
    #[serde(rename = "activeForm")]
    pub active_form: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TodoStatus { Pending, InProgress, Completed }

pub struct TodoManager { items: Vec<TodoItem>, rounds_without_todo: usize }

impl TodoManager {
    pub fn new() -> Self { Self { items: Vec::new(), rounds_without_todo: 0 } }
    pub fn update(&mut self, items: Vec<TodoItem>) -> String {
        if items.len() > 20 { return "Error: Max 20 todos".to_string(); }
        if items.iter().filter(|i| i.status == TodoStatus::InProgress).count() > 1 { return "Error: Only one in_progress allowed".to_string(); }
        self.items = items; self.rounds_without_todo = 0;
        self.render()
    }
    pub fn render(&self) -> String {
        if self.items.is_empty() { return "No todos.".to_string(); }
        let lines: Vec<_> = self.items.iter().map(|i| {
            let m = match i.status { TodoStatus::Completed => "[x]", TodoStatus::InProgress => "[>]", TodoStatus::Pending => "[ ]" };
            let s = if i.status == TodoStatus::InProgress { format!(" <- {}", i.active_form) } else { String::new() };
            format!("{} {}{}", m, i.content, s)
        }).collect();
        lines.join("\n")
    }
    pub fn has_open_items(&self) -> bool { self.items.iter().any(|i| i.status != TodoStatus::Completed) }
    pub fn should_nag(&self) -> bool { self.has_open_items() && self.rounds_without_todo >= 3 }
    pub fn increment_rounds(&mut self) { self.rounds_without_todo += 1; }
    pub fn reset_rounds(&mut self) { self.rounds_without_todo = 0; }
}

impl Default for TodoManager { fn default() -> Self { Self::new() } }
```

- [ ] **Step 3: 编译验证**

```bash
cargo build -p omega-todo
```

- [ ] **Step 4: 提交**

```bash
git add crates/omega-todo/
git commit -m "feat(s03): add omega-todo"
```

---

### Task 10: omega-subagent - 子智能体

**Files:**
- Create: `crates/omega-subagent/Cargo.toml`
- Create: `crates/omega-subagent/src/lib.rs`

- [ ] **Step 1: 创建 Cargo.toml**

```toml
[package]
name = "omega-subagent"
version.workspace = true
edition.workspace = true

[dependencies]
omega-client = { path = "../omega-client" }
anyhow.workspace = true
tokio.workspace = true
serde_json.workspace = true
```

- [ ] **Step 2: 实现 SubAgent**

```rust
// crates/omega-subagent/src/lib.rs
use omega_client::{ChatRequest, ContentBlock, DynLlmClient, Message};
use anyhow::Result;

pub struct SubAgent {
    client: DynLlmClient,
    system: String,
    tools: Vec<serde_json::Value>,
    max_rounds: usize,
}

impl SubAgent {
    pub fn new(client: DynLlmClient, system: String, tools: Vec<serde_json::Value>) -> Self {
        Self { client, system, tools, max_rounds: 30 }
    }

    pub async fn run<F>(&self, prompt: &str, mut handler: F) -> Result<String>
    where F: FnMut(&str, serde_json::Value) -> Result<String> {
        let mut msgs = vec![Message { role: "user".to_string(), content: prompt.to_string() }];
        for _ in 0..self.max_rounds {
            let req = ChatRequest {
                system: Some(self.system.clone()),
                messages: msgs.clone(),
                tools: Vec::new(),
                max_tokens: 8000,
            };
            let resp = self.client.chat(req).await?;
            msgs.push(Message::assistant(resp.content.clone()));
            if resp.stop_reason != Some("tool_use".to_string()) {
                return Ok(resp.content.first().and_then(|c| if let ContentBlock::Text { text } = c { Some(text.clone()) } else { None }).unwrap_or_else(|| "(no summary)".to_string()));
            }
            let mut results = Vec::new();
            for block in &resp.content {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    results.push(serde_json::json!({ "type": "tool_result", "tool_use_id": id, "content": handler(name, input)? }));
                }
            }
            msgs.push(Message { role: "user".to_string(), content: serde_json::to_string(&results)? });
        }
        Ok("(max rounds)".to_string())
    }
}
```

- [ ] **Step 3: 编译验证**

```bash
cargo build -p omega-subagent
```

- [ ] **Step 4: 提交**

```bash
git add crates/omega-subagent/
git commit -m "feat(s04): add omega-subagent"
```

---

### Task 11: omega-compression - 上下文压缩

**Files:**
- Create: `crates/omega-compression/Cargo.toml`
- Create: `crates/omega-compression/src/lib.rs`

- [ ] **Step 1: 创建 Cargo.toml**

```toml
[package]
name = "omega-compression"
version.workspace = true
edition.workspace = true

[dependencies]
omega-client = { path = "../omega-client" }
anyhow.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
```

- [ ] **Step 2: 实现压缩功能**

```rust
// crates/omega-compression/src/lib.rs
use omega_client::Message;

pub fn estimate_tokens(messages: &[Message]) -> usize {
    serde_json::to_string(messages).unwrap_or_default().len() / 4
}

pub fn microcompact(messages: &mut Vec<Message>) {
    let mut tool_results = Vec::new();
    for msg in messages.iter_mut() {
        if msg.role == "user" {
            if let Ok(content) = serde_json::from_str::<Vec<serde_json::Value>>(&msg.content) {
                for item in content {
                    if let Some(obj) = item.as_object() {
                        if obj.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                            if let Some(c) = obj.get("content").and_then(|v| v.as_str()) {
                                if c.len() > 100 { tool_results.push(obj.clone()); }
                            }
                        }
                    }
                }
            }
        }
    }
    if tool_results.len() > 3 {
        for msg in messages.iter_mut() {
            if msg.role == "user" {
                if let Ok(content) = serde_json::from_str::<Vec<serde_json::Value>>(&msg.content) {
                    let new: Vec<_> = content.into_iter().map(|mut item| {
                        if let Some(obj) = item.as_object_mut() {
                            if obj.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                                if let Some(c) = obj.get("content").and_then(|v| v.as_str()) {
                                    if c.len() > 100 && !tool_results.contains(obj) {
                                        obj.insert("content".to_string(), serde_json::Value::String("[cleared]".to_string()));
                                    }
                                }
                            }
                        }
                        item
                    }).collect();
                    msg.content = serde_json::to_string(&new).unwrap_or_default();
                }
            }
        }
    }
}
```

- [ ] **Step 3: 编译验证**

```bash
cargo build -p omega-compression
```

- [ ] **Step 4: 提交**

```bash
git add crates/omega-compression/
git commit -m "feat(s06): add omega-compression"
```

---

### Task 12: omega-background - 后台任务

**Files:**
- Create: `crates/omega-background/Cargo.toml`
- Create: `crates/omega-background/src/lib.rs`

- [ ] **Step 1: 创建 Cargo.toml**

```toml
[package]
name = "omega-background"
version.workspace = true
edition.workspace = true

[dependencies]
anyhow.workspace = true
tokio.workspace = true
serde.workspace = true
uuid.workspace = true
```

- [ ] **Step 2: 实现 BackgroundManager**

```rust
// crates/omega-background/src/lib.rs
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

#[derive(Debug, Clone)]
pub struct BackgroundTask {
    pub id: String,
    pub status: TaskStatus,
    pub command: String,
    pub result: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus { Running, Completed, Error }

#[derive(Debug, Clone)]
pub struct Notification { pub task_id: String, pub status: TaskStatus, pub result: Option<String> }

pub struct BackgroundManager {
    tasks: Arc<Mutex<HashMap<String, BackgroundTask>>>,
    notification_tx: mpsc::Sender<Notification>,
}

impl BackgroundManager {
    pub fn new() -> Self {
        let (tx, _rx) = mpsc::channel(100);
        Self { tasks: Arc::new(Mutex::new(HashMap::new())), notification_tx: tx }
    }

    pub async fn run(&self, command: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        self.tasks.lock().await.insert(id.clone(), BackgroundTask { id: id.clone(), status: TaskStatus::Running, command: command.to_string(), result: None });
        let tasks = self.tasks.clone();
        let tx = self.notification_tx.clone();
        tokio::spawn(async move {
            let result = tokio::process::Command::new("sh").arg("-c").arg(command).output().await.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).ok();
            let status = if result.is_some() { TaskStatus::Completed } else { TaskStatus::Error };
            tasks.lock().await.insert(id.clone(), BackgroundTask { id: id.clone(), status: status.clone(), command: command.to_string(), result: result.clone() });
            let _ = tx.send(Notification { task_id: id, status, result }).await;
        });
        format!("Background task {} started", id)
    }

    pub async fn check(&self, task_id: Option<&str>) -> String {
        let tasks = self.tasks.lock().await;
        match task_id {
            Some(id) => tasks.get(id).map(|t| format!("[{:?}] {}", t.status, t.result.as_deref().unwrap_or("(running)"))).unwrap_or_else(|| format!("Unknown: {}", id)),
            None => if tasks.is_empty() { "No bg tasks.".to_string() } else { tasks.values().map(|t| format!("{}: [{:?}] {}", t.id, t.status, &t.command[..t.command.len().min(60)])).collect::<Vec<_>>().join("\n") }
        }
    }
}

impl Default for BackgroundManager { fn default() -> Self { Self::new() } }
```

- [ ] **Step 3: 编译验证**

```bash
cargo build -p omega-background
```

- [ ] **Step 4: 提交**

```bash
git add crates/omega-background/
git commit -m "feat(s08): add omega-background"
```

---

### Task 13: omega-team - 团队协作

**Files:**
- Create: `crates/omega-team/Cargo.toml`
- Create: `crates/omega-team/src/lib.rs`

- [ ] **Step 1: 创建 Cargo.toml**

```toml
[package]
name = "omega-team"
version.workspace = true
edition.workspace = true

[dependencies]
omega-message = { path = "../omega-message" }
omega-tasks = { path = "../omega-tasks" }
anyhow.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
```

- [ ] **Step 2: 实现 TeammateManager**

```rust
// crates/omega-team/src/lib.rs
use omega_message::MessageBus;
use omega_tasks::TaskManager;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamConfig { pub team_name: String, pub members: Vec<TeamMember> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember { pub name: String, pub role: String, pub status: String }

pub struct TeammateManager {
    config_path: std::path::PathBuf,
    config: RwLock<TeamConfig>,
    bus: Arc<MessageBus>,
    tasks: Arc<TaskManager>,
}

impl TeammateManager {
    pub fn new(team_dir: &Path, bus: Arc<MessageBus>, tasks: Arc<TaskManager>) -> anyhow::Result<Self> {
        std::fs::create_dir_all(team_dir)?;
        let config_path = team_dir.join("config.json");
        let config = if config_path.exists() { serde_json::from_str(&std::fs::read_to_string(&config_path)?)? } else { TeamConfig { team_name: "default".to_string(), members: Vec::new() } };
        Ok(Self { config_path, config: RwLock::new(config), bus, tasks })
    }

    pub async fn spawn(&self, name: &str, role: &str, _prompt: &str) -> String {
        let mut cfg = self.config.write().await;
        cfg.members.push(TeamMember { name: name.to_string(), role: role.to_string(), status: "working".to_string() });
        std::fs::write(&self.config_path, serde_json::to_string_pretty(&*cfg)?).ok();
        format!("Spawned '{}' (role: {})", name, role)
    }

    pub fn list_all(&self) -> String { "Team: default".to_string() }
}
```

- [ ] **Step 3: 编译验证**

```bash
cargo build -p omega-team
```

- [ ] **Step 4: 提交**

```bash
git add crates/omega-team/
git commit -m "feat(s09-s11): add omega-team"
```

---

### Task 14: omega-core - 核心 Agent

**Status:** Completed（2026-03-18）

**Progress:** Agent struct + run_loop + run_loop_with + create_default_tools + max_iterations guard、12 项测试、clippy 零警告

**Files:**
- Create: `crates/omega-core/Cargo.toml`
- Create: `crates/omega-core/src/lib.rs`

- [ ] **Step 1: 创建 Cargo.toml**

```toml
[package]
name = "omega-core"
version.workspace = true
edition.workspace = true

[dependencies]
omega-client.workspace = true
omega-tools.workspace = true
omega-tools-builtin.workspace = true
omega-todo.workspace = true
omega-subagent.workspace = true
omega-compression.workspace = true
omega-tasks.workspace = true
omega-background.workspace = true
omega-skills.workspace = true
omega-message.workspace = true
omega-team.workspace = true
omega-worktree.workspace = true
anyhow.workspace = true
tokio.workspace = true
serde_json.workspace = true
```

- [ ] **Step 2: 实现 Agent**

```rust
// crates/omega-core/src/lib.rs
pub use omega_client::{ChatRequest, ContentBlock, DynLlmClient, LlmClient, Message, MinimaxClient, MinimaxConfig};
pub use omega_tools::ToolDispatcher;
pub use omega_todo::TodoManager;
pub use omega_subagent::SubAgent;
pub use omega_compression::{estimate_tokens, microcompact};
pub use omega_tasks::TaskManager;
pub use omega_background::BackgroundManager;
pub use omega_skills::SkillLoader;
pub use omega_message::MessageBus;
pub use omega_team::TeammateManager;
pub use omega_worktree::WorktreeManager;

use std::path::PathBuf;

pub struct Agent {
    client: DynLlmClient,
    messages: Vec<Message>,
    tools: Vec<serde_json::Value>,
    system: String,
}

impl Agent {
    pub fn new(client: DynLlmClient, system: String, tools: Vec<serde_json::Value>) -> Self {
        Self { client, messages: Vec::new(), tools, system }
    }

    pub fn add_message(&mut self, role: &str, content: &str) {
        self.messages.push(Message { role: role.to_string(), content: content.to_string() });
    }

    pub async fn run_loop<F>(&mut self, mut handler: F) -> anyhow::Result<()>
    where F: FnMut(&str, serde_json::Value) -> anyhow::Result<String> {
        loop {
            let req = ChatRequest {
                system: Some(self.system.clone()),
                messages: self.messages.clone(),
                tools: Vec::new(),
                max_tokens: 8000,
            };
            let resp = self.client.chat(req).await?;
            self.messages.push(Message::assistant(resp.content.clone()));
            match &resp.stop_reason {
                Some(r) if r == "tool_use" => {
                    let mut results = Vec::new();
                    for block in &resp.content {
                        if let ContentBlock::ToolUse { id, name, input } = block {
                            results.push(serde_json::json!({ "type": "tool_result", "tool_use_id": id, "content": handler(name, input.clone())? }));
                        }
                    }
                    self.messages.push(Message { role: "user".to_string(), content: serde_json::to_string(&results)? });
                }
                _ => break,
            }
        }
        Ok(())
    }

    pub fn get_messages(&self) -> &[Message] { &self.messages }
}

pub fn create_default_tools(root: &PathBuf) -> omega_tools::ToolDispatcher {
    use omega_tools_builtin::{BashHandler, ReadHandler, WriteHandler};
    let mut dispatcher = omega_tools::ToolDispatcher::new();
    dispatcher.register(Box::new(BashHandler));
    dispatcher.register(Box::new(ReadHandler::new(root.clone())));
    dispatcher.register(Box::new(WriteHandler::new(root.clone())));
    dispatcher
}
```

- [ ] **Step 3: 编译验证**

```bash
cargo build -p omega-core
```

- [ ] **Step 4: 提交**

```bash
git add crates/omega-core/
git commit -m "feat: add omega-core"
```

---

### Task 15: omega-tui - TUI 界面

**TODO Mapping:** `Task 15A` = 最小终端交互里程碑（M1，历史上曾实现为 REPL），`Task 15B` = `omega-tui` Ratatui 完整 TUI（M11），`Task 15C` = 交互层重构（`omega-tui` 库化与 UI 边界收敛），`Task 15C-2` = `omega-app` 单入口应用装配包，`Task 15D` = `omega-tui` 非 UI 职责剥离（新增 `omega-session` + `omega-observability`，必要时预留 `omega-interaction`），`Task 15E` = 视觉与主题基础，`Task 15F` = 可见执行工作流与运行态状态摘要基础

**Refactor Note (2026-03-20):** `Task 15C` 与 `Task 15C-2` 已完成；`omega-repl` 已退役，`omega-tui` 已成为 library-first 的 UI crate，唯一应用入口已迁移到 `omega-app`。后续主线应直接在 `omega-app -> omega-tui + omega-session + omega-observability` 的装配边界上推进。

**Progress:** `Task 15A`、`Task 15C`、`Task 15C-2` 与 `Task 15D` 已完成；`omega-session` 与 `omega-observability` 已落地，`omega-app` 也已接手唯一应用入口。剩余 `Task 15B` 与 `Task 15F` 应以“`omega-tui` 只负责 UI、`omega-app` 负责装配”作为稳定边界。

**Files:**
- Create: `crates/omega-app/Cargo.toml`
- Create: `crates/omega-app/src/lib.rs`
- Create: `crates/omega-app/src/main.rs`
- Update: `crates/omega-tui/Cargo.toml`
- Create: `crates/omega-tui/src/lib.rs`
- Update: `crates/omega-tui/src/main.rs` 或迁移后删除 binary target
- Create: `crates/omega-session/Cargo.toml`
- Create: `crates/omega-session/src/lib.rs`
- Create: `crates/omega-observability/Cargo.toml`
- Create: `crates/omega-observability/src/lib.rs`

- [x] **Step 1: 先完成 Task 15C 交互层重构**

    - 历史上的最小 REPL 已完成独立拆分并在后续收敛阶段退役
    - 已将 `omega-tui` 收敛为 library-first crate，并拆出 `src/lib.rs`
    - 当前仍保留极薄 TUI wrapper binary 作为临时入口，等待迁移到 `omega-app`
    - 后续高级 TUI 功能应建立在新的模块边界上，而不是继续堆叠在历史 `main.rs` 中

- [x] **Step 1B: 执行 Task 15C-2，新增 `omega-app` 装配包**

    - 已新建 `omega-app` 作为唯一 `main` 所在 crate
    - 已将 provider/config/cwd/runtime/tracing/session wiring 从 `omega-tui` 迁移到 `omega-app`
    - `omega-tui` 已仅保留 UI runtime、reducer、event/render 与 terminal 生命周期
    - 默认运行命令已切换为 `cargo run -p omega-app`

- [x] **Step 2: 先执行 Task 15D 非 UI 职责剥离**

    - 新建 `omega-session`，迁移 `agent_session.rs`、checkpoint/interrupt 逻辑与现有单测
    - 新建 `omega-observability`，迁移 tracing 初始化、ANSI 清洗、文件日志与 UI sink
    - `omega-tui` 改为依赖 `omega-session` 与 `omega-observability`
    - 如 `app.rs` / `event.rs` 在实现过程中继续膨胀，则按 `docs/specs/omega-tui-non-ui-extraction.md` 启动 `omega-interaction` 的第二阶段抽离
    - 验证 `omega-core` 未新增任何 TUI 专属概念

- [ ] **Step 3: 在完成 Task 15C-2 / 15D 的边界上继续 Task 15B 的 Ratatui TUI**

  - 将 Markdown 渲染、语法高亮、输入历史、搜索、会话统计等能力放在库化后的结构上实现
  - `omega-core` 仍保持前端无关

- [ ] **Step 4: 编译验证**

```bash
cargo build -p omega-app
cargo build -p omega-tui
cargo build -p omega-session
cargo build -p omega-observability
```

- [ ] **Step 5: 提交**

```bash
git add crates/omega-app/ crates/omega-tui/ crates/omega-session/ crates/omega-observability/
git commit -m "Refine app entry and tui boundaries"
```

---

### Task 15F-1: omega-workflow - 可配置四阶段工作流系统

**Status:** Completed

**Completed:** 2026-03-19

**Files:**
- Create: `crates/omega-workflow/Cargo.toml`
- Create: `crates/omega-workflow/src/lib.rs`
- Update: `crates/omega-session/src/lib.rs`
- Update: `crates/omega-tui/src/app.rs`
- Update: `crates/omega-tui/src/render.rs`
- Create: `docs/specs/omega-workflow-package.md`

- [x] **Step 1: 创建 `omega-workflow` crate 与默认配置模型**

    - 定义 canonical workflow steps：`analysis -> plan -> execute -> report`
    - 增加 `.omega/workflow.toml` 默认模板、加载、校验和 fallback 逻辑
    - 保持首期只支持线性四阶段，不引入任意 DAG

- [x] **Step 2: 将工作流阶段推进接入 `omega-session`**

    - 在 turn 生命周期中定义何时进入 `analysis`、`plan`、`execute`、`report`
    - 通过 typed `SessionUpdate` 暴露当前阶段、序号与用户可见 label
    - turn 完成或中断后正确清理当前 workflow run

- [x] **Step 3: 将当前阶段接入 TUI 底部状态栏**

    - 为底部状态带新增 workflow slot
    - 运行中显示当前阶段；空闲时隐藏或退化为 `Idle`
    - 窄终端下定义短格式退化规则

- [x] **Step 4: 编译与测试验证**

```bash
cargo build -p omega-workflow
cargo test -p omega-workflow -p omega-session -p omega-tui
```

- [ ] **Step 5: 提交**

```bash
git add crates/omega-workflow/ crates/omega-session/ crates/omega-tui/ docs/specs/omega-workflow-package.md docs/TODO.md
git commit -m "feat: add omega-workflow"
```

**Summary:** `omega-workflow` crate 已落地，包含 `WorkflowStepKind` 枚举、`WorkflowDefinition` 加载/校验/回退、`WorkflowPrompts` 外置到 `.omega/prompt/step/*.md`、`WorkflowRun` 状态机；`omega-session` 已按阶段顺序驱动四阶段执行并发送 `WorkflowStepChanged` 事件；`omega-tui` 消费事件在底部状态带显示当前阶段；6 项 workflow 单测 + session 集成测试全部通过

---

### Task 15F-2A: omega-session - 会话资产管理基础

**Status:** Completed

**Completed:** 2026-03-20

**Files:**
- Create: `crates/omega-session/src/tool_catalog.rs`
- Create: `crates/omega-session/src/skill_catalog.rs`
- Update: `crates/omega-session/src/lib.rs`
- Update: `crates/omega-core/src/lib.rs`
- Update: `crates/omega-tools/src/lib.rs`
- Update: `crates/omega-skills/src/lib.rs`
- Update: `docs/specs/omega-step-session-asset-model.md`

- [x] **Step 1: 收敛术语与边界**

    - 明确 `step` 是 workflow 的正式最小执行单元
    - 明确 tools 与 skills 属于 session 内共享资产，而不是 step 私有字段直接驱动
    - 保持 `omega-tui` 只消费状态更新，不承接资产管理逻辑

- [x] **Step 2: 实现 `SessionToolCatalog` 为独立组合型结构体**

    - 独立文件 `crates/omega-session/src/tool_catalog.rs`，独立单元测试
    - 持有默认工具名列表，提供 `resolve_for_step(request: &StepToolRequest) -> ResolvedToolSet` 纯方法
    - `ResolvedToolSet` 为排序后的工具名 `Vec<String>`，可直接传给 `Agent::set_visible_tools`
    - `Inherit` → 返回全部默认工具；`Extend(names)` → 默认 + 追加（忽略未注册名）；`Block(names)` → 默认 - 屏蔽
    - 设计为 `Arc<SessionToolCatalog>` 友好（只读 resolve，不持有可变状态），预留多消费者并发访问

- [x] **Step 3: 实现 `SessionSkillCatalog` 为独立组合型结构体**

    - 独立文件 `crates/omega-session/src/skill_catalog.rs`，独立单元测试
    - 将 task matching、显式追加、禁用装配收敛为统一接口
    - `resolve_for_step(task: &str, request: &StepSkillRequest) -> ResolvedSkillSet`
    - 保证后续 step、subagent、team 都可复用同一接口
    - 避免把 skill 装配逻辑散落在多个 runner 内

- [x] **Step 4: 为 `omega-core::Agent` 补齐动态工具切换 API**

    - 在 `ToolDispatcher` 新增 `to_schemas_filtered(&self, names: &[&str]) -> Vec<Value>`
    - 在 `Agent` 新增 `set_visible_tools(&mut self, names: Option<&[&str]>) -> Vec<String>`
    - `set_visible_tools(None)` 恢复全量工具（安全默认值）
    - 保持现有 `run_single_response`（无工具）与 `run_loop_with`（带工具）语义不变
    - 为后续 step runner 提供稳定依赖点

- [x] **Step 5: 将 `AgentSession` 改为组合持有 catalog**

    - `AgentSession` 新增 `tool_catalog: Arc<SessionToolCatalog>` 与 `skill_catalog: Arc<SessionSkillCatalog>` 字段
    - 保持现有默认工具和技能行为不变（首轮不改变运行时行为，只建立接口）
    - `WorkflowTurnRunner` 内暂时仍走原 path，但可调用 catalog.resolve 做 assertion 验证

- [x] **Step 6: 验证与文档更新**

```bash
cargo test -p omega-tools -p omega-core -p omega-session -p omega-skills
cargo clippy -p omega-tools -p omega-core -p omega-session -p omega-skills --all-targets -- -D warnings
```

**Summary:** `omega-session` 已新增 `SessionToolCatalog` 与 `SessionSkillCatalog` 两个独立组合型结构体，并通过 `Arc` 持有到 `AgentSession`；`omega-tools::ToolDispatcher` 已新增 `to_schemas_filtered`，`omega-core::Agent` 已新增 `set_visible_tools` 与 `visible_tool_names`，可按 step 解析结果切换本轮对模型可见的工具子集；当前固定四阶段 runner 已接入 catalogs，但仍保持既有四阶段行为不变，为 15F-2B 的 step 泛化提供稳定依赖点；相关单测与 clippy 验证均已通过

---

### Task 15F-2B: omega-workflow - 通用 Step 编排接入会话资产层

**Status:** Completed

**Completed:** 2026-03-20

**Files:**
- Update: `crates/omega-workflow/src/lib.rs`
- Update: `crates/omega-session/src/lib.rs`
- Update: `crates/omega-tui/src/app.rs`
- Update: `crates/omega-tui/src/render.rs`
- Update: `docs/specs/omega-step-session-asset-model.md`
- Update: `docs/specs/omega-workflow-package.md`

- [x] **Step 1: 泛化 `WorkflowStep` 内部模型（enum → string-keyed）**

    - 将 `WorkflowStep.kind: WorkflowStepKind` 改为 `WorkflowStep.id: String` + `WorkflowStep.loop_mode: StepLoopMode`
    - 保留 4 个 canonical id（`analysis`, `plan`, `execute`, `report`）作为内建默认值
    - 将 `WorkflowPrompts` 从固定 4 字段改为 `HashMap<String, String>`，内建默认仍覆盖 4 个 canonical 的 prompt
    - TOML 配置校验**仍只允许** 4 个 canonical id（向后兼容，开放自定义 id 留到独立后续任务）
    - 移除对 `WorkflowStepKind` 枚举的硬编码依赖点（`prompt_for`、`default_label`、`file_stem`、`build_step_system_prompt`）

- [x] **Step 2: 在 `WorkflowStepDefinition` 中增加 tool/skill request 字段**

    - 在 `WorkflowStep`（或新 `WorkflowStepDefinition`）中加入 `StepToolRequest` 与 `StepSkillRequest`
    - 定义 `StepLoopMode { SingleResponse, ToolLoop }` 枚举
    - 更新 `.omega/workflow.toml` 格式，新增 `loop_mode`、`tool_request`、`skill_request` 字段
    - 提供从当前固定 step 表达到通用 step definition 的兼容映射

- [x] **Step 3: 将 `omega-session` 改为通用 step runner**

    - 移除 `WorkflowTurnRunner::run` 中对 `WorkflowStepKind::Execute` 的硬编码分支
    - 按 `loop_mode` 选择单次响应或工具循环
    - 通过 `SessionToolCatalog::resolve_for_step` 解析工具集，调用 `agent.set_visible_tools` 切换
    - 通过 `SessionSkillCatalog::resolve_for_step` 解析技能集，注入到 system prompt
    - 采用 `WorkflowRun` 作为编排运行时容器（替代当前的直接 iterator），使用 `current_step()`/`advance()` 驱动

- [x] **Step 4: 稳定 step 事件协议，增加 `step_id`**

    - `SessionUpdate::WorkflowStepChanged` 新增 `step_id: String` 字段
    - `step_id` 对应配置中的 id，用于日志/调试/程序化匹配
    - `step_label` 继续仅用于 TUI 展示
    - 更新 `omega-tui` 事件消费代码适配新字段

- [x] **Step 5: 明确 `context` 延后，不纳入首轮实现**

    - 保持 `context` 仅为保留能力
    - 后续单独起规格设计 artifact/context 模型
    - 避免首轮任务同时承担 step 泛化与 context 设计

- [x] **Step 6: 验证与文档更新**

```bash
cargo test -p omega-workflow -p omega-session -p omega-tui -p omega-core -p omega-skills
cargo clippy -p omega-workflow -p omega-session -p omega-tui -p omega-core -p omega-skills --all-targets -- -D warnings
```

**Summary:** `omega-workflow` 已把内部 step 模型泛化为 string-keyed `WorkflowStep`，并在 step definition 中正式纳入 `prompt_path`、`StepLoopMode`、`StepToolRequest`、`StepSkillRequest`；`WorkflowPrompts` 已改为按 `step_id` 查找的映射结构，仍保持 4 个 canonical step 的 TOML 兼容与默认 prompt 写入。`omega-session` 现通过 `WorkflowRun` 驱动整轮 step 编排，不再对 `execute` 做枚举分支特判，而是按 `loop_mode` 选择单响应或工具循环，并结合 `SessionToolCatalog` / `SessionSkillCatalog` 解析每个 step 的能力集。`SessionUpdate::WorkflowStepChanged` 已增加稳定的 `step_id` 字段，`omega-tui` 已适配新的事件协议；相关测试与 `clippy -D warnings` 均已通过。

---

### Task 15F-3: omega-session - 统一 runtime UI 消息与效果协议

**Status:** Completed

**Files:**
- Planned: `crates/omega-session/src/runtime_ui.rs`
- Planned: `crates/omega-session/src/lib.rs`
- Planned: `docs/specs/omega-runtime-ui-message-contract.md`
- Planned: `docs/specs/omega-tui-runtime-experience.md`

- [x] **Step 1: 定义统一 runtime UI envelope 与所有子类型**

    - 新增 `RuntimeUiEnvelope = Message | Effect`
    - `RuntimeUiMessage` 包含 `target: UiTarget`、`source: UiSource`、`kind: UiMessageKind`、`content: UiContent`、`priority: Option<UiPriority>`
    - `RuntimeUiEffect` 包含 `SetStatusSlot`、`ClearStatusSlot`、`ReplacePanel`、`ShowOverlay`、`HideOverlay`、`FocusHint`（不含 `AppendMessage` 和 `Invalidate`）
    - 补齐所有子类型定义：`UiContent(Text)`、`UiPriority(Normal/Low/High)`、`ActivityTarget(Log)`、`StatusSlot(Workflow/Agent/Session)`、`StatusValue(Label/Hidden)`、`OverlayTarget`、`OverlayRequest`

- [x] **Step 2: 定义 bridge / sink trait 与 session context**

    - 新增 `RuntimeUiBridge` trait（`fn send(&self, envelope: RuntimeUiEnvelope)`）
    - 新增 `RuntimeUiSink` trait（`fn try_recv(&self) -> Option<RuntimeUiEnvelope>`）
    - 首轮实现基于 `mpsc::Sender`/`Receiver`
    - `SessionRuntimeContext` 首轮仅包含 `ui_bridge: Arc<dyn RuntimeUiBridge>`
    - 保持显式注入，不引入隐藏式全局 singleton / service locator

- [x] **Step 3: 一步替换 `SessionUpdate` → `RuntimeUiEnvelope`**

    - 按映射表将 7 个 `SessionUpdate` variant 逐一转为 envelope 发送
    - `ToolCallPreview` → `Message { target: Activity(Log), source: Tool, kind: Log }`
    - `TodoSnapshot` → `Effect::ReplacePanel { target: Todo }`
    - `WorkflowStepChanged` → `Effect::SetStatusSlot { slot: Workflow }` + `Message { target: Activity(Log), kind: Log }`
    - `StepText` → `Message { target: Response, source: WorkflowStep, kind: Narrative }`
    - `AssistantText` → `Message { target: Response, source: Assistant, kind: Result }`
    - `TurnFinished` → `Effect::ClearStatusSlot { slot: Workflow }` + `Effect::SetStatusSlot { slot: Agent, value: Idle }`
    - 废弃 `SessionUpdate` enum，不保留双通道过渡期
    - `omega-tui` 直接消费 `RuntimeUiEnvelope`

- [x] **Step 4: 文档与验证**

```bash
cargo test -p omega-session -p omega-tui
cargo clippy -p omega-session -p omega-tui --all-targets -- -D warnings
```

**Summary:** 该任务已完成：`omega-session` 新增 `RuntimeUiEnvelope`、`RuntimeUiMessage`、`RuntimeUiEffect`、`UiTarget`、`UiSource`、`UiMessageKind` 等 typed contract，以及 `RuntimeUiBridge` / `RuntimeUiSink` trait 和 `SessionRuntimeContext` 边界；workflow phase、step text、tool preview、todo snapshot、assistant reply 与 turn finish 已全部一步迁移出 `SessionUpdate`，`omega-tui` 现直接消费 envelope 通道。验证已通过 `cargo test -p omega-session -p omega-tui -p omega-app`。

**Current Path Note:** 本任务应以当前真实主链路 `omega-tui shell -> omega-session -> omega-core` 为准推进；同时文档与 API 设计要为目标装配路径 `omega-app -> omega-tui + omega-session + omega-observability` 预留稳定边界。

---

### Task 15F-4: omega-workflow - scene catalog 与 workflow routing 模型

**Status:** Completed

**Completed:** 2026-03-20

**Files:**
- Updated: `crates/omega-workflow/src/lib.rs`
- Added: `.omega/scenes.toml`
- Added: `.omega/workflows/root.toml`
- Added: `.omega/workflows/chat.toml`
- Added: `.omega/workflows/feature.toml`
- Added: `.omega/prompt/step/scene-recognition.md`
- Added: `.omega/prompt/step/select-workflow.md`
- Added: `.omega/prompt/step/chat.md`
- Updated: `.omega/workflow.toml`

- [x] **Step 1: 定义 scene 与 workflow catalog 模型**

    - 新增 `SceneDefinition`，至少包含 `id`、`label`、`workflow_id`
    - 新增 root workflow / child workflow 的 catalog 结构
    - 固定内建 `root`、`chat`、`feature` 三个 workflow preset

- [x] **Step 2: 定义配置布局与兼容迁移策略**

    - 推荐新增 `.omega/scenes.toml` 与 `.omega/workflows/*.toml`
    - 旧 `.omega/workflow.toml` 在迁移期继续作为 `feature` workflow 的兼容来源
    - scene/workflow 配置非法时回退到内建 `feature` scene + `feature` workflow

- [x] **Step 3: 新增主路由 workflow 默认 step**

    - 新增 `scene-recognition` step prompt
    - 新增 `select-workflow` step prompt
    - 新增 `chat` workflow 默认单 step prompt

- [x] **Step 4: 为 future step-level delegation 预留模型**

    - 当前先不开放任意 step 触发 child workflow
    - 但 workflow model 必须为 `StartWorkflow { workflow_id }` 之类的过渡语义预留稳定接口
    - 当前通过显式 `SceneCatalog` / `WorkflowCatalog` / `LoadedWorkflowCatalog` 边界先把 root router workflow 与 child workflow catalog 独立出来，为 `omega-session` 下一步维护 routing state 与 workflow stack 留出稳定输入面

- [x] **Step 5: 验证**

```bash
cargo test -p omega-workflow
cargo clippy -p omega-workflow --all-targets -- -D warnings
```

**Summary:** 已在 `omega-workflow` 中落地 `SceneCatalog`、`WorkflowCatalog`、`WorkflowPromptCatalog` 与 `LoadedWorkflowCatalog`，把 `root/chat/feature` workflow preset 和 `chat/feature` scene preset 收敛为稳定模型；默认配置文件已补齐到 `.omega/scenes.toml`、`.omega/workflows/*.toml` 与新增 step prompt，旧 `.omega/workflow.toml` 继续作为 `feature` workflow 的兼容来源。`WorkflowDefinition::load()` 目前仍返回 `feature` workflow，确保 `omega-app` 与 `omega-session` 可在不破坏现有调用面的前提下继续推进 `Task 15F-5`。

---

### Task 15F-5: omega-session - scene recognition 与 child workflow delegation

**Status:** Completed

**Completed:** 2026-03-20

**Files:**
- Updated: `crates/omega-session/src/lib.rs`
- Updated: `crates/omega-app/src/lib.rs`
- Updated: `docs/TODO.md`

- [x] **Step 1: 在 session 中执行 root workflow**

    - turn 开始后先运行 `scene-recognition`
    - 再运行 `select-workflow`
    - 不把 scene recognition 写成 session 外围的预处理特例

- [x] **Step 2: 实现 child workflow delegation**

    - `select-workflow` 根据 recognized scene 选择 child workflow
    - session 启动 child workflow run，并把它作为当前稳定执行流
    - child workflow 完成后 turn 才结束

- [x] **Step 3: 维护 workflow routing runtime state**

    - 显式跟踪 current scene、selected workflow 与 active child workflow
    - 为 future nested workflow / workflow stack 预留状态结构

- [x] **Step 4: 对接 runtime-visible 路由状态**

    - 向 runtime UI contract 发出 scene / workflow selection 的可见状态
    - 区分 root workflow step 与 child workflow step 的可见语义

- [x] **Step 5: 验证**

```bash
cargo test -p omega-session -p omega-workflow -p omega-app
cargo clippy -p omega-session -p omega-app -p omega-workflow --all-targets -- -D warnings
```

**Summary:** `omega-session` 已改为从 `SceneCatalog` / `WorkflowCatalog` 驱动 scene-aware turn orchestration：turn 先执行 `root` workflow 的 `scene-recognition` 与 `select-workflow`，再把选中的 child workflow 作为真正的稳定执行流。实现中还补上了 routing runtime state、scene/workflow fallback、root routing step 输出回滚，以及通过 `StatusSlot::Session` 与 Activity 日志发出的 scene / workflow 可见状态，为后续 `Task 15B-19` 的 TUI 明确化展示提供了稳定输入面。

---

### Task 15B-19: omega-tui - scene / workflow routing 可见性

**Status:** Pending

**Files:**
- Planned: `crates/omega-tui/src/app.rs`
- Planned: `crates/omega-tui/src/reducer.rs`
- Planned: `crates/omega-tui/src/render.rs`
- Planned: `docs/specs/omega-tui-runtime-experience.md`

- [ ] **Step 1: 增加 scene / workflow 状态展示**

    - 在底部状态带展示当前 scene 与 active workflow 摘要
    - 保持窄终端可退化为短格式

- [ ] **Step 2: 区分 root workflow 与 child workflow 可见性**

    - 用户需要看见当前在 `scene-recognition` / `select-workflow`
    - 也需要看见真正执行中的 child workflow

- [ ] **Step 3: 收敛到现有 reducer / runtime UI contract**

    - 继续沿 `RuntimeUiEnvelope` + `TuiUpdateReducer` 扩展
    - 不重新引入 feature-specific UI 分支

- [ ] **Step 4: 验证**

```bash
cargo test -p omega-tui -p omega-session -p omega-app
cargo clippy -p omega-tui --all-targets -- -D warnings
```

**Summary:** 该任务用于把 scene-aware routing 从“后台路由逻辑”变成用户可确认的运行态信息，并保持可见性继续通过统一 reducer 与状态槽位扩展，而不是重新退回零散特例映射。

---

### Task 15B-18: omega-tui - 统一 runtime UI sink / reducer

**Status:** Completed

**Files:**
- Planned: `crates/omega-tui/src/app.rs`
- Planned: `crates/omega-tui/src/runtime.rs`
- Planned: `crates/omega-tui/src/render.rs`
- Planned: `docs/specs/omega-tui-runtime-experience.md`

- [x] **Step 1: 建立 TUI runtime UI reducer**

    - 已新增 `TuiUpdateReducer`，按 `target` / `kind` / `source` 把 envelope 映射到 `App`
    - 已消除原先分散在 `App` 中的 feature-by-feature envelope 分支

- [x] **Step 2: 收敛固定 surface 路由规则**

    - `Response`、`Activity`、`Todo`、`StatusBar`、`Overlay` 的落点规则已集中到 reducer
    - workflow step 正文结果、assistant reply、tool preview、warning、summary 等当前已接通样式映射已集中化

- [x] **Step 3: 为 future style variants 预留扩展点**

    - 当前 reducer 已按 `source` / `kind` 保留映射层，为后续不同 rendering preset 提供稳定入口
    - `StatusSlot` / `OverlayTarget` / `UiTarget` 与 reducer helper 已为 step block、markdown-aware rendering、theme-driven variant 预留稳定接口

- [x] **Step 4: 验证**

```bash
cargo test -p omega-tui
cargo clippy -p omega-tui --all-targets -- -D warnings
```

**Summary:** 该任务已完成：`omega-tui` 新增 `reducer.rs` 与 `TuiUpdateReducer`，把 `RuntimeUiEnvelope` 对 `Response`、`Activity(Log)`、`Todo`、`StatusBar`、`Overlay` 的当前落点规则集中到单一 reducer；`App` 只保留通用状态变更 helper 与 UI 状态本身，底部状态带已支持额外 `Session` slot，overlay show/hide 与 focus hint 也通过统一 effect 路由处理。验证已通过 `cargo test -p omega-tui -p omega-session -p omega-app` 与 `cargo clippy -p omega-tui --all-targets -- -D warnings`。

---

### Task 16: 最终整合测试

- [ ] **Step 1: 完整编译**

```bash
cargo build
```

- [ ] **Step 2: 运行测试**

```bash
cargo test
```

- [ ] **Step 3: 最终提交**

```bash
git add .
git commit -m "feat: omega agent complete - all 16 tasks implemented"
```
