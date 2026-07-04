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
