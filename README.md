# PrismAgent

PrismAgent 是一个以本地工作区为中心的 AI Agent 应用。它使用 Rust 与 Tokio Actor 构建运行时，通过 React Web UI 提供流式对话、工具审批和多 Agent 工作流，让 Agent 在实际项目目录中读取文件、执行命令并协作完成任务。

项目仍在迭代中，配置格式、接口和持久化结构可能变化。

## 功能

- **工作区与会话**：注册本机项目目录，在工作区内创建、切换和管理 Agent。
- **流式交互**：通过 WebSocket 接收模型输出、工具调用与运行状态。
- **可配置角色**：使用 TOML Profile 定义模型、系统提示词、可用工具和审批策略；内置 `default`、`planner`、`executor`、`verifier`。
- **工具执行**：提供文件操作、Shell 命令、网页搜索与抓取，以及工作流相关工具。
- **多 Agent 工作流**：使用 TOML DAG 描述步骤依赖，通过 Context 传递任务与产出。
- **本地持久化**：在 `~/.prismagent/` 保存配置、会话与工作流数据。
- **嵌入式 Web UI**：构建后将前端资源打包进 `prismagentd`，运行时无需单独启动前端服务。

## 快速开始

### 环境要求

- Rust 工具链，支持 Edition 2024；本项目尚未声明最低 Rust 版本。
- Node.js `20.19+`（20.x）或 `22.12+`，以及 npm，要求来自当前 Vite 依赖。
- 可用的模型服务 API Key。
- 使用文件读取、目录查看和 Shell 等工具时，需要在后端进程的 `PATH` 中提供 `rtk`。当前代码调用 `rtk read`、`rtk tree`、`rtk ls`、`rtk stat` 和 `rtk run -c` 等命令；仓库尚未固定其安装来源与兼容版本。
- 使用 `web_search` / `web_fetch` 时，额外需要 `TINYFISH_API_KEY`。

### 构建

```bash
git clone https://github.com/WSdlly02/PrismAgent.git
cd PrismAgent
npm --prefix web ci
npm --prefix web run build
cargo build --release --locked
```

必须先构建前端，再构建 Rust 二进制。缺少 `web/dist/index.html` 时，后端构建脚本只会生成提示页面，不会自动构建完整 UI。修改前端后也需要按此顺序重新构建。

### 启动

内置 `default` Profile 当前配置为 `deepseek` / `deepseek-v4-flash`，从 `DEEPSEEK_API_KEY` 环境变量读取密钥。请确认自己的服务账号支持该模型，或按下文修改 Profile。

在 Bash 中输入密钥并启动：

```bash
read -rsp 'DeepSeek API key: ' DEEPSEEK_API_KEY
echo
export DEEPSEEK_API_KEY
PRISMAGENTD_ADDR=127.0.0.1:7618 ./target/release/prismagentd
```

打开 <http://127.0.0.1:7618>，添加一个**后端所在机器上已存在的项目目录**，创建 Agent 并选择 `default` Profile，即可开始对话。需要规划多个协作步骤时，可以创建 `planner` Agent。

后端不会自动加载仓库中的 `.env` 文件。其他 API Key 同样需要在启动进程前通过环境变量传入。

可以用以下命令检查 HTTP 服务是否可达；它不检查模型服务或工具是否可用：

```bash
curl http://127.0.0.1:7618/health
```

## 配置与数据

首次启动会将缺失的内置 Profile 和 Skill 写入用户目录，已有文件不会被覆盖。

```text
~/.prismagent/
├── profiles/       # 模型、提示词与工具策略
├── skills/         # Skill 文档及相关资源
└── workspaces/     # 按工作区 UUID 保存元数据和运行数据
```

修改 `~/.prismagent/profiles/default.toml` 的模型配置时，保留文件中的其他配置节：

```toml
[model]
provider = "deepseek"
model_name = "deepseek-v4-flash"
api_key_env = "DEEPSEEK_API_KEY"
```

`api_key_env` 填写环境变量名，不填写密钥本身。Profile 的 `name` 应与 TOML 文件名（不含扩展名）一致。修改已有 Profile 后重启后端以使配置生效；升级程序不会自动覆盖已有 Profile。

完整配置参考 [内置 Profiles](stdlib/profiles)。工作流结构参考 [TOML 示例](stdlib/skills/multi-agent-collaboration/assets/workflow-example.toml)，角色协作约定见 [协作 Skill](stdlib/skills/multi-agent-collaboration/SKILL.md)。示例中的 Agent 和 Context 引用需要与实际运行状态对应。

## 运行边界

PrismAgent 的工具以启动后端的系统用户权限运行，工作区路径不是操作系统沙箱。文件和命令操作可能影响工作区以外的路径。

工具审批由 Profile 控制：`available_tools` 定义可用工具，`auto_approve` 指定自动批准的工具，`yolo = true` 会跳过逐次审批。当前内置 `executor` 和 `verifier` 开启了 `yolo`，使用前应检查各自的工具权限。

`pub` 用于可选的进度说明，例如 `{"command":"检查工作区与 SSH 连接"}`。它只发布文字，不执行命令，也不代表任务完成。网页收到完整工具调用后，将 `command` 按普通 assistant 消息展示，支持 Markdown、复制和历史定位；成功回执不重复展示，错误仍显示为工具结果。既有本地 Profile 不会被内置模板覆盖，需要在其 `available_tools` 和 `auto_approve` 中加入 `"pub"` 后重启服务。

`pub` 的入口 schema 使用严格模式并声明 `additionalProperties: false`，要求模型只输出约定字段。执行侧只校验 `command` 是否为非空字符串；如果模型仍返回 `depth`、`path` 等额外字段，整个参数对象会原样保存在 Unit 中，前端仍正常展示 `command`。

`fs_file_read` 自动识别 PNG、JPEG、WebP 图片，无需新增参数。图片上限为 10 MiB、2500 万像素、单边 16384 像素；读取时校验图片并保存原始内容，历史回放不重新读取源文件。工具结果显示元数据和可放大的图片预览，文本文件继续使用 `rtk read`。

工具图片保存在 Unit 的 `tool_images` 中，不塞入文本工具回执。构造 LLM 请求时，先发送整批 tool 回执，再追加携带来源路径与 `call_id` 的多模态 user 消息；OpenAI 兼容接口将图片编码为 `image_url` data URL。这个 user 消息仅用于协议适配，不会作为用户发言落盘或显示。所用模型和 API 需要支持视觉输入。

后端默认监听 `0.0.0.0:7618`，允许回环与指定私网 IP 段访问；当前没有面向公网部署的用户认证机制。上面的启动命令将监听范围收窄到本机。工作区写入租约用于协调客户端，不应视为用户身份认证。

本地保存会话不意味着离线推理：模型请求会发送给配置的服务商，网页工具会调用 TinyFish。请按实际任务决定可以交给这些服务的内容。

## 开发

后端和前端分别在两个终端运行，后端终端需先设置模型所需的环境变量：

```bash
# 终端 1：仓库根目录
PRISMAGENTD_ADDR=127.0.0.1:7618 cargo run --locked --bin prismagentd
```

```bash
# 终端 2：仓库根目录
npm --prefix web ci
npm --prefix web run dev
```

打开 Vite 输出的本机地址。开发服务器将 `/api`、`/ws` 和 `/health` 代理到 `127.0.0.1:7618`；若修改后端端口，也需调整 [Vite 配置](web/vite.config.ts)。

已有检查命令：

```bash
cargo test --locked
npm --prefix web run test -- --run
npm --prefix web run build
```

## 代码结构

```text
src/bin/prismagentd.rs  HTTP / WebSocket 入口与 Actor 启动
src/actors/            Agent、Context、Workflow、工具、模型等 Actor
src/handles.rs         Actor 间通信句柄
src/stdlib_assets.rs   内置 Profile 与 Skill 的嵌入和初始化
src/web_assets.rs      Web 静态资源服务
stdlib/                内置角色配置与协作 Skill
web/                   React + TypeScript + Vite 前端
build.rs               构建资源准备
```

请求经 ShellActor 进入运行时，AgentActor 协调模型推理和工具执行；WorkflowActor 调度步骤，ContextActor 管理共享上下文，StorageActor 负责会话等数据的文件持久化。各 Actor 通过 Tokio channel 交换消息。

## 参与贡献

欢迎通过 [Issues](https://github.com/WSdlly02/PrismAgent/issues) 报告问题或讨论功能，并提交 Pull Request。问题报告请附上复现步骤、工具链版本和去除敏感信息后的日志；涉及行为变化的提交请说明验证方式。

请勿在 Issue、日志或提交中包含 API Key、个人会话或私有工作区内容。

## 许可证

本项目采用 [MIT License](LICENSE)。
