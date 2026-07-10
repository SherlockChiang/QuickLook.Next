namespace QuickLook.Next.App;

internal static class UiStrings
{
    private static readonly Lazy<Microsoft.Windows.ApplicationModel.Resources.ResourceLoader?> Loader = new(CreateLoader);

    public static string AppName => Get(nameof(AppName), "QuickLook Next");
    public static string Ready => Get(nameof(Ready), "Ready");
    public static string ReadyKind => Get(nameof(ReadyKind), "READY");
    public static string EmptyValue => Get(nameof(EmptyValue), "-");
    public static string FitZoom => Get(nameof(FitZoom), "Fit");

    public static string PreviewUnavailableTitle => Get(nameof(PreviewUnavailableTitle), "Preview unavailable");
    public static string PreviewUnavailableMessage => Get(nameof(PreviewUnavailableMessage), "Unable to preview this file.");
    public static string PreviewTimedOut => Get(nameof(PreviewTimedOut), "preview timed out");
    public static string PdfPageFailed => Get(nameof(PdfPageFailed), "pdf page failed");
    public static string SurfaceFailed => Get(nameof(SurfaceFailed), "surface failed");
    public static string PathCopied => Get(nameof(PathCopied), "Path copied");
    public static string FileCopied => Get(nameof(FileCopied), "File copied");
    public static string NoExifData => Get(nameof(NoExifData), "No EXIF data");

    public static string TrayShowPreview => Get(nameof(TrayShowPreview), "显示预览");
    public static string TrayAutoStart => Get(nameof(TrayAutoStart), "开机自启");
    public static string TrayExit => Get(nameof(TrayExit), "退出 QuickLook Next");
    public static string AutoStartEnableFailed => Get(nameof(AutoStartEnableFailed), "开机自启开启失败");
    public static string AutoStartDisableFailed => Get(nameof(AutoStartDisableFailed), "开机自启关闭失败");

    public static string ListingReading => Get(nameof(ListingReading), "正在读取文件夹...");
    public static string ListingError => Get(nameof(ListingError), "无法读取此文件夹");
    public static string StartupErrorPrefix => Get(nameof(StartupErrorPrefix), "startup error: ");
    public static string MovedToRecycleBin => Get(nameof(MovedToRecycleBin), "Moved to Recycle Bin");
    public static string DeleteFileTitle => Get(nameof(DeleteFileTitle), "Move file to Recycle Bin?");
    public static string DeleteFileMessage => Get(nameof(DeleteFileMessage), "{0} will be moved to the Recycle Bin.");
    public static string MoveToRecycleBin => Get(nameof(MoveToRecycleBin), "Move to Recycle Bin");
    public static string Cancel => Get(nameof(Cancel), "Cancel");
    public static string TextPreviewTruncated => Get(nameof(TextPreviewTruncated), "[Preview truncated]");
    public static string CopyAction => Get(nameof(CopyAction), "Copy");
    public static string CopiedAction => Get(nameof(CopiedAction), "Copied!");
    public static string DialogOk => Get(nameof(DialogOk), "OK");
    public static string ErrorKind => Get(nameof(ErrorKind), "ERROR");

    public static string FolderTypeDisplay => Get(nameof(FolderTypeDisplay), "文件夹");
    public static string CertificateHeroSubtitle => Get(nameof(CertificateHeroSubtitle), "Certificate");
    public static string PackageHeroSubtitle => Get(nameof(PackageHeroSubtitle), "App package icon");
    public static string ExecutableHeroSubtitle => Get(nameof(ExecutableHeroSubtitle), "Application icon");
    public static string OfficeEmbeddedImagePreview => Get(nameof(OfficeEmbeddedImagePreview), "Embedded image preview");

    private static Microsoft.Windows.ApplicationModel.Resources.ResourceLoader? CreateLoader()
    {
        try { return new Microsoft.Windows.ApplicationModel.Resources.ResourceLoader(); }
        catch { return null; }
    }

    private static string Get(string key, string fallback)
    {
        try
        {
            string? value = Loader.Value?.GetString(key);
            return string.IsNullOrWhiteSpace(value) ? fallback : value;
        }
        catch
        {
            return fallback;
        }
    }
}
