---
name: multi-agent-collaboration
description: >
  PrismAgent 的多 agent 协作框架。该 skill 描述了 prismagentd 运行时的工作原理、
  工作流生命周期、以及各角色（profile）在协作中的职责分工。
  不同角色的 agent 应阅读 reference/ 目录下对应的 {profile}.md 文件获取详细指引。
scope: global
---

# Multi-Agent Collaboration Skill

## 快速上手

Agent 在收到任务后，按以下步骤执行。这些是硬规则，不可跳过：

### 所有角色通用

1. **只使用当前 profile 暴露的工具** — 不要引用或假设自己没有的工具。
2. **不要编造 UUID** — 需要新对象时先用 `prismagent_uuid_generate` 预分配。
3. **把 UUID 写入文档正文** — 下游 agent/coordinator 依赖 context、workflow、trigger、agent UUID 做确定性协作。
4. **先阅读对应 reference 文档** — 使用 `prismagent_skill_dir_get` 找到本 skill 目录，再读取 `reference/{profile}.md`。

### Worker 角色通用

仅 Executor/Verifier 属于 worker 角色：

1. 收到任务后立即用 `prismagent_self_show` 查看 `context_out` 列表。
2. 用 `prismagent_context_create` 产出每个指定的 context，UUID 必须精确匹配 `context_out`。
3. 完成所有 `context_out` 后调用 `prismagent_task_finish` 关闭 auto_loop。

Planner/Coordinator 不是 worker，不产出普通任务结果，不调用 `prismagent_task_finish`。

### auto_loop 语义

- **Executor/Verifier**：`auto_loop=true`，系统会自动发送消息确保持续工作。
- **task_finish**：用于关闭 auto_loop，表示任务完成。Executor/Verifier 完成任务后必须调用。
- **Planner/Coordinator**：`auto_loop=false`，不调用 task_finish，通过事件驱动（用户输入/trigger 触发）。

### Planner

- 规划前先 `prismagent_profile_list` 看有哪些角色可用。
- 简单任务直接回答（单步查询、单文件修改）。复杂任务（plan→execute→verify 闭环、多步依赖）创建工作流。
- 不确定时优先建工作流。
- **创建 Context**：为每个下游 agent 创建任务说明书，声明 context_out（本 agent 需要产出的 context UUID）。
- **创建 Workflow**：纯控制流描述（Mermaid + 文字），包含所有 UUID 和推进指导，不包含业务细节。
- **启动工作流**：用 `prismagent_workflow_start` 启动 → 自动创建 coordinator，workflow 内容自动注入。
- **接收结果**：工作流完成后，coordinator 通过 `piped_context_out` 发送最终结果。
- **迭代优化**：如果结果不满意，根据反馈创建新的 Context/Workflow 继续。

### Coordinator

- **收到 Workflow**：内容已自动注入你的上下文，理解其中的 UUID 和推进指导。
- **创建 Agent**：根据 Workflow 中的注册表创建 agent，设置 `context_refs`（需要读取的 context）和 `context_out`（需要产出的 context UUID）。
- **创建 Trigger**：监控 context 创建事件，OR 语义（任一 context 创建即触发）。
- **推进逻辑**：收到 trigger → 记录已触发的 context → 检查 agent 依赖是否满足 → 满足则推进下一步。
- **查询状态**：不确定时用 `prismagent_agent_list` 查看所有 agent 状态和 context 存在性。
- **汇报结果**：工作流完成后，用 `prismagent_agent_message_send` + `piped_context_out` 发送最终 context 给 planner。
- **绝不**：产出 context、执行业务逻辑、在无 trigger 时空转。

### Executor

- **第一步**：`prismagent_self_show` → 确认 context_out UUID。
- **读取任务**：context_refs 中的 context 已自动注入你的 prompt。
- **执行任务**：做最小的安全变更。
- **产出结果**：`prismagent_context_create`（用正确 UUID），顶部写 `STATUS: DONE / BLOCKED / FAILED`。
- **完成任务**：`prismagent_task_finish`（系统检查 context_out 是否都存在，然后关闭 auto_loop）。
- **永远不要**：编造 context UUID、修改无关文件、在产出 context 后继续工作。

### Verifier

- **第一步**：`prismagent_self_show` → 确认 context_out UUID。
- **读取任务**：task context + result context 已自动注入你的 prompt。
- **验证**：逐条核对需求是否满足。
- **产出裁决**：`prismagent_context_create`（用正确 UUID），先写 `VERDICT: ACCEPTED / REJECTED / BLOCKED / FAILED`，再附证据。
- **完成任务**：`prismagent_task_finish`。
- **验证命令**：仅运行 `cargo check/test/build`, `go test`, `npm test` 等，不运行破坏性命令。

### Default

- 你是用户对话入口，不做工作流角色。
- 简单任务直接做。复杂任务建议用户创建 planner agent。
- 你没有 agent/workflow 创建工具，不要尝试。

---

## 系统概述

prismagentd 是一个基于 actor 模型的异步 agent 运行时。Agent 通过 LLM（大语言模型）驱动，通过工具调用与文件和外部服务交互。

### 核心概念

- **Workspace** — 一个工作区目录，包含一个 `.prismagent/` 子目录，所有数据（agents、contexts、workflows、units）存储在此。
- **Agent** — 一个由 LLM + profile 配置驱动的对话实体。每个 agent 有一个唯一的 UUID。
- **Profile** — 定义 agent 的角色、可用工具、系统提示词模板和模型配置。
- **Context** — 工作流中传递的结构化文档。语义：
  - `context_refs`：需要读取的 context UUID 列表（输入），创建 agent 时自动注入 prompt。
  - `context_out`：需要产出的 context UUID 列表（输出声明）。Executor/Verifier 调用 `task_finish` 时会检查这些 context 是否存在；Planner/Coordinator 不调用 `task_finish`。
- **Workflow** — 描述工作流的纯控制流文档（Markdown + Mermaid），包含所有 UUID 和推进指导，不包含业务细节。启动时自动注入 coordinator 上下文。
- **Trigger** — 监听 context 创建事件的机制。语义：
  - `context_uuids`：监控的 context 列表，OR 语义（任一创建即触发）。
  - `message`：触发时发送给 coordinator 的消息。
  - Coordinator 收到所有依赖的 trigger 后才推进下一步。
- **Unit** — 对话中的一条消息。每个 agent 的对话历史由一系列 units 组成。

### 迭代优于循环

工作流内部不支持无限循环。需要迭代时：

1. Workflow-1 执行 → Verifier REJECT
2. Coordinator 发送 `piped_context_out` 给 Planner
3. Planner 根据反馈创建新的 Context + Workflow-2
4. Workflow-2 执行 → Verifier ACCEPTED

每次迭代是独立工作流，由 Planner 统筹协调。

### 意外停工风险

**Planner/Coordinator 可能意外停工**（事件驱动 LLM 的固有限制）：

- Planner 在创建 workflow 之前或收到结果之后可能停止响应
- Coordinator 在等待 trigger 或推进过程中可能停止响应
- 没有自动恢复机制

**缓解措施**：

- 优化提示词：强调"完成所有要求的创建，不能遗漏"
- 任务原子化：保持每个 agent 的工作范围小而明确
- 用户监控：定期检查 agent 状态，手动发消息恢复

### 工具命名规范

所有工具遵循 `<namespace>_<resource>_<verb>` 格式：

- `fs_*` — 文件系统操作（如 `fs_file_read`、`fs_tree_list`）
- `web_*` — 网络操作（如 `web_search`、`web_fetch`）
- `shell_exec` — Shell 命令执行
- `prismagent_*` — PrismAgent 平台操作

### 文件系统

- 所有 `fs_*` 工具操作的工作目录是当前 workspace 的路径。
- 绝对路径指向系统真实路径，相对路径相对于 workspace 根目录。
- `fs_path_remove` 删除空目录需 `recursive=false`，删除非空目录需 `recursive=true`。

### 技能阅读指引

本 skill 目录下包含各 profile 的详细参考文档：

```
reference/
├── default.md       — 通用助手角色的工作方式
├── coordinator.md   — 协调者角色的工作方式
├── planner.md       — 规划者的工作方式
├── executor.md      — 执行者的工作方式
└── verifier.md      — 验证者的工作方式
```

使用 `prismagent_skill_dir_get` 工具获取本 skill 目录的路径，然后用 `fs_file_read` 阅读对应的参考文档。
