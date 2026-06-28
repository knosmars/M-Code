# 贡献指南 / Contributing

欢迎 Issue 与 Pull Request！

## 开发环境

见 [README 的「从源码构建」](./README.md#从源码构建开发)。简版：

```bash
npm install
npm run tauri:dev
```

## 提 PR 前

请确保以下全部通过（CI 也会跑这些）：

```bash
npx vitest run                                              # 前端单测
npx tsc -b                                                  # 类型检查
cd src-tauri && cargo test --all-targets                   # Rust 单测
cd src-tauri && cargo clippy --all-targets -- -D warnings  # Rust lint
```

## 约定

- **小而聚焦的 PR** —— 一个 PR 只做一件事，便于审查。
- **测试驱动** —— 新功能 / 修 bug 请带测试。
- **提交信息** —— 用 [Conventional Commits](https://www.conventionalcommits.org/)（`feat:` / `fix:` / `refactor:` / `docs:` / `chore:` …）。
- **跟随现有风格** —— 匹配周边代码的命名、结构与注释密度，不引入无关重构。
- **依赖许可** —— 新增依赖必须是宽松许可（MIT / Apache-2.0 / BSD 等）。**不要引入 GPL / AGPL 等 copyleft 依赖**，否则会破坏本项目的 Apache-2.0 发布。新增 Rust 依赖后请用 `cargo license` 自查。

## 许可

提交即表示你同意你的贡献以 **Apache-2.0** 授权（inbound = outbound）。

“Meyatu” / “梅亚图” 名称与 Logo 是 MEYATU LLC 的商标，不在代码许可授权范围内（Apache-2.0 §6）。
