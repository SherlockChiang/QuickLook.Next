# QuickLook Next

[English](README.md)

在 Windows 文件资源管理器中选中文件，按一下空格键即可预览。

QuickLook Next 是使用 WinUI 3、Rust 和 GPU 合成表面构建的原生 Windows 文件预览工具。复杂解析器和光栅解码器运行在受限制的辅助进程中，损坏文件或异常大文件不容易拖垮主程序。

> QuickLook Next 仍在积极开发。当前版本是面向早期用户和测试者的免安装、未签名 Windows 版本。

## 主要特点

- 在文件资源管理器中按 **空格键** 打开或关闭预览。
- 预览打开时，可用 **方向键** 跟随资源管理器中的选中项切换文件。
- 支持图片、GIF/WebP 动画、PDF、文本与源代码、Markdown、CSV、文件夹、压缩包、Office 文档、音视频、字体、安装包、证书、可执行文件、SQLite、电子书、邮件等常用格式。
- 查看图片元数据和 EXIF、缩放图片、浏览相邻图片，并可打开原文件或所在位置。
- 不启动其他播放器即可播放 Windows 支持的本地音视频。
- 压缩包、Office、电子书、可执行文件和图片光栅处理均与界面进程隔离。
- 不会悄悄下载仅在线的云端文件。对于云端占位文件，默认使用只读元数据预览。
- 遵循 Windows 高对比度和减少动态效果等辅助功能设置。

## 下载与运行

从 [GitHub Releases](https://github.com/SherlockChiang/QuickLook.Next/releases) 下载最新的 `QuickLook.Next-*-win-x64.zip`。

1. 将 ZIP 解压到一个长期保留的目录，例如 `%LOCALAPPDATA%\Programs\QuickLook.Next`。
2. 运行 `QuickLook.Next.App.exe`。
3. 保持托盘程序运行，在文件资源管理器中选中文件，然后按 **空格键**。

当前版本为免安装版，暂时没有安装程序和自动更新。请勿直接在 ZIP 压缩包内部运行程序。

### Windows 安全提示

当前版本尚未进行 Authenticode 代码签名，Windows SmartScreen 可能显示“无法识别的应用”提示。运行前请使用同一 Release 中的 `.sha256` 文件核对 ZIP：

```powershell
Get-FileHash .\QuickLook.Next-0.1.0-win-x64.zip -Algorithm SHA256
```

将输出值与 `QuickLook.Next-0.1.0-win-x64.zip.sha256` 第一列比较。只有哈希一致且文件确实来自本仓库 Releases 页面时才应继续运行。

## 使用方法

| 操作 | 快捷键 |
| --- | --- |
| 打开或关闭预览 | `空格键` |
| 关闭预览 | `Esc` |
| 预览资源管理器中的上一个或下一个项目 | 方向键 |
| 缩放图片 | 鼠标滚轮或 `+` / `-` |
| 重置图片视图 | `Home` 或 `Ctrl+0` |
| 浏览相邻图片 | 图片窗口获得焦点时按 `Left` / `Right` |

点击预览窗口后仍可按空格键关闭。焦点位于文本框、按钮、列表项、开关或滑块时，空格键会保留 Windows 控件的标准行为。

托盘菜单提供开机启动和退出功能。关闭预览只会隐藏窗口，QuickLook Next 仍会留在托盘中等待下一次预览。

## 支持的内容

部分格式取决于 Windows 已安装的解码器，但内置路径已覆盖常用场景：

- **图片：** JPEG、PNG、GIF、WebP、BMP、TIFF；HEIC、AVIF 等格式可使用系统解码器回退。
- **文档：** PDF、DOCX、XLSX、PPTX、EPUB、FB2、Markdown、纯文本、源代码、配置文件和 CSV。
- **压缩包和软件包：** ZIP 等受支持压缩格式、应用包元数据、压缩包浏览和内部文件预览。
- **音视频：** Windows Media Foundation 支持的常见本地媒体格式，并显示轻量容器信息。
- **开发和专业文件：** PE/EXE/DLL、ELF、Minidump、证书、字体、SQLite、Torrent、邮件和 CHM 元数据。
- **文件夹：** 有安全数量限制的目录列表和缩略图调度。

Office 预览采用近似渲染，不会运行 Microsoft Office、宏、公式重算、嵌入脚本或浏览器引擎。复杂文档的显示效果可能与 Office 本身存在差异。

## 云端文件

QuickLook Next 会谨慎处理 OneDrive 等服务的仅在线占位文件：

- 打开预览不会自动下载仅在线文件。
- 无法确定内容是否在本地时，只显示元数据。
- 装饰性缩略图、Sidecar 和媒体播放不会触发隐藏的二次读取。
- 本地文件和已经下载到本地的云端文件可使用完整预览功能。

显式下载、下载进度和云端状态界面将在后续版本中完善。

## 系统要求

- Windows 10 1809 或更高版本，或 Windows 11。
- x64 处理器。
- Windows 文件资源管理器。全局空格键目前不集成其他文件管理器。
- Windows 合成系统支持的显卡和驱动。

免安装发布包已包含应用所需的 Windows App SDK 运行组件。部分图片和媒体格式仍需要 Windows 或 Microsoft Store 提供的可选解码器。

## 常见问题

### 按空格键没有反应

- 确认 `QuickLook.Next.App.exe` 正在运行且托盘图标存在。
- 确认文件资源管理器位于前台并且已选中文件。
- 重命名文件或在资源管理器文本框中输入时，程序会有意保留空格键。
- 启动新解压的版本前，请先通过托盘退出旧版本。

### 只显示元数据，没有显示完整内容

- 文件可能是仅在线的云端占位文件。
- Windows 可能未安装对应的系统解码器。
- 解析器可能达到安全限制或超时，可重新打开文件重试。

### 反馈问题

请创建 [GitHub Issue](https://github.com/SherlockChiang/QuickLook.Next/issues)，并提供：

- Windows 版本和 QuickLook Next 版本。
- 文件类型和大致大小，请勿上传隐私文件。
- 预期结果和实际结果。
- 可复现步骤及相关日志（如果有）。

## 从源码构建

需要：

- Windows x64、Visual Studio Build Tools 和 Desktop C++/MSVC 工具链。
- [`global.json`](global.json) 指定的 .NET SDK。
- Stable Rust MSVC 工具链。

```powershell
dotnet restore QuickLook.Next.slnx --locked-mode
cargo test --locked --manifest-path native/quicklook_next_native/Cargo.toml
cargo build --release --locked --manifest-path native/quicklook_next_native/Cargo.toml
dotnet build QuickLook.Next.slnx -c Release --no-restore
dotnet test QuickLook.Next.slnx -c Release --no-build --no-restore
.\tools\pack-release.ps1
```

版本化 ZIP 和校验文件会生成到 `artifacts/`，打包过程同时运行架构和图片语料守卫。

## 架构概览

- `QuickLook.Next.App`：WinUI 3 外壳、预览 Presenter、输入处理和进程监管。
- `quicklook_next_native`：Rust 文件探测、资源管理器集成、原生解析器、缩略图和图片解码。
- `QuickLook.Next.ParserHost`：隔离处理压缩包、Office、电子书、可执行文件等结构化解析。
- `QuickLook.Next.RasterHost`：隔离处理图片、PDF、系统解码器和共享 GPU 表面。
- App 与 Host 使用仅限当前用户、经过认证的命名管道，并通过请求 Generation 和取消机制隔离过期结果。

工程边界、验证方式和剩余工作参见 [`docs/review-readiness.md`](docs/review-readiness.md)。

## 安全问题

请勿通过公开 Issue 披露尚未修复的安全漏洞。在私有安全策略发布前，请通过仓库所有者的 GitHub 主页联系，并只提供最小描述，不要发送敏感样本文件。

## 许可证

项目尚未发布正式许可证。源码公开并不自动授予适用法律之外的再分发或修改权。稳定版本发布前计划补充正式许可证。
