# Security Policy

EnvManager 会读取和修改 Windows User/System 环境变量，并管理本地 Command Shim 文件。环境变量和固定参数可能包含敏感信息，请不要在公开 Issue、日志或截图中披露真实值。

## 报告安全问题

请通过 GitHub 仓库的 **Security > Report a vulnerability** 私下提交报告，不要创建公开 Issue。报告中请包含：

- 受影响版本或 commit；
- 可复现步骤和实际影响；
- 涉及的数据范围与权限边界；
- 可行的缓解或修复建议（如有）。

维护者确认问题前，请避免公开利用细节。普通功能缺陷和不涉及敏感数据的错误可以使用公开 Issue。

## 安全边界

- EnvManager 不会把 Registry 环境变量复制到 Git 仓库。
- System scope 写入依赖 Windows UAC，不绕过管理员权限。
- 导入在用户确认后才写入，并只接受受支持的环境变量范围。
- Command Shim 使用结构化 executable 和参数，不执行任意 shell 文本。
- EnvManager 只覆盖或删除所有权和内容校验均匹配的受管 wrapper。
- 安装包目前未进行代码签名，Windows 可能显示未知发布者提示。

安全修复优先支持最新发布版本和当前主分支。
