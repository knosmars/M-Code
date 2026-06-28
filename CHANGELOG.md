# Changelog

本文件记录值得注意的变更。格式大致遵循 [Keep a Changelog](https://keepachangelog.com/)，版本号遵循 SemVer。

## v0.1.0 — 2026-06-28

首个公开发布（Apache-2.0）。桌面端自主 AI 编程智能体。

### 新增

- **多 Provider** — Meyatu 官方网关（[api.meyatu.io](https://api.meyatu.io)）+ OpenAI 兼容 / Anthropic / Gemini / 自定义第三方 API；SSE 流式，支持取消、用量统计、429/5xx 退避重试。
- **Agent 循环引擎** — 多轮工具执行状态机，权限门、流式回调、可中断。
- **数十个工具**
  - 代码：读 / 写 / 编辑 / 多文件编辑 / 列目录 / grep / glob
  - 检索：语义索引与搜索、代码图、影响分析、LSP（hover / 跳转定义 / 找引用）
  - 运行：命令执行、内置终端
  - 版本控制：Git（status / diff / log / commit / branch / push）+ GitHub PR
  - 远程：SSH 执行
  - 网页研究：单页抓取 / 同域 BFS 爬取 / 链接发现（输出 Markdown）
  - 自动化：触发器、生命周期钩子、技能模板
  - 记忆：项目记忆 + 全局跨项目知识库
  - 文件检查点（可回滚）、MCP 工具接入、并行子智能体
- **桌面端** — Tauri 2 + Rust 后端，React 19 + TypeScript 前端；安装包覆盖 macOS（.dmg）/ Windows（.msi/.exe）/ Linux（.AppImage/.deb/.rpm）。
- **会话** — SQLite 持久化，搜索 / 重命名 / 自动标题；写文件、执行命令、SSH 等副作用操作经权限门确认。
- **隐私** — API key 存操作系统钥匙链，不进 WebView、不离开本机。

### 说明

- 以 **Apache-2.0** 开源；网页引擎采用宽松许可栈（reqwest + htmd），依赖树无 GPL/AGPL。
- 打包字体 Nunito、Resource Han Rounded 以 SIL OFL 1.1 随附（见 [NOTICE](./NOTICE)）。
- 安装包未做代码签名，首次运行系统可能拦截（见 README 放行步骤）。
