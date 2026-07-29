# Command Shims 发布记录

## 发布信息

| 字段 | 内容 |
|------|------|
| 版本 | `v0.2.0` |
| 发布日期 | 2026-07-29 |
| 公开仓库 | https://github.com/mulianju/EnvManager |
| GitHub Release | https://github.com/mulianju/EnvManager/releases/tag/v0.2.0 |
| 发布提交 | `a8ec4030265efba01adb3eae7e625fc3f0288aef` |
| CI 修复提交 | `2765e5a67e9d603cbca7822f1f816c0594a7b620`（仅测试目录隔离，不改变安装包） |

## 安装包

| 文件 | 大小 | SHA-256 |
|------|------|---------|
| `EnvManager_0.2.0_x64_en-US.msi` | 3,665,920 bytes | `D9289CEDE30F43EE6207573FB27ED10028CFF9160237A679168FFB264AAE1366` |
| `EnvManager_0.2.0_x64-setup.exe` | 2,469,223 bytes | `90FDC0796371EEAEE3ABD6C4C7999AC2F4DDFDE1A13AA99ED807128D45577C9E` |

GitHub Release 返回的 asset digest 与本地 SHA-256 一致，两个附件状态均为 `uploaded`。

## 发布验证

- GitHub 仓库可见性为 `PUBLIC`，默认分支为 `main`。
- 完整 Git 历史 50 个提交经 gitleaks 扫描，无泄漏。
- GitHub Actions run `30420705513` 通过前端测试、前端构建、Rust 格式检查、Rust 全量测试和 `cargo check`。
- 本地 Tauri release 构建生成 MSI 与 NSIS，构建退出码为 0。
- live HKCU 写入测试仍为 opt-in ignored，不在 CI 中修改 runner Registry。

## 回退

- 源码回退到 `v0.1.0` 可移除 Command Shims 功能。
- 已生成的受管 Shim 和 `%APPDATA%\EnvManager\command-shims.json` 需按所有权标识确认后删除。
- User `PATH` 中 `%LOCALAPPDATA%\EnvManager\bin` 默认保留，需移除时通过 EnvManager 的 User Path 编辑器操作。
