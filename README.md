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
