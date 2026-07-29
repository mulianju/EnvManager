# Contributing to EnvManager

感谢你参与 EnvManager。这个项目直接读写 Windows 环境变量和本地文件，修改时请优先保持行为可预测、影响范围明确。

## 开发准备

- Windows 10/11
- Node.js 20.19+
- pnpm 10
- Rust stable，Windows 使用 MSVC target
- Visual Studio Build Tools 的 Desktop development with C++ 和 Windows SDK

```powershell
pnpm install --frozen-lockfile
pnpm test
pnpm build
cargo test --all-targets --manifest-path src-tauri/Cargo.toml
cargo check --all-targets --manifest-path src-tauri/Cargo.toml
cargo fmt --check --manifest-path src-tauri/Cargo.toml
```

## 提交修改

1. 先创建或关联 Issue，说明用户问题、预期行为和影响范围。小型文档修正可以直接提交 Pull Request。
2. 保持改动聚焦，不顺带重构无关模块，不无理由新增依赖。
3. 为行为变更补充相应的 Rust 或 Vitest 测试，并在 Pull Request 中写明验证命令和结果。
4. 涉及 Registry、User `PATH`、备份、导入导出、Command Shim 或 Tauri 权限时，说明失败回滚方式及仍需人工验证的 Windows 场景。
5. 不提交真实环境变量、`.env`、`.reg`、导出文件、访问令牌、个人绝对路径或其他本机敏感数据。

自动格式化或 lint fix 可能扩大 diff。请先检查其影响，仅提交与当前修改有关的排版变更。

## Pull Request 检查

- TypeScript 构建和前端测试通过。
- Rust 测试、类型检查和 `cargo fmt --check` 通过。
- 新增的 Tauri command 同步更新 invoke handler、权限清单和安全契约测试。
- 用户可见行为、数据位置、权限要求和兼容性变化已更新 README 或对应文档。
- 没有加入本机数据或凭据。
