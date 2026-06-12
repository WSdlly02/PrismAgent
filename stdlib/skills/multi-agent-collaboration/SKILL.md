---
name: multi-agent-collaboration
description: >
  PrismAgent 的多 agent 协作框架。该 skill 描述了 prismagentd 运行时的工作原理、
  工作流生命周期、以及各角色（profile）在协作中的职责分工。
  不同角色的 agent 应阅读 reference/ 目录下对应的 {profile}.md 文件获取详细指引。
  Use when user asks about making a plan, multi-agent collaboration, workflow design, or expressed similar intentions.
scope: global
---

# Multi-Agent Collaboration Skill

## 快速上手

Agent 在收到任务后，先用 `prismagent_skill_dir_get` 找到本 skill 目录，再用 `fs_file_read` 阅读 `reference/{profile}.md`。该 reference 是对应 profile 的单一事实源。

通用硬规则：

- 只使用当前 profile 暴露的工具，不假设隐藏能力。
- 不编造 UUID；需要新对象时先用 `prismagent_uuid_generate`。
- `context_refs` 是输入，创建 agent 时自动注入 prompt。
- `context_out` 是输出契约，Executor/Verifier 必须创建这些 context。
- Planner 不是 auto_loop，不调用 `prismagent_task_finish`。
- Executor/Verifier 是 auto_loop，完成全部 `context_out` 后必须调用 `prismagent_task_finish`。

角色入口：

- Default: `reference/default.md`
- Planner: `reference/planner.md`
- Executor: `reference/executor.md`
- Verifier: `reference/verifier.md`

Workflow 是 TOML DAG 定义，由 WorkflowActor 代码引擎解析执行。不创建 LLM Coordinator，不设 Trigger。

## 系统概述

prismagentd 是一个基于 actor 模型的异步 agent 运行时。Agent 通过 LLM（大语言模型）驱动，通过工具调用与文件和外部服务交互。

### 核心概念

- **Workspace** — 一个工作区目录，包含一个 `.prismagent/` 子目录，所有数据（agents、contexts、workflows、units）存储在此。
- **Agent** — 一个由 LLM + profile 配置驱动的对话实体。每个 agent 有一个唯一的 UUID。
- **Profile** — 定义 agent 的角色、可用工具、系统提示词模板和模型配置。
- **Context** — 工作流中传递的结构化文档。语义：
  - `context_refs`：需要读取的 context UUID 列表（输入），创建 agent 时自动注入 prompt。
  - `context_out`：需要产出的 context UUID 列表（输出声明）。Executor/Verifier 调用 `task_finish` 时会检查这些 context 是否存在；Planner 不调用 `task_finish`。
- **Workflow** — TOML DAG 定义，包含 `[workflow]`、`[[context]]`、`[[agent]]`、`[[step]]` 四部分。由 WorkflowActor（代码引擎）解析执行。
  - `[[step]]` 的 `needs` 字段表达 DAG 依赖边
  - engine 按拓扑序创建 agent，等待 context_out 全部产出后自动推进
  - engine 不解释 context 内容，不处理条件分支
- **Unit** — 对话中的一条消息。每个 agent 的对话历史由一系列 units 组成。

### 工作流生命周期

```
Planner 创建 Context + Workflow (TOML)
       ↓
prismagent_workflow_start → WorkflowActor 解析 TOML
       ↓
校验：UUID、注册表、context 溯源、graph 无环
       ↓
启动 needs=[] 的 step（创建 agent）
       ↓
Agent 执行 → 产出 context_out → prismagent_task_finish
       ↓
engine 检测 step 完成 → 推进下游 step
       ↓
全部 step 完成 → pipe final_piped_contexts 给 Planner
       ↓
Planner 决定是否迭代（创建新的 Workflow）
```

### 迭代优于循环

工作流内部不支持无限循环。需要迭代时：

1. Workflow-1 执行 → Verifier 产出 VERDICT: REJECTED（这是 context 内容，engine 不关心）
2. WorkflowActor 把 `final_piped_contexts` 给 Planner
3. Planner 查看 context 内容，决定是否需要迭代
4. 如果需要，创建新的 Workflow-2，重新启动

每次迭代是独立工作流，由 Planner 统筹协调。

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
assets/
└── workflow-example.toml  — 工作流示例文件，展示了 TOML 定义的结构和语法
reference/
├── default.md       — 通用助手角色的工作方式
├── planner.md       — 规划者的工作方式
├── executor.md      — 执行者的工作方式
└── verifier.md      — 验证者的工作方式
```

使用 `prismagent_skill_dir_get` 工具获取本 skill 目录的路径，然后用 `fs_file_read` 阅读对应的参考文档。
