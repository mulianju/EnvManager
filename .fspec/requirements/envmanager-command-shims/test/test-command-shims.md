# Command Shims 测试记录

## 验收范围

- Windows User-scope Command Shims。
- 结构化 `command name + executable + fixed arguments`。
- `%APPDATA%\EnvManager\command-shims.json` 配置持久化。
- `%LOCALAPPDATA%\EnvManager\bin` 受管目录与 User `PATH` 幂等接入。
- 创建、编辑、重命名、删除、冲突、缺失目标和外部修改保护。
- 参数、stdin/stdout/stderr 与退出码传递。
- PowerShell/CMD 使用 `.cmd`，Git Bash 使用无扩展名 shebang wrapper。

## 自动化验证

| 验证项 | 命令 | 实际结果（2026-07-28） |
|------|------|------|
| Command Shim 定向测试 | `cargo test --manifest-path src-tauri/Cargo.toml command_shim --lib` | 13 passed，包含真实 Git Bash、PATH 回滚、事务重入与实时冲突预检 |
| Rust 全量测试 | `cargo test --manifest-path src-tauri/Cargo.toml --all-targets` | 128 passed；live HKCU 写入测试 1 ignored |
| Rust 类型检查 | `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | 退出码 0 |
| Rust 格式检查 | `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | 退出码 0，不改写文件 |
| TypeScript | 指定 Node.js 执行 `node_modules\typescript\bin\tsc` | 退出码 0 |
| 前端测试 | 指定 Node.js 执行 `node_modules\vitest\vitest.mjs run` | 9 files、94 tests 全部通过 |
| 前端构建 | 指定 Node.js 执行 `node_modules\vite\bin\vite.js build` | 1810 modules transformed，退出码 0 |

## 真实 Shim 语义

Rust Windows 测试 `generated_shim_preserves_streams_arguments_spaces_and_exit_code` 在隔离临时目录生成并通过 `PATH` 解析真实 `.cmd`，目标使用 Windows PowerShell 脚本。覆盖：

- executable、Shim 和脚本路径包含空格，脚本路径包含中文。
- fixed arguments 包含空格、`&`、`%` 和字面双引号。
- runtime arguments 包含空格、`&` 和 `%`。
- stdin 输入 `hello-input` 可被目标读取。
- stdout 和 stderr 保持可见。
- 目标退出码 `7` 由 Shim 原样返回。

Rust Windows 测试 `generated_shell_shim_runs_from_git_bash_path` 通过本机 Git 安装定位真实 `bash.exe`，从临时 `PATH` 解析无扩展名 wrapper，并覆盖固定参数、运行时参数、单引号转义、stdin/stdout/stderr 和退出码 `7`。未安装 Git Bash 的环境会跳过该平台集成断言。

测试只操作系统临时目录，不创建或覆盖真实 `sharedev`，不修改真实 User `PATH`。

## 桌面验收

- 实际启动 Tauri 开发端并定位唯一 `EnvManager` 主窗口。
- 检查 1180x760 Command Shims 空状态、受管目录状态、表格、分页和新增弹窗。
- 检查命令名、executable、fixed argument 行、文件选择按钮、执行预览及保存禁用状态。
- 确认主工作区不发生页面级滚动，表格和弹窗正文各自内部滚动。
- 860px 以下顶部工具栏使用两行布局；620px 以下表格行改为纵向布局。

## 安全与回退

- 同名外部 `.cmd/.bat/.exe/.com` 不覆盖。
- 受管 Shim 内容和记录 checksum 不一致时显示 `Externally modified`，保存和删除均拒绝。
- 配置写入失败时恢复旧 Shim；删除配置失败时恢复已删除 Shim；重命名失败时恢复旧配置和文件。
- User `PATH` 写入复用环境变量服务的备份、广播和失败回滚。
- Shim 保存前按 Registry 合成的新终端 `PATH` 预检冲突；后续 Shim 事务失败时回滚本次新增的 PATH，检测到并发外部修改时拒绝覆盖。
- 回退代码后可删除 `%APPDATA%\EnvManager\command-shims.json` 与确认属于 EnvManager 的受管 Shim；User `PATH` 目录条目默认保留，需移除时通过 EnvManager 的 User Path 编辑器操作。

## 未自动覆盖

- 未对真实 `sharedev` 进行创建或替换，避免影响用户当前调试工具。
- Windows PowerShell 5.1 已作为真实测试目标；CMD 是 `.cmd` 解析入口；Git Bash 由真实 `bash.exe` 自动化覆盖。PowerShell 7 与 IDE Terminal 的人工矩阵可在用户创建首个实际 Shim 后继续验证。
- 尚未通过故障注入逐个触发配置原子写、重命名删除和回滚本身失败的所有分支；这些分支已做代码审查，后续可增加可注入文件系统测试接口。
