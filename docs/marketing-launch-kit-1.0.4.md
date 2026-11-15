# QuickLook Next 1.0.4 宣传准备包

这份文档用于准备 1.0.4 的公开宣传。主宣传点放在 Windows Explorer 里的
空格键预览工作流，HDR 行步长修复放在版本更新说明中。

## 核心定位

QuickLook Next 让用户在 Windows 文件资源管理器中选中文件并按空格键，
无需打开对应应用就能快速查看内容。

一句话描述：

> 在 Windows 文件资源管理器中按空格键，立即预览文件。

英文一句话描述：

> Select a file in Windows Explorer, press Space, and preview it instantly.

## 面向人群

- 经常整理下载目录和项目目录的 Windows 用户
- 需要快速查看代码、PDF、Office 文件和压缩包的开发者与办公用户
- 处理图片、素材和媒体文件的设计师与内容创作者

## 首发文案

### 中文短文案

QuickLook Next 是一个快速、原生的 Windows 文件预览工具。
在资源管理器中选中文件，按空格键即可查看图片、PDF、Markdown、源代码、
压缩包、Office 文档、媒体和更多内容。方向键可以继续浏览相邻文件，
Esc 关闭预览。

### English short copy

QuickLook Next is a fast, native file previewer for Windows. Select a file in
Explorer, press Space, and preview images, documents, source code, archives,
media, folders, and more without leaving your workflow.

### 更新说明

- 修复 HDR FP16 图像上传时的行步长计算，解决部分 PQ HDR 预览中的重复行和洋红/白色块状伪影。
- 保留图片缩放、平移、相邻文件浏览和图像信息查看能力。
- 继续支持键盘导航、云文件下载状态和隔离辅助进程。

## 20 秒视频脚本

1. 显示 Windows Explorer 中包含 JPG、PDF、Markdown 和 ZIP 的目录。
2. 选中 JPG，按 Space，展示预览窗口出现。
3. 用方向键切换到 PDF 和 Markdown，展示相邻文件跟随浏览。
4. 展示图片缩放和底部胶片栏。
5. 画面停留在一句话上。

> Select a file. Press Space. Keep working.

结尾同时放置 GitHub Release 和 Microsoft Store 入口。Store 入口在商店版本
完成认证并显示可获取后启用。

## 截图清单

建议准备四张 16:9 截图，分辨率至少 1366 × 768。

- 图片预览，包含缩放和胶片栏
- PDF 页面预览和页码切换
- Markdown 或源代码预览与搜索
- 压缩包或 SQLite 表格预览

现有主视觉：`docs/images/quicklook-next-hero.png`。

## 发布渠道

- GitHub Release 页面，用于开发者、开源用户和直接下载
- Microsoft Store 产品页，用于普通 Windows 用户
- 项目网站首页，用于解释工作流和展示截图
- Reddit、Hacker News、Windows 社区和中文软件社区，用真实演示视频引流

各渠道使用独立链接参数，至少区分 `github`、`store`、`reddit`、`zh-community`
和 `video`，便于比较访问和安装效果。

## 首周节奏

- 第一天发布版本和 20 秒演示
- 第二天发布支持格式和键盘操作截图
- 第四天发布图片、PDF、代码的连续预览演示
- 第七天发布一次真实工作流案例，例如整理下载目录或筛选素材

## 发布前检查

- 确认 GitHub Release `v1.0.4` 链接可访问
- 确认 Microsoft Store 页面显示最新可获取版本后再投放 Store 链接
- 确认视频中使用的是当前版本界面和快捷键
- 文案中使用“预览”和“查看”，避免写成完整编辑器
- 对 HEIC、AVIF 和复杂 Office 版式保留可选解码器或版式差异说明
- 在宣传帖中提供反馈入口和隐私政策链接

## 指标

- GitHub Release 页面访问和安装包下载
- Microsoft Store 产品页访问和安装量
- 视频播放完成率和链接点击率
- 首次启动后能否成功完成一次 Space 预览

## 已发布帖子

- [r/opensource](https://www.reddit.com/r/opensource/comments/1xwof9e/i_built_quicklook_next_a_native_windows_explorer/)，使用 `Promotional` 标签，发布时间为 2026-11-15。
- [r/windowsapps](https://www.reddit.com/r/windowsapps/comments/1xwofop/quicklook_next_a_spacekey_file_previewer_for/)，使用 `Developer` 标签，发布时间为 2026-11-15。
- Hacker News 暂未发布，提交页要求登录 HN 账号。

