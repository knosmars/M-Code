# Meyatu Code

[![Release](https://img.shields.io/github/v/release/knosmars/M-Code?sort=semver)](https://github.com/knosmars/M-Code/releases)
[![License](https://img.shields.io/github/license/knosmars/M-Code)](./LICENSE)

桌面端自主 AI 编程智能体 —— 基于 Tauri 2 + React 构建。读写、运行代码，跨 SSH 操作远程服务器，研究网页，并能给自己配置自动化。

![Meyatu Code](https://github.com/user-attachments/assets/0f4843cf-340a-4626-a976-63ae105aa139)

<p align="center">
  <a href="https://github.com/knosmars/M-Code/releases/latest"><img src="https://img.shields.io/badge/下载-Windows-0078D6?style=for-the-badge&logo=windows&logoColor=white" alt="Download for Windows"></a>
  &nbsp;
  <a href="https://github.com/knosmars/M-Code/releases/latest"><img src="https://img.shields.io/badge/下载-macOS-000000?style=for-the-badge&logo=apple&logoColor=white" alt="Download for macOS"></a>
  &nbsp;
  <a href="https://github.com/knosmars/M-Code/releases/latest"><img src="https://img.shields.io/badge/下载-Linux-FCC624?style=for-the-badge&logo=linux&logoColor=black" alt="Download for Linux"></a>
</p>

<p align="center"><sub>跳转到最新 <a href="https://github.com/knosmars/M-Code/releases/latest">Release</a>，按平台选安装包。</sub></p>

## 安装

到 [Releases](https://github.com/knosmars/M-Code/releases) 下载对应平台的安装包：

- **macOS** — `.dmg`
- **Windows** — `.msi` / `.exe`
- **Linux** — `.AppImage` / `.deb`

安装包未做代码签名，首次打开系统可能拦截：

- macOS：右键 → 打开（或系统设置 → 隐私与安全性 → 仍要打开）
- Windows：SmartScreen → 更多信息 → 仍要运行

### 首次使用

Meyatu Code 自带界面但**不内置模型** —— 需要一个 API key。打开后进设置，添加一个 Provider 并填入 key，即可开始：

- **Meyatu 官方网关**（推荐） —— 到 [api.meyatu.io](https://api.meyatu.io) 申请 API key
- 或自带：OpenAI 兼容 / Anthropic / Gemini / 自定义第三方 API

Key 存在系统钥匙链，不离开本机。

## 截图

| Git 集成 | SSH 远程 |
|---|---|
| ![Git](https://github.com/user-attachments/assets/8e006a1b-5dd2-4712-84ea-67afd3f10fc7) | ![SSH](https://github.com/user-attachments/assets/4fc2739e-f4f8-4313-8f24-fcd903e63bb2) |
| **设置 / Provider** | |
| ![设置](https://github.com/user-attachments/assets/cfbc3d01-38e9-4c77-a9ea-3fbdb518563a) | |

## 特性

- **多 Provider** — 内置 OpenAI 兼容、Anthropic、Gemini 三协议适配，支持自定义第三方 API
- **数十个 Agent 工具** — 代码读写/编辑、命令与终端、grep/glob、Git 与 GitHub PR、语义检索与索引、LSP（hover / 跳转定义 / 找引用）、代码图与影响分析、文件检查点（可回滚）、MCP 工具接入
- **区别于通用编码助手的能力**
  - **SSH 远程执行** — 一等公民远程运维
  - **网页研究** — 抓取单页、同域 BFS 爬取、链接发现，输出 Markdown
  - **语义代码检索** — 向量索引，而非纯 grep
  - **自演化自动化** — Agent 可为自己写规则、技能、后台触发器与生命周期钩子
  - **全局跨项目知识库** — 跨项目的记忆与模式沉淀
  - **并行子智能体** — 并行派发多个 agent
- **桌面原生** — Tauri 2 + Rust 后端，体积小、性能高
- **隐私优先** — API Key 存系统钥匙链，不进 WebView；代码不离开本地
- **权限门** — 写文件 / 执行命令 / SSH 等副作用操作需用户确认
- **流式对话 + 会话管理** — SSE streaming（中断保留已收内容）、SQLite 持久化、搜索 / 重命名 / 自动标题

## 技术栈

| 层 | 技术 |
|----|------|
| 桌面框架 | Tauri 2 |
| 后端 | Rust (Edition 2021) |
| 前端 | React 19 + TypeScript (strict) |
| 流式通信 | Tauri Channel |
| 密钥存储 | keyring crate（OS 钥匙链） |
| 状态管理 | Zustand |
| 测试 | vitest（TS）/ cargo test（Rust） |

## 从源码构建（开发）

### 系统要求

- **Rust** ≥ 1.77
- **Node.js** ≥ 20
- **Linux** 额外依赖：`libwebkit2gtk-4.1-dev libgtk-3-dev libappindicator3-dev librsvg2-dev patchelf libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libsecret-1-dev`
  （macOS / Windows 的系统钥匙链已内置）

### 安装与开发

```bash
npm install            # 前端依赖
npm run tauri:dev      # 启动开发版
```

### 构建

```bash
npm run tauri:build    # 产物在 src-tauri/target/release/bundle/
```

## 测试

```bash
npx vitest run                                              # 前端单测
npx tsc -b                                                  # 类型检查
cd src-tauri && cargo test --all-targets                   # Rust 单测
cd src-tauri && cargo clippy --all-targets -- -D warnings  # Rust lint
```

## 项目结构

```
meyatu-code/
├── src/              # 前端 (React / TS)
│   ├── components/   # UI 组件
│   ├── agent/        # Agent 引擎（循环、工具注册、系统提示）
│   ├── hooks/        # React Hooks（会话、流式、工作区）
│   ├── stores/       # Zustand 状态（会话、Provider、设置、视图）
│   └── types/        # TypeScript 类型
├── src-tauri/        # Rust 后端
│   └── src/
│       ├── commands/ # Tauri 命令（聊天流、密钥管理）
│       ├── tools/    # 工具实现（含 web 研究、SSH 等）
│       ├── provider/ # LLM 适配器（OpenAI / Anthropic / Gemini）
│       └── lib.rs    # Tauri 入口
└── docs/             # 文档
```

## 贡献

欢迎 Issue 与 Pull Request。提交前请确保 `npx vitest run`、`cargo test`、`cargo clippy -- -D warnings`、`npx tsc -b` 全绿。

## License

[Apache-2.0](./LICENSE)。第三方资产（打包字体 Nunito、Resource Han Rounded，均 SIL OFL 1.1）的归属见 [NOTICE](./NOTICE)。

代码以 Apache-2.0 开放，但 **“Meyatu” / “梅亚图” 名称与 Logo 是 MEYATU LLC 的商标**，不在许可授权范围内（Apache-2.0 §6）。

---

Copyright (c) 2026 MEYATU LLC / 长沙梅亚图智能科技有限公司 · [meyatu.net](https://meyatu.net) · [meyatu.cn](https://meyatu.cn)

Made with 🦀 Rust + ⚡ Tauri 2
