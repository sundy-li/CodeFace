# 可编辑主题包模板

1. 复制这个文件夹并重命名。
2. 把自己的 PNG、JPEG 或 WebP 图片放进文件夹；默认文件名为 `background.png`。
3. 编辑 `theme.json` 中的名称、文字、图片文件名和颜色。
4. 编辑 `codeface.css` 覆盖基础皮肤的视觉细节。
5. 在 CodeFace App 中点击“导入主题包”，选择整个文件夹。

导入文件夹必须同时包含 `theme.json`、`codeface.css` 和 JSON `image` 指向的图片。

主题 CSS 会追加到安全基础样式之后。为避免主题在后台请求外部资源，CSS 不允许使用
`@import`、`@font-face` 或 `url(...)`；背景图统一通过 `theme.json` 的本地 `image` 字段提供，
CSS 中可直接使用 `var(--codeface-art)`。

不要隐藏、覆盖或伪造 Codex 的输入框、项目选择器和操作按钮。Codex 更新后 DOM 结构可能变化，
依赖深层选择器的覆盖需要重新验证。
