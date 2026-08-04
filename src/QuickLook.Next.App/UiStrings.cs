using System.Globalization;

namespace QuickLook.Next.App;

internal static class UiStrings
{
    private static readonly Lazy<Microsoft.Windows.ApplicationModel.Resources.ResourceLoader?> Loader = new(CreateLoader);

    public static string AppName => Get(nameof(AppName));
    public static string Ready => Get(nameof(Ready));
    public static string ReadyKind => Get(nameof(ReadyKind));
    public static string PreviewReadyAnnouncement => Get(nameof(PreviewReadyAnnouncement));
    public static string EmptyValue => Get(nameof(EmptyValue));
    public static string FitZoom => Get(nameof(FitZoom));
    public static string PlayAnimation => Get(nameof(PlayAnimation));
    public static string PauseAnimation => Get(nameof(PauseAnimation));
    public static string ShowPreviewDetails => Get(nameof(ShowPreviewDetails));
    public static string HidePreviewDetails => Get(nameof(HidePreviewDetails));

    public static string PreviewUnavailableTitle => Get(nameof(PreviewUnavailableTitle));
    public static string PreviewUnavailableMessage => Get(nameof(PreviewUnavailableMessage));
    public static string PreviewTimedOut => Get(nameof(PreviewTimedOut));
    public static string PdfPageFailed => Get(nameof(PdfPageFailed));
    public static string SurfaceFailed => Get(nameof(SurfaceFailed));
    public static string PreviewTimedOutTitle => Get(nameof(PreviewTimedOutTitle));
    public static string PreviewTimedOutMessage => Get(nameof(PreviewTimedOutMessage));
    public static string PreviewServiceUnavailableTitle => Get(nameof(PreviewServiceUnavailableTitle));
    public static string PreviewServiceUnavailableMessage => Get(nameof(PreviewServiceUnavailableMessage));
    public static string PreviewDisplayFailedTitle => Get(nameof(PreviewDisplayFailedTitle));
    public static string PreviewDisplayFailedMessage => Get(nameof(PreviewDisplayFailedMessage));
    public static string PreviewContentFailedTitle => Get(nameof(PreviewContentFailedTitle));
    public static string PreviewContentFailedMessage => Get(nameof(PreviewContentFailedMessage));
    public static string ImageCodecRequiredTitle => Get(nameof(ImageCodecRequiredTitle));
    public static string ImageCodecRequiredMessageFormat => Get(nameof(ImageCodecRequiredMessageFormat));
    public static string ImageDecodeFailedTitle => Get(nameof(ImageDecodeFailedTitle));
    public static string ImageDecodeFailedMessage => Get(nameof(ImageDecodeFailedMessage));
    public static string RetryPreview => Get(nameof(RetryPreview));
    public static string PathCopied => Get(nameof(PathCopied));
    public static string FileCopied => Get(nameof(FileCopied));
    public static string NoExifData => Get(nameof(NoExifData));
    public static string OpeningFileFormat => Get(nameof(OpeningFileFormat));
    public static string DownloadingCloudFileFormat => Get(nameof(DownloadingCloudFileFormat));
    public static string DownloadingCloudFileProgressFormat => Get(nameof(DownloadingCloudFileProgressFormat));
    public static string DownloadingCloudFileBytesFormat => Get(nameof(DownloadingCloudFileBytesFormat));
    public static string CloudDownloadConsentTitle => Get(nameof(CloudDownloadConsentTitle));
    public static string CloudDownloadConsentMessageFormat => Get(nameof(CloudDownloadConsentMessageFormat));
    public static string DownloadForPreview => Get(nameof(DownloadForPreview));
    public static string UnknownFileSize => Get(nameof(UnknownFileSize));
    public static string CloudDownloadDeclined => Get(nameof(CloudDownloadDeclined));
    public static string CloudDownloadTooLargeFormat => Get(nameof(CloudDownloadTooLargeFormat));
    public static string CheckingFileAvailabilityFormat => Get(nameof(CheckingFileAvailabilityFormat));
    public static string CloudUnknownDeferred => Get(nameof(CloudUnknownDeferred));
    public static string CloudAvailabilityUnknownDeferred => Get(nameof(CloudAvailabilityUnknownDeferred));
    public static string CloudDownloadDeferred => Get(nameof(CloudDownloadDeferred));
    public static string CloudMediaDeferred => Get(nameof(CloudMediaDeferred));
    public static string CloudMediaAvailabilityUnknownDeferred => Get(nameof(CloudMediaAvailabilityUnknownDeferred));
    public static string CloudMetadataPreviewFormat => Get(nameof(CloudMetadataPreviewFormat));
    public static string PreviewModifiedMetadataFormat => Get(nameof(PreviewModifiedMetadataFormat));
    public static string PageCountSingularFormat => Get(nameof(PageCountSingularFormat));
    public static string PageCountFormat => Get(nameof(PageCountFormat));
    public static string PdfPageTimedOutStatusFormat => Get(nameof(PdfPageTimedOutStatusFormat));
    public static string PdfPageFailedStatusFormat => Get(nameof(PdfPageFailedStatusFormat));

    public static string TrayShowPreview => Get(nameof(TrayShowPreview));
    public static string TraySettings => Get(nameof(TraySettings));
    public static string TrayAutoStart => Get(nameof(TrayAutoStart));
    public static string TrayExit => Get(nameof(TrayExit));
    public static string WelcomeTitle => Get(nameof(WelcomeTitle));
    public static string WelcomeHeading => Get(nameof(WelcomeHeading));
    public static string WelcomeIntroduction => Get(nameof(WelcomeIntroduction));
    public static string WelcomeOpenShortcut => Get(nameof(WelcomeOpenShortcut));
    public static string WelcomeCloseShortcut => Get(nameof(WelcomeCloseShortcut));
    public static string WelcomeNavigationShortcut => Get(nameof(WelcomeNavigationShortcut));
    public static string WelcomeTrayBehavior => Get(nameof(WelcomeTrayBehavior));
    public static string WelcomeHelpHint => Get(nameof(WelcomeHelpHint));
    public static string WelcomeStart => Get(nameof(WelcomeStart));
    public static string AutoStartEnableFailed => Get(nameof(AutoStartEnableFailed));
    public static string AutoStartDisableFailed => Get(nameof(AutoStartDisableFailed));
    public static string SettingsTitle => Get(nameof(SettingsTitle));
    public static string SettingsGeneral => Get(nameof(SettingsGeneral));
    public static string SettingsGeneralDescription => Get(nameof(SettingsGeneralDescription));
    public static string SettingsAutoStart => Get(nameof(SettingsAutoStart));
    public static string SettingsAutoStartDescription => Get(nameof(SettingsAutoStartDescription));
    public static string SettingsLanguage => Get(nameof(SettingsLanguage));
    public static string SettingsLanguageDescription => Get(nameof(SettingsLanguageDescription));
    public static string SettingsSystemLanguage => Get(nameof(SettingsSystemLanguage));
    public static string SettingsLanguageEnglish => Get(nameof(SettingsLanguageEnglish));
    public static string SettingsLanguageSimplifiedChinese => Get(nameof(SettingsLanguageSimplifiedChinese));
    public static string SettingsLanguageTraditionalChinese => Get(nameof(SettingsLanguageTraditionalChinese));
    public static string SettingsAnimation => Get(nameof(SettingsAnimation));
    public static string SettingsAnimationDescription => Get(nameof(SettingsAnimationDescription));
    public static string SettingsAnimationSystem => Get(nameof(SettingsAnimationSystem));
    public static string SettingsAnimationAlways => Get(nameof(SettingsAnimationAlways));
    public static string SettingsAnimationStill => Get(nameof(SettingsAnimationStill));
    public static string SettingsTextWrapping => Get(nameof(SettingsTextWrapping));
    public static string SettingsTextWrappingDescription => Get(nameof(SettingsTextWrappingDescription));
    public static string SettingsTextWrappingAutomatic => Get(nameof(SettingsTextWrappingAutomatic));
    public static string SettingsTextWrappingAlways => Get(nameof(SettingsTextWrappingAlways));
    public static string SettingsTextWrappingNever => Get(nameof(SettingsTextWrappingNever));
    public static string SettingsTextSize => Get(nameof(SettingsTextSize));
    public static string SettingsTextSizeDescription => Get(nameof(SettingsTextSizeDescription));
    public static string SettingsTextSizeSmall => Get(nameof(SettingsTextSizeSmall));
    public static string SettingsTextSizeDefault => Get(nameof(SettingsTextSizeDefault));
    public static string SettingsTextSizeLarge => Get(nameof(SettingsTextSizeLarge));
    public static string SettingsTextLineNumbers => Get(nameof(SettingsTextLineNumbers));
    public static string SettingsTextLineNumbersDescription => Get(nameof(SettingsTextLineNumbersDescription));
    public static string DatabasePreviewUnavailable => Get(nameof(DatabasePreviewUnavailable));
    public static string SettingsRestartTitle => Get(nameof(SettingsRestartTitle));
    public static string SettingsRestartMessage => Get(nameof(SettingsRestartMessage));
    public static string SettingsAbout => Get(nameof(SettingsAbout));
    public static string SettingsAboutDescription => Get(nameof(SettingsAboutDescription));
    public static string SettingsVersionFormat => Get(nameof(SettingsVersionFormat));
    public static string SettingsProjectSource => Get(nameof(SettingsProjectSource));
    public static string SettingsHelpShortcuts => Get(nameof(SettingsHelpShortcuts));
    public static string SettingsOpenGitHub => Get(nameof(SettingsOpenGitHub));
    public static string SettingsViewReleases => Get(nameof(SettingsViewReleases));
    public static string SettingsCheckForUpdates => Get(nameof(SettingsCheckForUpdates));
    public static string SettingsCheckingForUpdates => Get(nameof(SettingsCheckingForUpdates));
    public static string UpdateAvailableTitle => Get(nameof(UpdateAvailableTitle));
    public static string UpdateAvailableMessageFormat => Get(nameof(UpdateAvailableMessageFormat));
    public static string UpdateUpToDateTitle => Get(nameof(UpdateUpToDateTitle));
    public static string UpdateUpToDateMessage => Get(nameof(UpdateUpToDateMessage));
    public static string UpdateNewerBuildTitle => Get(nameof(UpdateNewerBuildTitle));
    public static string UpdateNewerBuildMessage => Get(nameof(UpdateNewerBuildMessage));
    public static string UpdateCheckFailedTitle => Get(nameof(UpdateCheckFailedTitle));
    public static string UpdateCheckFailedMessage => Get(nameof(UpdateCheckFailedMessage));
    public static string SettingsHookStatus => Get(nameof(SettingsHookStatus));
    public static string SettingsHookReady => Get(nameof(SettingsHookReady));
    public static string SettingsHookDegradedFormat => Get(nameof(SettingsHookDegradedFormat));
    public static string SettingsHookFailedFormat => Get(nameof(SettingsHookFailedFormat));
    public static string SettingsHookStopped => Get(nameof(SettingsHookStopped));
    public static string SettingsRetryHook => Get(nameof(SettingsRetryHook));
    public static string SettingsLicenseNotice => Get(nameof(SettingsLicenseNotice));
    public static string SettingsDiagnostics => Get(nameof(SettingsDiagnostics));
    public static string SettingsDiagnosticsDescription => Get(nameof(SettingsDiagnosticsDescription));
    public static string SettingsCreateDiagnostics => Get(nameof(SettingsCreateDiagnostics));
    public static string DiagnosticsConsentTitle => Get(nameof(DiagnosticsConsentTitle));
    public static string DiagnosticsConsentMessage => Get(nameof(DiagnosticsConsentMessage));
    public static string DiagnosticsZipType => Get(nameof(DiagnosticsZipType));
    public static string DiagnosticsSavedTitle => Get(nameof(DiagnosticsSavedTitle));
    public static string DiagnosticsSavedMessage => Get(nameof(DiagnosticsSavedMessage));
    public static string DiagnosticsFailedTitle => Get(nameof(DiagnosticsFailedTitle));
    public static string DiagnosticsFailedMessage => Get(nameof(DiagnosticsFailedMessage));
    public static string SettingsSaveFailed => Get(nameof(SettingsSaveFailed));
    public static string SettingsSaveFailedMessage => Get(nameof(SettingsSaveFailedMessage));

    public static string ListingReading => Get(nameof(ListingReading));
    public static string ListingError => Get(nameof(ListingError));
    public static string StartupFailedMessage => Get(nameof(StartupFailedMessage));
    public static string MovedToRecycleBin => Get(nameof(MovedToRecycleBin));
    public static string DeleteFileTitle => Get(nameof(DeleteFileTitle));
    public static string DeleteFileMessage => Get(nameof(DeleteFileMessage));
    public static string MoveToRecycleBin => Get(nameof(MoveToRecycleBin));
    public static string Cancel => Get(nameof(Cancel));
    public static string TextPreviewTruncated => Get(nameof(TextPreviewTruncated));
    public static string TextPreviewTruncatedAtCharacterCountFormat => Get(nameof(TextPreviewTruncatedAtCharacterCountFormat));
    public static string SyntaxHighlightingCharacterLimitFormat => Get(nameof(SyntaxHighlightingCharacterLimitFormat));
    public static string SyntaxHighlightingSpanLimitFormat => Get(nameof(SyntaxHighlightingSpanLimitFormat));
    public static string CopyAction => Get(nameof(CopyAction));
    public static string CopiedAction => Get(nameof(CopiedAction));
    public static string DialogOk => Get(nameof(DialogOk));
    public static string ErrorKind => Get(nameof(ErrorKind));

    public static string FolderTypeDisplay => Get(nameof(FolderTypeDisplay));
    public static string ListingFileTypeDisplay => Get(nameof(ListingFileTypeDisplay));
    public static string ListingTypedFileFormat => Get(nameof(ListingTypedFileFormat));
    public static string ListingSortName => Get(nameof(ListingSortName));
    public static string ListingSortModified => Get(nameof(ListingSortModified));
    public static string ListingSortType => Get(nameof(ListingSortType));
    public static string ListingSortSize => Get(nameof(ListingSortSize));
    public static string ListingPartialSuffix => Get(nameof(ListingPartialSuffix));
    public static string ListingSummaryFormat => Get(nameof(ListingSummaryFormat));
    public static string ListingFilterPlaceholder => Get(nameof(ListingFilterPlaceholder));
    public static string ListingFilterAccessibleName => Get(nameof(ListingFilterAccessibleName));
    public static string ListingFilterResultsFormat => Get(nameof(ListingFilterResultsFormat));
    public static string ListingEncryptedSummaryFormat => Get(nameof(ListingEncryptedSummaryFormat));
    public static string ListingEncryptedPartialSummaryFormat => Get(nameof(ListingEncryptedPartialSummaryFormat));
    public static string ListingEncryptedCannotPreview => Get(nameof(ListingEncryptedCannotPreview));
    public static string ListingBrowseOnlySuffix => Get(nameof(ListingBrowseOnlySuffix));
    public static string ListingEntriesCannotBePreviewed => Get(nameof(ListingEntriesCannotBePreviewed));
    public static string ListingEncryptedRowAccessibleNameFormat => Get(nameof(ListingEncryptedRowAccessibleNameFormat));
    public static string ListingOpenBreadcrumbFormat => Get(nameof(ListingOpenBreadcrumbFormat));
    public static string PdfPageIndicatorFormat => Get(nameof(PdfPageIndicatorFormat));
    public static string PdfPageIndicatorPagedFormat => Get(nameof(PdfPageIndicatorPagedFormat));
    public static string PdfPageIndicatorEmpty => Get(nameof(PdfPageIndicatorEmpty));
    public static string PdfPageAccessibleNameFormat => Get(nameof(PdfPageAccessibleNameFormat));
    public static string CertificateHeroSubtitle => Get(nameof(CertificateHeroSubtitle));
    public static string PackageHeroSubtitle => Get(nameof(PackageHeroSubtitle));
    public static string ExecutableHeroSubtitle => Get(nameof(ExecutableHeroSubtitle));
    public static string OfficeEmbeddedImagePreview => Get(nameof(OfficeEmbeddedImagePreview));
    public static string OfficeEmbeddedImagePreviewFormat => Get(nameof(OfficeEmbeddedImagePreviewFormat));
    public static string TableDimensionsFormat => Get(nameof(TableDimensionsFormat));
    public static string TableShowingRowsFormat => Get(nameof(TableShowingRowsFormat));
    public static string TableSummaryFormat => Get(nameof(TableSummaryFormat));
    public static string TableFallbackColumnFormat => Get(nameof(TableFallbackColumnFormat));
    public static string TableCornerAccessibleName => Get(nameof(TableCornerAccessibleName));
    public static string TableColumnHeaderAccessibleNameFormat => Get(nameof(TableColumnHeaderAccessibleNameFormat));
    public static string TableRowHeaderAccessibleNameFormat => Get(nameof(TableRowHeaderAccessibleNameFormat));
    public static string TableCellAccessibleNameFormat => Get(nameof(TableCellAccessibleNameFormat));
    public static string TableBlankCell => Get(nameof(TableBlankCell));
    public static string OfficeSlideAccessibleNameFormat => Get(nameof(OfficeSlideAccessibleNameFormat));
    public static string OfficeSheetAccessibleNameFormat => Get(nameof(OfficeSheetAccessibleNameFormat));
    public static string OfficePageAccessibleNameFormat => Get(nameof(OfficePageAccessibleNameFormat));
    public static string OfficeColumnHeaderAccessibleNameFormat => Get(nameof(OfficeColumnHeaderAccessibleNameFormat));
    public static string OfficeRowHeaderAccessibleNameFormat => Get(nameof(OfficeRowHeaderAccessibleNameFormat));
    public static string OfficeCellAccessibleNameFormat => Get(nameof(OfficeCellAccessibleNameFormat));
    public static string OfficeEmbeddedImageAccessibleName => Get(nameof(OfficeEmbeddedImageAccessibleName));

    public static string Format(string format, params object[] arguments)
        => string.Format(CultureInfo.CurrentCulture, format, arguments);

    internal static string LocalizePreviewKind(string? kind)
        => kind?.ToLowerInvariant() switch
        {
            "image" => Get("PreviewKindImage"),
            "pdf" => Get("PreviewKindPdf"),
            "text" => Get("PreviewKindText"),
            "markdown" => Get("PreviewKindMarkdown"),
            "table" => Get("PreviewKindTable"),
            "folder" => Get("PreviewKindFolder"),
            "archive" or "listing" => Get("PreviewKindArchive"),
            "office" or "document" => Get("PreviewKindDocument"),
            "presentation" => Get("PreviewKindPresentation"),
            "workbook" => Get("PreviewKindWorkbook"),
            "database" => Get("PreviewKindDatabase"),
            "package" => Get("PreviewKindPackage"),
            "certificate" => Get("PreviewKindCertificate"),
            "executable" => Get("PreviewKindExecutable"),
            "ebook" => Get("PreviewKindEbook"),
            "torrent" => Get("PreviewKindTorrent"),
            "media" => Get("PreviewKindMedia"),
            "video" => Get("PreviewKindVideo"),
            "audio" => Get("PreviewKindAudio"),
            "font" => Get("PreviewKindFont"),
            "binary" => Get("PreviewKindBinary"),
            "unknown" or null or "" => Get("PreviewKindUnknown"),
            _ => Get("PreviewKindUnknown"),
        };

    internal static string BuildPreviewStatus(string? kind, string? title)
        => Format(Get("PreviewStatusFormat"), LocalizePreviewKind(kind), title ?? string.Empty);

    private static Microsoft.Windows.ApplicationModel.Resources.ResourceLoader? CreateLoader()
    {
        try { return new Microsoft.Windows.ApplicationModel.Resources.ResourceLoader(); }
        catch { return null; }
    }

    internal static string Get(string key)
    {
        string missing = $"⟦{key}⟧";
        try
        {
            string? value = Loader.Value?.GetString(key);
            return string.IsNullOrWhiteSpace(value) || string.Equals(value, key, StringComparison.Ordinal)
                ? missing
                : value;
        }
        catch
        {
            return missing;
        }
    }
}
