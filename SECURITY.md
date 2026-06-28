# 安全策略 / Security Policy

## 支持的版本

| 版本 | 安全更新 |
|------|----------|
| 0.1.x | ✅ |

当前为早期版本，安全修复只跟进最新发布。

## 报告漏洞

**请不要通过公开 Issue 报告安全漏洞。** 公开披露会让用户在修复前暴露于风险。

请走私密渠道：

- **首选** —— GitHub 私密漏洞报告：本仓库 **Security** 标签 → **Report a vulnerability**（GitHub Security Advisories）。
- 或通过官网 [meyatu.net](https://meyatu.net) / [meyatu.cn](https://meyatu.cn) 的联系方式联系 MEYATU LLC。

报告时请尽量包含：

- 受影响的版本 / 平台（macOS / Windows / Linux）
- 复现步骤或 PoC
- 影响范围（数据泄露、RCE、权限绕过等）

我们会尽力及时确认并修复，并在修复发布后与你协调披露时间。

## 用户须知

- Meyatu Code 是**自带 key** 的客户端：API key 由你提供，存于操作系统钥匙链（macOS Keychain / Windows 凭据管理器 / Linux libsecret），**不写入仓库、不进 WebView、不离开本机**。
- 安装包**未做代码签名**，请只从官方 [Releases](https://github.com/knosmars/M-Code/releases) 下载。
- 写文件、执行命令、SSH 等副作用操作默认需要你在界面确认（权限门）。
