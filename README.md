# EnvManager

EnvManager 是一个 Windows 优先的用户/系统环境变量管理器，提供变量搜索、编辑、删除、PATH 分项管理、自动备份和恢复能力。

## 数据来源

EnvManager 直接读取和写入 Windows 注册表：

- User variables：`HKEY_CURRENT_USER\Environment`
- System variables：`HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Session Manager\Environment`

支持 `REG_SZ` 和 `REG_EXPAND_SZ`。应用不维护另一份环境变量副本。

## 权限边界

- 用户变量可在普通权限下修改。
- 系统变量需要管理员权限。
- 非管理员进程仍可只读查看系统变量。
- UI 提供 **Restart as administrator**，通过 Windows UAC 重新启动当前应用。

系统变量写入不会自动绕过 UAC，也不会把变量值放进提权命令行。

## 安全与恢复

每次新增、修改、删除或恢复之前，EnvManager 都会备份目标 scope 的完整注册表环境变量。Windows 备份目录为：

```text
%APPDATA%\EnvManager\backups
```

恢复操作会先创建新的 rollback backup，再让目标 scope 与所选备份一致。注册表修改完成后，应用广播 `WM_SETTINGCHANGE`。

已经运行的终端和程序仍保留自身的旧进程环境；需要重新打开终端或重启相关程序才能读取新值。

## PATH 编辑器

`Path` 变量会自动切换为分项编辑模式，支持：

- 新增、删除、上移和下移条目
- 大小写不敏感的重复项检测
- `%VARIABLE%` 展开结果预览
- 本地路径存在性检查

保存时会按当前顺序使用分号重新组合注册表值。

## 开发环境

- Node.js 20.19+
- pnpm 10
- Rust stable
- Visual Studio Build Tools：Desktop development with C++、Windows 10/11 SDK

```powershell
pnpm install
pnpm test
pnpm tauri dev
```

`pnpm dev` 仅启动 Web 预览，使用内存示例数据，不读取或修改注册表。

## 验证与构建

```powershell
cargo test --all-targets --manifest-path src-tauri/Cargo.toml
cargo check --all-targets --manifest-path src-tauri/Cargo.toml
pnpm test
pnpm build
pnpm tauri:build
```

Windows 构建会生成 MSI 和 NSIS 安装包。macOS 目前只保留编译边界，环境变量写入会返回 unsupported-platform；macOS 没有与 Windows User/System 注册表完全等价的统一存储。
