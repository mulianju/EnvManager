# EnvManager 命令别名（Command Shims）PRD

> 版本：v0.3 | 创建日期：2026-07-22 | 更新日期：2026-07-28
> 需求来源：用户沟通
> 优先级：高
> 文档状态：已确认

## 0. 工作区

### 0.1 中断恢复卡

- **当前阶段**：发布准备
- **当前状态**：实现与验证完成
- **最后完成步骤**：已完成通用 User-scope Command Shims 后端、Tauri API、桌面界面和全量验证
- **当前中断点**：无
- **下一步动作**：生成最终安装包并完成 GitHub 开源发布
- **继续前检查**：复核当前分支和未提交工作，不覆盖正在进行的 EnvManager convenience-enhancements 改动
- **最后更新**：2026-07-28
- **责任人**：用户 / Codex

### 0.2 本轮输出决策

- **输出级别**：已确认首期范围
- **选择原因**：通用 Shim、User scope、结构化参数和 Shell 覆盖范围均已确认
- **完成条件**：全量自动化、桌面端和安装包验收完成

### 0.3 来源摘要

| 来源 | 已确认信息 | 可信度 |
|------|------------|--------|
| 用户沟通 | 需要通过 `sharedev` 调用指定 Node.js 和指定 `sharedev.js` | 高 |
| Windows 命令解析规则 | 普通环境变量不能直接注册同名终端命令 | 高 |
| EnvManager 当前能力 | 产品管理 Registry 环境变量，尚未管理可执行命令 Shim | 高 |

### 0.4 设计 / 交互分析

- **是否包含设计输入**：否
- **设计分析状态**：待后续产品交互设计
- **当前可确认交互**：Command Shims 应是独立导航；新增、编辑、删除均提供明确状态和错误反馈
- **当前阻塞**：无设计稿，因此本稿只约束信息架构、操作流程和状态，不约束像素规格

### 0.5 待确认问题

| 编号 | 优先级 | 问题 | 推荐答案 | 状态 |
|------|--------|------|----------|------|
| Q1 | P0 | 首期是否做通用“可执行文件 + 固定参数”Shim？ | 是；覆盖 Node、Python 和原生 CLI | 已确认 |
| Q2 | P1 | 是否仅支持 User scope？ | 是；不引入 UAC 和全局冲突 | 已确认 |
| Q3 | P1 | 是否允许在应用内试运行？ | 首期不允许，只验证配置和生成物 | 已确认 |
| Q4 | P1 | 目标产品版本是什么？ | 下一个 minor 版本 | 待确认 |
| Q5 | P2 | 是否支持工作目录和额外环境变量？ | 首期不支持 | 已确认 |

---

# 一、需求概述

## 1.1 背景

开发工具经常以脚本形式存在，并依赖特定运行时版本。例如本需求中的 `sharedev.js` 需要由指定的 Node.js 执行。如果直接调用系统 `node`，运行结果取决于当前终端的 `PATH`，可能因为 Node 版本过旧而失败。

用户希望只输入：

```powershell
sharedev <args>
```

系统实际执行：

```powershell
& 'C:\Tools\Node\node.exe' `
  'C:\Tools\sharedev\dist\sharedev.js' `
  <args>
```

Windows 环境变量只能保存字符串，不能将变量名注册为终端命令。因此 EnvManager 需要在环境变量之外提供 Command Shim 管理能力。

## 1.2 目标

1. 使用简短、稳定的命令名调用指定可执行文件和固定参数。
2. 用户调用时附加的参数必须继续传递给目标程序。
3. 工具运行时不依赖系统默认 Node.js 或当前终端中同名运行时。
4. EnvManager 自动管理 Shim 文件及其目录的 User `PATH` 接入。
5. 所有创建、更新、删除操作可预览影响、可校验、可追踪，且不破坏用户自行维护的文件。

## 1.3 目标用户与场景

### 目标用户

- 在 Windows 上使用多个 Node.js、Python 或其他运行时的开发者。
- 需要从 PowerShell、CMD、Git Bash、IDE Terminal 或子进程稳定调用内部 CLI 的用户。

### 核心场景

1. 为本地构建产物注册短命令，如 `sharedev`。
2. 为特定版本运行时绑定脚本，避免被系统 `PATH` 中的其他版本影响。
3. 更新脚本或运行时路径后，在 EnvManager 中统一修改，不手工寻找 `.cmd` 文件。
4. 检查命令失效原因，例如运行时文件或目标脚本已被删除。

## 1.4 非目标

- 不把命令字符串保存为普通 User/System 环境变量并自动求值。
- 不提供任意 shell 文本的 `eval` / `Invoke-Expression` 能力。
- 不在首期托管脚本源码或下载运行时。
- 不替代 PowerShell `$PROFILE`、包管理器全局安装或 Windows 文件关联。
- 首期不支持系统级 Shim、工作目录、按项目生效和额外环境变量注入。

## 1.5 方案选择

### 方案 A：通用 Command Shim（推荐）

数据模型是“命令名 + 可执行文件 + 固定参数列表”。Node 脚本只是其中一种配置：Node 是 executable，脚本路径是第一个固定参数。

优点：模型清晰，可复用于 Node、Python、Java 和原生 CLI；不需要为不同运行时增加专用字段。缺点：参数编辑和校验比 Node 专用表单更复杂。

### 方案 B：仅 Node 脚本命令

字段固定为命令名、Node 路径和 JavaScript 路径。

优点：表单简单、校验精确。缺点：能力过窄，后续支持 Python 等场景时需要迁移数据模型和 UI。

### 方案 C：环境变量保存完整命令

不采用。它无法让 Windows 将变量名解析为命令，并会引入 shell 字符串求值、转义和命令注入问题。

---

# 二、产品方案

## 2.1 信息架构

EnvManager 主导航新增 `Command Shims`，与以下模块平级：

- User variables
- System variables
- Effective environment
- Command Shims
- Backups

Command Shims 不属于 Registry 环境变量，不出现在 Effective environment 中。它只通过一个受 EnvManager 管理的目录接入 User `PATH`。

## 2.2 命令配置模型

### 必填字段

| 字段 | 说明 | 示例 |
|------|------|------|
| Command name | 用户在终端输入的命令名 | `sharedev` |
| Executable | 实际启动的 `.exe` / `.com` 文件绝对路径 | `C:\Tools\Node\node.exe` |
| Fixed arguments | 每次调用都放在用户参数之前的参数列表 | `D:\...\dist\sharedev.js` |

### 系统维护字段

| 字段 | 说明 |
|------|------|
| Managed shim path | EnvManager 生成的 `.cmd` 与无扩展名 wrapper 路径 |
| Status | Ready、Missing executable、Missing argument path、Name conflict、Externally modified |
| Created / updated time | 用于问题定位和审计 |
| Ownership signature | 识别文件是否确由 EnvManager 创建，防止误删用户文件 |

### 待确认字段

- Description
- Working directory
- Additional environment variables
- Shell visibility（PowerShell / CMD / all）

## 2.3 创建命令

### UC-01 创建 `sharedev`

**前置条件**：用户可写入 EnvManager 的 User command directory。

**基本流程**：

1. 用户进入 `Command Shims`，点击新增。
2. 输入 command name：`sharedev`。
3. 选择 executable：指定 Node.js 的 `node.exe`。
4. 添加 fixed argument：指定 `sharedev.js`。
5. 系统展示执行预览，分别呈现 executable、固定参数和运行时参数占位符，不拼成可执行 shell 字符串。
6. 系统检查名称、路径、同名命令和目标文件冲突。
7. 用户确认保存。
8. 系统原子生成 PowerShell/CMD 与 Git Bash 受管理 wrapper，并确保 managed bin directory 位于 User `PATH`。
9. 页面显示成功状态，并提示新终端可直接调用 `sharedev`。

**后置条件**：新启动的 PowerShell、CMD、Git Bash 和 IDE Terminal 能解析 `sharedev`。

**异常流程**：

- 命令名不合法：阻止保存并定位字段。
- executable 不存在：阻止保存。
- fixed argument 是绝对路径但不存在：阻止保存；普通非路径参数不做文件存在性校验。
- managed directory 中存在非 EnvManager 所有的同名文件：禁止覆盖。
- 系统 `PATH` 中已经存在同名命令：展示冲突来源并要求用户明确处理，不静默抢占。
- User `PATH` 写入失败：不保留半完成 Shim；显示错误和恢复建议。

## 2.4 命令调用语义

调用：

```powershell
sharedev app-dev paas-app list
```

必须等价于直接启动指定 executable，并按以下顺序传参：

1. 配置中的 fixed arguments。
2. 用户调用 `sharedev` 时提供的全部 arguments。

行为约束：

- 路径包含空格、中文、括号时仍可正确调用。
- 参数中的空格和引号不能被错误拆分。
- 目标进程的 stdout、stderr 和交互输入保持连接到当前终端。
- Shim 返回码必须与目标进程退出码一致。
- 不使用系统默认 Node.js，不通过 `node` 命令二次查找。
- 不对参数执行额外 shell 求值。

## 2.5 列表与状态

列表至少展示：

- Command name
- Executable 文件名或路径摘要
- Fixed arguments 摘要
- Status
- Actions

支持按 command name、executable 和 fixed arguments 搜索。

状态规则：

| 状态 | 条件 | 用户操作 |
|------|------|----------|
| Ready | 配置、文件和受管理 Shim 一致 | 复制命令、编辑、删除 |
| Missing executable | executable 不存在 | 重新选择路径 |
| Missing target | 必须存在的固定参数文件缺失 | 编辑参数或定位文件 |
| Name conflict | `PATH` 中有其他同名命令且优先级不确定 | 查看冲突来源 |
| Externally modified | Shim 内容与记录不一致 | 查看差异、重新生成或解除管理 |

## 2.6 编辑与删除

### 编辑

- 编辑前重新检查 Shim 所有权和外部修改状态。
- 保存前展示 executable / arguments 的变化摘要。
- 使用临时文件和原子替换，失败时保留旧 Shim。
- 更改 command name 等价于安全地创建新文件并删除旧的受管理文件；任一步失败时回滚。

### 删除

- 删除前明确展示将移除的命令名和 Shim 路径。
- 仅删除所有权校验通过的受管理文件。
- 如果文件已被外部修改，不自动删除；用户可选择解除 EnvManager 记录，但保留文件。
- 删除最后一个 Shim 后保留 User `PATH` 中的 managed directory，避免反复修改 `PATH`。

## 2.7 Managed directory 与 PATH

建议使用稳定的当前用户目录，例如：

```text
%LOCALAPPDATA%\EnvManager\bin
```

要求：

- 首次创建 Shim 时，将目录以独立条目加入 User `PATH`。
- 已存在时不得重复添加。
- 不修改 System `PATH`。
- 目录位置和 Registry 写入结果需在 UI 中可见。
- 保存后通知 Windows 环境变化；已打开的终端通常仍需重启，产品必须明确提示。
- 从 EnvManager 启动的“最新环境 PowerShell”应立即识别新 Shim。

## 2.8 安全边界

- command name 仅允许 Windows 文件名和命令解析可安全支持的字符；禁止路径分隔符、控制字符和保留设备名。
- executable 必须是绝对路径，不接受一段待 shell 解析的命令文本。
- executable 首期只接受 Windows `.exe` / `.com`；脚本必须通过对应 runtime executable 和 fixed argument 调用。
- fixed arguments 必须以结构化数组保存，不以一个不可解析的 raw command string 作为唯一事实源。
- 首期不提供完整命令文本解析入口，所有 fixed arguments 逐项编辑。
- 不自动提升权限，不写 System 目录。
- 配置中可能包含敏感参数；日志、错误信息和导出能力不得默认暴露完整敏感值。
- EnvManager 只删除带有自身所有权标识且记录匹配的文件。

## 2.9 数据持久化

Command Shim 配置属于便利性元数据，不写入 Windows Registry 环境变量值。

建议与 settings 分离存储，记录结构化配置和文件校验信息。具体文件位置、schema version、锁和原子写策略在技术方案中确定。

要求：

- 多进程并发修改不能丢失更新。
- 文件损坏时不删除现有 Shim，并提供可恢复错误。
- schema 必须支持后续新增字段和迁移。
- 不保存运行时产生的 secret、token 或命令输出。

## 2.10 错误与反馈

所有错误必须显示可执行的解决信息，至少覆盖：

- executable / target 不存在或无权限读取。
- managed directory 创建失败。
- User `PATH` 读取或写入失败。
- command name 与已有 EnvManager Shim 重复。
- command name 与外部文件或 `PATH` 中其他命令冲突。
- Shim 原子写入或替换失败。
- 配置持久化失败及回滚结果。
- 外部修改导致所有权校验失败。

成功反馈必须说明：命令已创建、真实执行目标、是否需要打开新终端。

---

# 三、验收标准

## 3.1 核心验收

1. 用户能配置 `sharedev`，其 executable 为指定 Node.js，fixed argument 为指定 `sharedev.js`。
2. 在新 PowerShell、CMD 或 Git Bash 中执行 `sharedev --help` 时，实际进程使用配置的 Node.js，而非 `PATH` 中的默认 Node.js。
3. `sharedev` 后的参数按原顺序传递给脚本。
4. executable 和脚本路径包含空格时仍能正确运行。
5. 目标程序 stdout、stderr、stdin 和退出码保持正确。
6. managed bin directory 只在 User `PATH` 中出现一次。
7. 创建、编辑和重命名使用原子文件操作，失败时不留下半写文件或丢失旧命令。
8. EnvManager 不覆盖或删除非自身管理的同名文件。
9. executable 或脚本被删除后，列表能显示失效状态。
10. 外部修改 Shim 后，EnvManager 能识别差异并阻止静默覆盖或删除。
11. 保存前使用当前 Registry 合成的新终端 `PATH` 检查同名命令；Shim 保存失败时，本次新增的 User `PATH` 条目会安全回滚，且不覆盖并发外部修改。

## 3.2 冲突验收

1. 已存在同名 EnvManager Shim 时，新增操作被阻止并引导编辑。
2. managed directory 中存在外部同名无扩展名文件或 `.cmd` / `.bat` / `.exe` / `.com` 时，保存被阻止。
3. 其他 `PATH` 目录存在同名命令时，UI 显示解析顺序和潜在覆盖关系。
4. 命令名包含 `\`、`/`、NUL、Windows 保留设备名或非法文件名字符时，保存被阻止。

## 3.3 回归验收

1. User/System 环境变量 CRUD、Effective view、import/export、favorites、backup/undo 不受影响。
2. 删除 Command Shim 不删除对应 Node.js、脚本或用户目录中的其他文件。
3. Command Shim 不作为环境变量出现在 Registry 或 Effective environment 中。
4. 不支持的平台返回明确的 unsupported 错误，不产生部分文件。

## 3.4 验证矩阵

| 维度 | 覆盖项 |
|------|--------|
| Shell | Windows PowerShell 5.1、PowerShell 7、CMD、Git Bash、IDE Terminal |
| Path | 无空格、有空格、中文、括号、长路径 |
| Arguments | 空参数、多个参数、带空格、引号、`--flag=value` |
| Exit behavior | 0、非 0、stderr、交互输入 |
| Lifecycle | create、edit、rename、delete、external modification、missing target |
| Conflict | managed duplicate、external file、其他 PATH command |
| Persistence | 重启应用、多进程并发、配置损坏、schema 升级 |

---

# 四、范围与发布

## 4.1 建议首期范围

- Windows User-scope Command Shims。
- 通用 executable + fixed arguments 模型。
- 新增、编辑、删除、搜索、状态检查和冲突提示。
- Managed directory 自动接入 User `PATH`。
- 不执行任意 shell 字符串，不提供系统级 Shim。

## 4.2 后续候选能力

- 工作目录和额外环境变量。
- 项目级 / 临时会话级命令。
- 从完整命令安全解析为结构化配置。
- 配置导入导出。
- 系统级 Shim。
- 应用内受控试运行与诊断输出。

## 4.3 发布与回退

- 产品版本、发布方式和里程碑待确认。
- 功能应作为独立导航增量上线，不迁移现有环境变量数据。
- 回退应用版本时，已生成 Shim 不应突然失去所有权信息；回退兼容策略需要在技术方案中明确。
- 发布前必须构建 MSI/NSIS，并在干净 Windows 用户环境验证安装、创建、调用和卸载边界。

---

# 附录

## 附录 A：术语

| 术语 | 定义 |
|------|------|
| Command Shim | 位于 `PATH` 目录中的轻量命令入口，将调用转发到确定的 executable 和参数 |
| Managed directory | 由 EnvManager 管理、用于存放 Shim 文件的目录 |
| Fixed arguments | 每次调用都位于用户参数之前的固定参数列表 |
| User arguments | 用户在命令名后临时输入并原样转发的参数 |
| Ownership signature | 用于证明 Shim 由 EnvManager 生成并防止误删外部文件的信息 |

## 附录 B：示例配置

```json
{
  "commandName": "sharedev",
  "executable": "C:\\Tools\\Node\\node.exe",
  "fixedArguments": [
    "C:\\Tools\\sharedev\\dist\\sharedev.js"
  ]
}
```

> 上述 JSON 仅表达产品字段，不代表最终持久化 schema；最终结构由技术方案确定。

## 附录 C：关联文档

| 文档 | 路径 |
|------|------|
| EnvManager convenience design | `docs/plans/2026-07-20-env-manager-convenience-design.md` |
| EnvManager convenience implementation | `docs/plans/2026-07-20-env-manager-convenience-implementation.md` |

## 附录 D：设计规格

无 Figma 输入。本 PRD 暂不定义精确视觉尺寸；后续设计应复用 EnvManager 现有导航、表格、状态标签、确认弹层和错误提示样式。
