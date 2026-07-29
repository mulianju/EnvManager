# Command Shims Shell 访问修复

## 问题

已保存的 Command Shim 在部分 PowerShell 与 Git Bash 中无法解析。真实桌面用户状态显示：

- User `Path` 已包含 `%LOCALAPPDATA%\EnvManager\bin`。
- 受管目录中只有 `.cmd`，缺少 Git Bash 使用的无扩展名 wrapper。
- 当前 `v0.2.0` 只在保存 Shim 时补写 User `Path`，查询状态不会修复旧安装残留。

早期命令行诊断运行在 `CodexSandboxOffline` 独立 SID 下，其 HKCU 不是桌面用户 HKCU，不能作为实际 User `Path` 证据。部分已运行 PowerShell/IDE 宿主仍可能继承旧环境；GUI 无法反向修改这些父进程。

## 修复范围

- 在 Command Shims 页面提供显式 `Repair shell access` 操作。
- 仅补写当前用户的 `Path`，不修改 System `Path`。
- 根据已有配置重建缺失的 `.cmd` 和 Git Bash wrapper。
- 已存在且内容不属于 EnvManager、或已被外部修改的文件不得覆盖。
- 任一步失败时，回滚本轮新增的 User `Path` 条目和本轮新建的 wrapper。
- 修复后提示完全退出并重新打开 Windows Terminal、PowerShell、Git Bash 或 IDE；已运行父进程继承的旧环境无法由 GUI 反向修改。

## 验收

- User `Path` 缺失时可显式修复，重复修复保持幂等。
- 只有 `.cmd` 的旧配置可补齐 Git Bash wrapper。
- `.cmd` 和无扩展名 wrapper 均缺失时可安全重建。
- 外部同名文件或已修改 wrapper 保持原样，并返回明确错误。
- 修复后的新 PowerShell/CMD 和 Git Bash 会话均可解析命令。
- 应用版本升级为 `0.2.1`，安装程序可从 `0.2.0` 正常识别为新版本。

## 回退

- 应用代码可回退到 `v0.2.0`。
- Repair 新建的 wrapper 仍带 EnvManager 所有权标识，可按现有删除逻辑处理。
- User `Path` 中的受管目录默认保留；需要移除时使用 User variables 的 Path 编辑器。

## 实现结果

- 新增 `repair_command_shims`，在同一 Command Shim 事务中补写 User `Path` 并修复缺失 wrapper。
- wrapper 修复仅接受“文件缺失”或“内容与所有权均匹配”两种状态；外部文件保持不变。
- wrapper 修复或后续 PATH 读取失败时，回滚本轮新增文件与 User `Path` 变更。
- 页面仅在存在 Shim 且 User `Path` 或 wrapper 缺失时展示 `Repair shell access`。
- 修复成功后明确提示完全退出并重新打开终端或 IDE。

## 验证结果

- Rust 全量：126 passed；live HKCU 写入测试 1 ignored。
- 安全契约：4 passed。
- 前端：9 files、95 tests passed；TypeScript 与 Vite production build 退出码为 0。
- Rust `cargo check`、`cargo fmt --check` 与 `git diff --check` 退出码为 0。
- Tauri `0.2.1` release build 成功生成 MSI 与 NSIS。
- 已在桌面用户 `MULIANJU-LEGION\mulianju` 下完成 Repair：页面由 `Missing shim` 变为 `Ready`。
- 真实用户新会话语义下，PowerShell 解析到 `sharedev.cmd`，Git Bash 解析到无扩展名 `sharedev` wrapper。
