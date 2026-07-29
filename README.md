# EnvManager

EnvManager 是一个 Windows 优先的用户/系统环境变量管理器。变量值直接读取和写入 Windows Registry，应用不会维护另一份环境变量副本。

## 变量来源与作用域

- **User**：`HKEY_CURRENT_USER\Environment`，普通权限下可读写。
- **System**：`HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Session Manager\Environment`，普通权限下可读，写入需要管理员权限。
- **Effective**：按 Windows 规则合并 User 和 System；同名变量由 User 覆盖 System，PATH 按 System PATH 后接 User PATH 展示。

User 和 System 页面支持搜索、双击编辑、复制名称/原始值/PowerShell 表达式、收藏，以及跨作用域复制或移动。Effective 页面用于确认最终生效值和被遮蔽的来源，不提供直接编辑。

注册表修改成功后，EnvManager 会广播 `WM_SETTINGCHANGE`。已经运行的终端和程序仍保留旧的进程环境，需要重新打开才能读取新值。

## PATH 编辑

`Path` 变量使用分项编辑器，支持：

- 通过系统目录选择器添加路径；
- 粘贴多行或分号分隔的路径，并在插入时去重；
- 拖放排序，以及上移、下移、删除；
- 按全部、重复、缺失筛选；
- 显式清理重复项，保留第一个规范化匹配项；
- 预览 `%VARIABLE%` 展开结果和本地路径存在性；
- 保存前查看新增、移除和移动项。

缺失路径不会自动删除，因为网络盘或暂时不可用的路径仍可能有效。

## Undo、备份与外部变更

每次新增、修改、删除、移动、导入或恢复前，EnvManager 都会备份受影响 scope 的完整 Registry 环境。Windows 备份目录为：

```text
%APPDATA%\EnvManager\backups
```

成功提示中的 **Undo** 会按相反顺序恢复本次操作创建的备份。Undo 是针对最近一次操作的恢复能力，不是长期的无限历史；恢复本身也会先创建 rollback backup。

应用通过 Registry revision 轮询检测外部修改。没有编辑器或确认框打开时会自动刷新；存在未提交编辑时只提示外部变化，不会静默覆盖当前内容。

## 导入与导出

导入会先解析文件并显示 create/update/conflict 预览，用户确认策略后才写入。支持范围：

- **JSON**：EnvManager 结构化格式，保留 scope 和 `REG_SZ` / `REG_EXPAND_SZ` 类型。
- **`.env`**：`NAME=VALUE` 文本，导入时必须选择目标 scope，值按字符串处理。
- **`.reg`**：仅接受 `HKCU\Environment` 和 `HKLM\...\Environment` 两个环境变量键；其他 Registry key 或不支持的构造会被拒绝。

System scope 的导入、编辑、删除、移动和恢复都遵循同一 UAC 权限边界。应用不会通过命令行传递变量值来绕过提权。

## 收藏、托盘与 QuickPanel

收藏只保存变量标识 `{scope,name}`，不会持久化变量值。设置文件位于：

```text
%APPDATA%\EnvManager\settings.json
```

托盘菜单可以显示主窗口、启动最新环境的 PowerShell 或退出应用。点击托盘图标可打开 QuickPanel：它优先显示收藏项，支持搜索、键盘选择、临时显示敏感值和复制原始值。QuickPanel 隐藏或刷新后会重新遮罩已显示的值。

**New PowerShell** 会重新读取 User/System Registry、按 Windows 顺序组合 PATH、有限递归展开 `%NAME%`，并为子进程显式设置最新环境，因此不依赖 EnvManager 自身启动时继承的旧环境。

## Command Shims

Command Shims 用短命令绑定指定的 Windows `.exe` / `.com` 可执行文件和固定参数。例如，把 `toolx` 配置为：

```text
Executable:      C:\Tools\Node\node.exe
Fixed argument: C:\Tools\toolx\dist\cli.js
```

在新终端执行 `toolx --help` 时，EnvManager 会直接启动上述 `node.exe`，先传入固定的脚本路径，再转发 `--help`。它不会使用当前 `PATH` 中的默认 Node.js，也不会执行任意 shell 字符串。

脚本文件不能直接作为 executable。需要选择对应运行时（例如 `node.exe`、`python.exe` 或 `powershell.exe`），再把脚本绝对路径作为第一个 fixed argument。

首次保存 Command Shim 时，EnvManager 会把以下目录幂等加入 User `PATH`：

```text
%LOCALAPPDATA%\EnvManager\bin
```

每个命令同时生成 `<command>.cmd` 和无扩展名的 `<command>` wrapper，分别供 PowerShell/CMD 和 Git Bash 使用。已打开的终端不会自动获得新 `PATH`，保存后需要重新打开终端。旧版本创建的命令若缺少 Git Bash wrapper，在编辑器中重新保存一次即可补齐。

配置元数据位于 `%APPDATA%\EnvManager\command-shims.json`。EnvManager 只更新或删除所有权标识及内容校验均匹配的文件；外部同名文件或被手工修改的 wrapper 会被保留并报告冲突。

## 权限与平台边界

- UI 提供 **Restart as administrator**，通过 Windows UAC 重新启动当前应用。
- HKLM/System 写入与 UAC 提权必须在真实 Windows 管理员会话中人工确认；自动测试不会绕过或代替 UAC。
- macOS 目前只保留编译边界，环境变量写入返回 `unsupported-platform`；它没有与 Windows User/System Registry 完全等价的统一存储。

## 开发环境

- Node.js 20.19+
- pnpm 10
- Rust stable（Windows 使用 MSVC target）
- Visual Studio Build Tools：Desktop development with C++、Windows 10/11 SDK

```powershell
pnpm install
pnpm test
pnpm tauri dev
```

`pnpm tauri dev` 启动桌面端并读取真实 Registry。`pnpm dev` 仅启动浏览器预览，使用内存示例数据，不读取或修改 Registry，也不能验证原生文件选择器、托盘、QuickPanel、UAC 或 PowerShell 启动。

运行时环境变量、备份、收藏设置和 Command Shim 配置都保存在 Windows Registry 或 `%APPDATA%` / `%LOCALAPPDATA%`，不在 Git 仓库中。不要把真实 `.env`、`.reg`、导出文件或包含敏感参数的配置提交到版本库。

参与开发前请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。安全问题请按 [SECURITY.md](SECURITY.md) 私下报告。

## 验证与构建

```powershell
pnpm test
pnpm build
cargo test --all-targets --manifest-path src-tauri/Cargo.toml
cargo check --all-targets --manifest-path src-tauri/Cargo.toml
cargo fmt --check --manifest-path src-tauri/Cargo.toml
pnpm tauri:build
```

Windows release 构建生成 MSI 和 NSIS 安装包。发布前还需执行 opt-in 的 live HKCU 测试，以及主窗口、QuickPanel、安装包内容和 release executable 的桌面 smoke test。

## License

EnvManager is released under the [MIT License](LICENSE).
