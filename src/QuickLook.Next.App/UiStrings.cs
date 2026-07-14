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
    public static string OpeningFileFormat => Get(nameof(OpeningFileFormat), "Opening {0}...");
    public static string DownloadingCloudFileFormat => Get(nameof(DownloadingCloudFileFormat), "Downloading {0} from cloud storage...");
    public static string CheckingFileAvailabilityFormat => Get(nameof(CheckingFileAvailabilityFormat), "Checking {0} availability safely...");
    public static string CloudUnknownDeferred => Get(nameof(CloudUnknownDeferred), "Preview is deferred because this cloud file type cannot be identified without downloading its contents.");
    public static string CloudAvailabilityUnknownDeferred => Get(nameof(CloudAvailabilityUnknownDeferred), "Preview is deferred because file availability could not be verified without reading its contents.");
    public static string CloudMediaDeferred => Get(nameof(CloudMediaDeferred), "Media playback is deferred until the cloud provider makes this file available locally.");
    public static string CloudMediaAvailabilityUnknownDeferred => Get(nameof(CloudMediaAvailabilityUnknownDeferred), "Media playback is deferred because file availability could not be verified safely.");

    public static string TrayShowPreview => Get(nameof(TrayShowPreview), "Show preview");
    public static string TraySettings => Get(nameof(TraySettings), "Settings");
    public static string TrayAutoStart => Get(nameof(TrayAutoStart), "Start with Windows");
    public static string TrayExit => Get(nameof(TrayExit), "Exit QuickLook Next");
    public static string WelcomeTitle => Get(nameof(WelcomeTitle), "Welcome to QuickLook Next");
    public static string WelcomeHeading => Get(nameof(WelcomeHeading), "Preview files without breaking your flow");
    public static string WelcomeIntroduction => Get(nameof(WelcomeIntroduction), "QuickLook Next is running in the notification area and is ready to preview files from File Explorer.");
    public static string WelcomeOpenShortcut => Get(nameof(WelcomeOpenShortcut), "1. Select a file in File Explorer, then press Space.");
    public static string WelcomeCloseShortcut => Get(nameof(WelcomeCloseShortcut), "2. Press Space again to close the preview.");
    public static string WelcomeNavigationShortcut => Get(nameof(WelcomeNavigationShortcut), "Use the arrow keys in File Explorer to move between files while the preview stays open.");
    public static string WelcomeTrayBehavior => Get(nameof(WelcomeTrayBehavior), "Closing the preview keeps QuickLook Next running in the notification area.");
    public static string WelcomeHelpHint => Get(nameof(WelcomeHelpHint), "You can reopen this guide from Settings under Help and shortcuts.");
    public static string WelcomeStart => Get(nameof(WelcomeStart), "Start previewing");
    public static string AutoStartEnableFailed => Get(nameof(AutoStartEnableFailed), "Could not enable start with Windows");
    public static string AutoStartDisableFailed => Get(nameof(AutoStartDisableFailed), "Could not disable start with Windows");
    public static string SettingsTitle => Get(nameof(SettingsTitle), "Settings");
    public static string SettingsGeneral => Get(nameof(SettingsGeneral), "General");
    public static string SettingsGeneralDescription => Get(nameof(SettingsGeneralDescription), "Choose how QuickLook Next behaves in Windows.");
    public static string SettingsAutoStart => Get(nameof(SettingsAutoStart), "Start with Windows");
    public static string SettingsAutoStartDescription => Get(nameof(SettingsAutoStartDescription), "Keep QuickLook Next ready in the notification area after you sign in.");
    public static string SettingsLanguage => Get(nameof(SettingsLanguage), "Language");
    public static string SettingsLanguageDescription => Get(nameof(SettingsLanguageDescription), "Choose the language used by the app and tray menu.");
    public static string SettingsSystemLanguage => Get(nameof(SettingsSystemLanguage), "Use system language");
    public static string SettingsRestartTitle => Get(nameof(SettingsRestartTitle), "Restart required");
    public static string SettingsRestartMessage => Get(nameof(SettingsRestartMessage), "Exit QuickLook Next from the tray and start it again to apply the language.");
    public static string SettingsAbout => Get(nameof(SettingsAbout), "About");
    public static string SettingsAboutDescription => Get(nameof(SettingsAboutDescription), "Version, project source, and release information.");
    public static string SettingsVersionFormat => Get(nameof(SettingsVersionFormat), "Version {0}");
    public static string SettingsProjectSource => Get(nameof(SettingsProjectSource), "QuickLook Next is an open-source project developed on GitHub. Builds published by this project originate from the repository below.");
    public static string SettingsHelpShortcuts => Get(nameof(SettingsHelpShortcuts), "Help and shortcuts");
    public static string TextFindPlaceholder => Get(nameof(TextFindPlaceholder), "Find");
    public static string TextFindCountFormat => Get(nameof(TextFindCountFormat), "{0:N0} / {1:N0}");
    public static string SettingsOpenGitHub => Get(nameof(SettingsOpenGitHub), "Open GitHub project");
    public static string SettingsViewReleases => Get(nameof(SettingsViewReleases), "View releases");
    public static string SettingsLicenseNotice => Get(nameof(SettingsLicenseNotice), "The project has not yet published a formal software license. See the repository for the current terms and source history.");
    public static string SettingsSaveFailed => Get(nameof(SettingsSaveFailed), "Setting not saved");
    public static string SettingsSaveFailedMessage => Get(nameof(SettingsSaveFailedMessage), "QuickLook Next could not save this setting.");

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
    public static string PdfPageAccessibleNameFormat => Get(nameof(PdfPageAccessibleNameFormat), "Page {0:N0} of {1:N0}");
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
