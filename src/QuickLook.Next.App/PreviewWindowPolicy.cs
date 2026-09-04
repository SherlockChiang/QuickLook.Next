namespace QuickLook.Next.App;

internal enum PreviewWindowLevel
{
    Standard,
    AlwaysOnTop,
}

internal static class PreviewWindowPolicy
{
    public const string StandardLevelSetting = "standard";
    public const string AlwaysOnTopLevelSetting = "always-on-top";

    public static bool IsSupportedLevel(string? value)
        => value is StandardLevelSetting or AlwaysOnTopLevelSetting;

    public static PreviewWindowLevel ParseLevel(string? value)
        => value == AlwaysOnTopLevelSetting
            ? PreviewWindowLevel.AlwaysOnTop
            : PreviewWindowLevel.Standard;

    public static bool ShouldCloseAfterDeactivation(
        bool closeOnFocusLoss,
        bool previewVisible,
        bool windowActivated,
        bool modalDialogOpen,
        bool ownedWindowOpen)
        => closeOnFocusLoss
            && previewVisible
            && !windowActivated
            && !modalDialogOpen
            && !ownedWindowOpen;
}
