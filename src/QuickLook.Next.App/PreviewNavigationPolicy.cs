namespace QuickLook.Next.App;

internal enum PreviewNavigationKey
{
    Other,
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
}

internal enum PreviewNavigationAction
{
    None,
    PreviousPdfPage,
    NextPdfPage,
    PreviousImage,
    NextImage,
}

internal static class PreviewNavigationPolicy
{
    public static PreviewNavigationAction Resolve(
        bool pdfVisible,
        bool imageVisible,
        bool modifierDown,
        PreviewNavigationKey key)
    {
        if (modifierDown)
            return PreviewNavigationAction.None;

        if (pdfVisible)
        {
            return key switch
            {
                PreviewNavigationKey.Left or PreviewNavigationKey.Up or PreviewNavigationKey.PageUp
                    => PreviewNavigationAction.PreviousPdfPage,
                PreviewNavigationKey.Right or PreviewNavigationKey.Down or PreviewNavigationKey.PageDown
                    => PreviewNavigationAction.NextPdfPage,
                _ => PreviewNavigationAction.None,
            };
        }

        if (!imageVisible)
            return PreviewNavigationAction.None;

        return key switch
        {
            PreviewNavigationKey.Left => PreviewNavigationAction.PreviousImage,
            PreviewNavigationKey.Right => PreviewNavigationAction.NextImage,
            _ => PreviewNavigationAction.None,
        };
    }
}
