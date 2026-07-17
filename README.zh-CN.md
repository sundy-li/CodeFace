# CodeFace

[English](README.md) | [简体中文](README.zh-CN.md)

CodeFace 是一个面向 Codex 桌面端的原生跨平台外观管理器。它使用 Rust 和 GPUI 构建，通过仅绑定本机回环地址的 Chrome DevTools Protocol（CDP）把主题 CSS 与必要的界面集成代码注入 Codex。

CodeFace 不修改官方 Codex App、Windows 安装包、`app.asar`、代码签名、登录状态或 API 配置。关闭主题后，官方界面可以安全恢复。

## 功能

- 创建、编辑、导入和切换主题
- 直接编辑 `theme.json` 和具有 CSS 语法高亮的 `codeface.css`
- 导入 PNG、JPEG 或 WebP 背景图
- 未选择图片时自动生成纯白背景
- 首页主题预览和四个原生快捷建议按钮
- 从管理器关闭或重启 Codex
- English 与简体中文界面
- 默认跟随系统语言，也可以在 Settings / 设置中手动切换
- macOS 与 Windows 共用一套 GPUI 界面和 Rust 核心

## 为什么使用 CDP

Codex 没有提供完整的第三方皮肤接口。CodeFace 使用 Codex 自带 Chromium 的调试协议，在运行时添加样式，而不是重新打包或修改官方应用。

安全约束：

- 调试端口只绑定 `127.0.0.1`
- WebSocket 地址必须通过本机地址与端口校验
- 主题图片最大 16 MiB，主题 CSS 最大 256 KiB
- 自定义 CSS 不能加载外部 URL、字体或 `@import`
- 装饰层不接管真实 Codex 控件的点击事件

## 项目结构

```text
gui/src/
├── main.rs              GPUI 界面和交互
├── i18n.rs              语言检测、设置持久化和翻译
├── theme.rs             主题校验、存储和图片转换
├── cdp.rs               Rust CDP 客户端、验证和注入守护进程
├── paths.rs             CodeFace 数据目录
└── platform/
    ├── macos.rs         macOS 应用发现与生命周期
    └── windows.rs       Windows 应用发现与生命周期

resources/
├── assets/              内嵌基础 CSS 和渲染器 JavaScript
├── i18n/                内嵌 JSON 翻译资源
└── theme-pack-template/ 可编辑主题模板

xtask/                   Rust 打包工具
```

程序运行不依赖 Shell、PowerShell、AppleScript 或外部 Node.js。`codeface-inject.js` 被编译进 Rust 二进制，因为 DOM 操作必须在 Codex 的 Chromium 渲染器中执行。

## 开发

需要当前稳定版 Rust 工具链。

```bash
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check --locked -p codeface --target x86_64-pc-windows-gnu
cargo xtask package
```

macOS 产物：

```text
dist/CodeFace.app
```

Windows 产物：

```text
dist/windows/CodeFace.exe
dist/windows/CodeFace.ico
```

`CodeFace.exe` 已内嵌应用图标，Windows 快捷方式默认使用该图标；如需手动指定快捷方式图标，可选择同目录下的 `CodeFace.ico`。

## 主题包

一个主题目录包含：

```text
theme.json
codeface.css
background.png
```

模板位于 [`resources/theme-pack-template`](resources/theme-pack-template)。主题列表中双击主题可以直接编辑源码。

## 数据目录

- macOS：`~/Library/Application Support/CodeFace`
- Windows：`%LOCALAPPDATA%\CodeFace`

## 命令行诊断

这些命令由同一个 CodeFace Rust 可执行文件提供：

```text
codeface --apply-active
codeface --verify 9341
codeface --restore
codeface --print-data-root
```

## 文档

完整文档从 [`docs/README.md`](docs/README.md) 开始，包括用户指南、主题包格式、架构与安全模型、开发构建、CI 发布和故障排查。

## 致谢

CodeFace 的设计受到以下推文和项目的影响与启发：

- [Randy 在 X 上发布的推文](https://x.com/randyloop/status/2077813650564452850)
- [Fei-Away/Codex-Dream-Skin](https://github.com/Fei-Away/Codex-Dream-Skin)

## 许可证

MIT。请参阅 [LICENSE](LICENSE) 和 [NOTICE.md](NOTICE.md)。
