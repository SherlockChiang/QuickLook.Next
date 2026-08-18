# Microsoft Store listing draft

Status: Partner Center-ready draft. Nothing in this file has been entered into
or submitted through Partner Center. The text and image constraints below were
rechecked against Microsoft Learn on 2026-08-18; the live Partner Center form
remains authoritative if its schema changes.

## Product facts

- Product: QuickLook Next
- Store product ID: `9PM0XKBFJC6R`
- Package identity: `Uranus92.QuickLookNext`
- Candidate version: `1.0.0.0`
- Architecture: x64
- Support: <https://github.com/SherlockChiang/QuickLook.Next/issues>
- Privacy policy: <https://sherlockchiang.github.io/QuickLook.Next/privacy.html>
- Source and release history: <https://github.com/SherlockChiang/QuickLook.Next>

## Verified listing constraints

- A description is required and accepts at most 10,000 plain-text characters.
- The optional short description accepts at most 1,000 characters; Microsoft
  recommends staying below 270 characters because some views truncate there.
- Up to 20 product features may be supplied. Each accepts at most 200
  characters and Partner Center adds the bullets.
- Each listing needs a description and at least one screenshot. Microsoft
  recommends at least four screenshots and accepts up to 10 desktop images.
- Desktop screenshots must be PNG, no larger than 50 MB, and at least
  1366 x 768. A caption may contain at most 200 characters.
- For a bulk import, first export Partner Center's UTF-8 CSV and preserve its
  `Field`, `ID`, and `Type` columns exactly. Add or edit only the locale value
  columns (`en-us`, `zh-cn`, and `zh-tw` as emitted by that export) and import
  the complete folder when local screenshot assets are included.

Sources: [Store listing fields](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/add-and-edit-store-listing-info),
[screenshots and images](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/screenshots-and-images), and
[listing import/export](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/import-and-export-store-listings).

## Proposed Partner Center classification

- Primary category: **Utilities + tools**.
- Subcategory: leave unset unless the live form requires one; QuickLook Next
  previews files but is not a file manager.
- Optional secondary category: **Productivity**.
- Base price: free.
- Publishing hold: do not publish automatically. Keep the first submission on
  manual hold until Store-signed acquisition, upgrade, rollback, and uninstall
  evidence is recorded.

## English (en-US)

Short description:

> Preview files instantly from Windows Explorer with the Space key.

Long description:

> QuickLook Next is a fast, native Windows file previewer. Select a file in
> Explorer, press Space, and see useful context without leaving your workflow.
>
> Preview images, documents, source code, structured data, archives, media,
> fonts, certificates, executables, ebooks, mail, and folders. Images support
> zooming, panning, nearby-file browsing, EXIF details, and color information.
> Specialist or potentially risky formats receive bounded, honest previews
> instead of pretending to be full editors.
>
> The WinUI 3 shell stays focused on presentation and Windows integration. A
> Rust-first native core handles file probing, metadata extraction, bounded
> parsing, and safe format detection. Raster and complex-content work is kept
> in cancellable helper processes so a difficult file is less likely to take
> down the interface.
>
> QuickLook Next does not require an account, show advertisements, or upload
> the files you preview to the project. See the privacy policy for details.

Feature highlights:

- Open and close a preview with Space.
- Follow nearby Explorer selections with the arrow keys.
- Zoom, pan, and inspect image metadata.
- Preview common documents, code, data, archives, media, and folders.
- Use keyboard navigation, high-contrast support, reduced-motion behavior, and
  multi-monitor DPI handling.

## 简体中文 (zh-CN)

简短描述：

> 在 Windows 文件资源管理器中按空格键，立即预览文件。

详细描述：

> QuickLook Next 是快速、原生的 Windows 文件预览工具。在资源管理器中
> 选中文件，按下空格键即可查看内容，不打断当前工作流。
>
> 支持预览图片、文档、源代码、结构化数据、压缩包、音视频、字体、证书、
> 可执行文件、电子书、邮件和文件夹。图片支持缩放、平移、浏览相邻文件，
> 并可查看 EXIF 和色彩信息。专业或潜在高风险格式会显示有边界、诚实的预览，
> 不冒充完整编辑器。
>
> WinUI 3 外壳负责界面展示和 Windows 集成；Rust 优先的原生核心负责文件探测、
> 元数据提取、有边界解析和安全格式识别。光栅化和复杂内容处理在可取消的辅助
> 进程中运行，降低异常文件影响界面的风险。
>
> QuickLook Next 不要求账号、不展示广告，也不会把你预览的文件上传给项目。
> 详情请参阅隐私政策。

功能亮点：

- 按空格键打开或关闭预览。
- 用方向键跟随资源管理器中的相邻文件。
- 缩放、平移图片并查看图像元数据。
- 预览常见文档、代码、数据、压缩包、音视频和文件夹。
- 支持键盘导航、高对比度、减少动态效果和多显示器 DPI。

## 繁體中文 (zh-TW)

簡短描述：

> 在 Windows 檔案總管按下空白鍵，即時預覽檔案。

詳細描述：

> QuickLook Next 是快速、原生的 Windows 檔案預覽工具。在檔案總管選取檔案，
> 按下空白鍵即可查看內容，不必離開目前的工作流程。
>
> 支援預覽圖片、文件、原始碼、結構化資料、壓縮檔、音訊與影片、字型、憑證、
> 可執行檔、電子書、郵件和資料夾。圖片支援縮放、平移、瀏覽相鄰檔案，並可查看
> EXIF 與色彩資訊。專業或可能有風險的格式會提供有界限、誠實的預覽，不假裝是
> 完整編輯器。
>
> WinUI 3 外殼負責介面呈現與 Windows 整合；Rust 優先的原生核心負責檔案探測、
> 中繼資料擷取、有界限解析和安全格式辨識。光柵化與複雜內容處理會在可取消的
> 輔助程序中執行，降低異常檔案影響介面的風險。
>
> QuickLook Next 不要求帳戶、不顯示廣告，也不會將你預覽的檔案上傳給專案。
> 詳情請參閱隱私權政策。

功能亮點：

- 按下空白鍵開啟或關閉預覽。
- 使用方向鍵跟隨檔案總管中的相鄰檔案。
- 縮放、平移圖片並查看影像中繼資料。
- 預覽常見文件、程式碼、資料、壓縮檔、影音和資料夾。
- 支援鍵盤導覽、高對比、減少動態效果和多螢幕 DPI。

## Capability explanation draft

Use these as review notes, not as a substitute for the Partner Center
questionnaire:

- `runFullTrust`: QuickLook Next is a packaged desktop application. The
  capability allows the WinUI shell and its Rust-first ParserHost, RasterHost,
  and ShellBroker helper processes to perform the local preview and Explorer
  integration they are designed for. The app does not execute embedded scripts
  from previewed files.
- Startup task: the optional startup entry keeps the tray integration ready for
  the user's Space-key workflow. It is user-controllable in the app settings
  and can be disabled.
- File access: previews are opened for files selected through Windows Explorer
  or otherwise chosen by the user. Preview content is processed locally and is
  not uploaded to the project.

## Age-rating factual basis

Use these facts while answering the live IARC questionnaire; do not infer an
answer solely from this draft because IARC can change its questions:

- QuickLook Next is a general-purpose utility, not a game and not directed to
  children.
- It has no account system, advertising, telemetry backend, in-app purchases,
  chat, social feed, matchmaking, or developer-operated content catalog.
- It displays user-selected local files, which may contain arbitrary content
  owned or obtained by the user. The app does not curate or distribute that
  content.
- Network-related actions are explicit and ancillary: manual update checks,
  opening an EXIF location in the browser, following support links, or asking
  Windows to hydrate a cloud placeholder after confirmation.

Microsoft uses the live questionnaire to generate IARC ratings and requires
all questions to be answered accurately. The publisher display name and email
address are shared with IARC as part of that process. See
[Age ratings for MSIX apps](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/age-ratings).

## Certification notes draft

> QuickLook Next is a local Windows file-preview utility and requires no test
> account. In File Explorer, select a supported file and press Space to open or
> close the preview. The optional startup task keeps the tray/hotkey integration
> available and can be disabled in Settings.
>
> The package declares `runFullTrust` because the packaged WinUI desktop shell
> starts the Rust-first ParserHost and the RasterHost/ShellBroker helper
> processes used for bounded local parsing, rendering, and Explorer integration.
> Previewed files are not executed and are not uploaded to the project.
>
> Normal previewing works offline. Network access occurs only for user-initiated
> update checks, external support/map links, or confirmed cloud-placeholder
> hydration through Windows and the configured cloud provider. The privacy
> policy documents these cases.
>
> Please test clean launch, Explorer Space-key preview, Settings startup-task
> control, and several common local formats. Difficult formats are isolated in
> supervised helper processes so a parser or renderer failure should return an
> in-app error rather than terminate the shell.

## Screenshot plan

Capture these only from the final Store-signed candidate after the clean-install
and upgrade checks. Use PNG at 1920 x 1080 (comfortably above the 1366 x 768
desktop minimum), keep each file below 50 MB, and keep captions within 200
characters:

1. Hero: Explorer selection with a clean image preview and the Space workflow.
2. Image details: zoom/pan view with EXIF or color information visible.
3. Document view: a representative PDF or Office approximation with its
   bounded-preview messaging where applicable.
4. Breadth: a folder or file-list preview showing nearby navigation.
5. Settings/accessibility: the native settings surface with keyboard or
   startup behavior visible.

Do not use debug windows, personal filenames, private paths, crash dialogs, or
screenshots from an unverified build.

## Remaining gates

- Restore a working WACK environment (the current host's `appcert.exe` exits
  with `0xc0000142` before loading a package).
- Obtain explicit approval before installing a Store-signed flight/acquisition
  package in an isolated test environment; the unsigned submission candidate
  itself is not a valid local AppX installation subject. Record clean install,
  update, rollback, and uninstall without altering the submission artifact.
- Export the live Partner Center listing CSV before any bulk import, then map
  the three prepared locales onto its unchanged schema.
- Complete the live Partner Center age-rating questionnaire.
- Capture and review the final screenshots, then submit only after the package
  and listing data agree on identity and version.
