using System.Globalization;

namespace QuickLook.Next.App;

internal static class UiStrings
{
    private static readonly Lazy<Microsoft.Windows.ApplicationModel.Resources.ResourceLoader?> Loader = new(CreateLoader);

    public static string AppName => Get(nameof(AppName), "QuickLook Next");
    public static string Ready => Get(nameof(Ready), "Ready");
    public static string ReadyKind => Get(nameof(ReadyKind), "READY");
    public static string EmptyValue => Get(nameof(EmptyValue), "-");
    public static string FitZoom => Get(nameof(FitZoom), "Fit");
    public static string PlayAnimation => Get(nameof(PlayAnimation), "Play animation");
    public static string PauseAnimation => Get(nameof(PauseAnimation), "Pause animation");
    public static string ShowPreviewDetails => Get(nameof(ShowPreviewDetails), "Show preview details");
    public static string HidePreviewDetails => Get(nameof(HidePreviewDetails), "Hide preview details");

    public static string PreviewUnavailableTitle => Get(nameof(PreviewUnavailableTitle), "Preview unavailable");
    public static string PreviewUnavailableMessage => Get(nameof(PreviewUnavailableMessage), "Unable to preview this file.");
    public static string PreviewTimedOut => Get(nameof(PreviewTimedOut), "preview timed out");
    public static string PdfPageFailed => Get(nameof(PdfPageFailed), "pdf page failed");
    public static string SurfaceFailed => Get(nameof(SurfaceFailed), "surface failed");
    public static string PreviewTimedOutTitle => Get(nameof(PreviewTimedOutTitle), "Preview timed out");
    public static string PreviewTimedOutMessage => Get(nameof(PreviewTimedOutMessage), "The preview took too long. Try again.");
    public static string PreviewServiceUnavailableTitle => Get(nameof(PreviewServiceUnavailableTitle), "Preview service unavailable");
    public static string PreviewServiceUnavailableMessage => Get(nameof(PreviewServiceUnavailableMessage), "The preview service disconnected or restarted. Try again.");
    public static string PreviewDisplayFailedTitle => Get(nameof(PreviewDisplayFailedTitle), "Unable to display preview");
    public static string PreviewDisplayFailedMessage => Get(nameof(PreviewDisplayFailedMessage), "The preview could not be displayed on this device.");
    public static string PreviewContentFailedTitle => Get(nameof(PreviewContentFailedTitle), "Preview unavailable");
    public static string PreviewContentFailedMessage => Get(nameof(PreviewContentFailedMessage), "This file could not be previewed.");
    public static string RetryPreview => Get(nameof(RetryPreview), "Try again");
    public static string PathCopied => Get(nameof(PathCopied), "Path copied");
    public static string FileCopied => Get(nameof(FileCopied), "File copied");
    public static string NoExifData => Get(nameof(NoExifData), "No EXIF data");

    public static string TrayShowPreview => Get(nameof(TrayShowPreview), "Show preview");
    public static string TrayAutoStart => Get(nameof(TrayAutoStart), "Start with Windows");
    public static string TrayExit => Get(nameof(TrayExit), "Exit QuickLook Next");
    public static string AutoStartEnableFailed => Get(nameof(AutoStartEnableFailed), "Could not enable start with Windows");
    public static string AutoStartDisableFailed => Get(nameof(AutoStartDisableFailed), "Could not disable start with Windows");

    public static string ListingReading => Get(nameof(ListingReading), "Reading folder...");
    public static string ListingError => Get(nameof(ListingError), "Could not read this folder");
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

    public static string FolderTypeDisplay => Get(nameof(FolderTypeDisplay), "Folder");
    public static string ListingSortName => Get(nameof(ListingSortName), "Name");
    public static string ListingSortModified => Get(nameof(ListingSortModified), "Modified");
    public static string ListingSortType => Get(nameof(ListingSortType), "Type");
    public static string ListingSortSize => Get(nameof(ListingSortSize), "Size");
    public static string ListingPartialSuffix => Get(nameof(ListingPartialSuffix), " - partial contents");
    public static string ListingSummaryFormat => Get(nameof(ListingSummaryFormat), "{0:N0} files, {1:N0} folders - {2}");
    public static string ListingOpenBreadcrumbFormat => Get(nameof(ListingOpenBreadcrumbFormat), "Open {0} in listing");
    public static string PdfPageIndicatorFormat => Get(nameof(PdfPageIndicatorFormat), "{0:N0} / {1:N0}");
    public static string PdfPageIndicatorPagedFormat => Get(nameof(PdfPageIndicatorPagedFormat), "{0:N0} / {1:N0} (paged)");
    public static string PdfPageIndicatorEmpty => Get(nameof(PdfPageIndicatorEmpty), "0 / 0");
    public static string CertificateHeroSubtitle => Get(nameof(CertificateHeroSubtitle), "Certificate");
    public static string PackageHeroSubtitle => Get(nameof(PackageHeroSubtitle), "App package icon");
    public static string ExecutableHeroSubtitle => Get(nameof(ExecutableHeroSubtitle), "Application icon");
    public static string OfficeEmbeddedImagePreview => Get(nameof(OfficeEmbeddedImagePreview), "Embedded image preview");

    public static string Format(string format, params object[] arguments)
        => string.Format(CultureInfo.CurrentCulture, format, arguments);

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
