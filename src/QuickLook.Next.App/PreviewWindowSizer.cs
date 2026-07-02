using Microsoft.UI;
using Microsoft.UI.Windowing;
using Windows.Graphics;

namespace QuickLook.Next.App;

internal static class PreviewWindowSizer
{
    private const double WindowHorizontalChrome = 48;
    private const double WindowVerticalChrome = 72;
    private const double MinWindowWidth = 360;
    private const double MinWindowHeight = 260;
    private const double MaxWindowScreenRatio = 0.86;

    public static (double Width, double Height) GetMaxContentSize(
        WindowId windowId,
        double preferredMaxWidth,
        double preferredMaxHeight)
    {
        SizeInt32 maxWindow = GetMaxWindowSize(windowId, preferredMaxWidth, preferredMaxHeight);
        return (
            Math.Max(1, maxWindow.Width - WindowHorizontalChrome),
            Math.Max(1, maxWindow.Height - WindowVerticalChrome));
    }

    public static SizeInt32 GetWindowSizeForContent(
        WindowId windowId,
        double contentWidth,
        double contentHeight,
        double preferredMaxWidth,
        double preferredMaxHeight)
    {
        SizeInt32 maxWindow = GetMaxWindowSize(windowId, preferredMaxWidth, preferredMaxHeight);
        if (!double.IsFinite(contentWidth) || contentWidth <= 0)
            contentWidth = MinWindowWidth - WindowHorizontalChrome;
        if (!double.IsFinite(contentHeight) || contentHeight <= 0)
            contentHeight = MinWindowHeight - WindowVerticalChrome;

        double width = Math.Clamp(contentWidth + WindowHorizontalChrome, MinWindowWidth, maxWindow.Width);
        double height = Math.Clamp(contentHeight + WindowVerticalChrome, MinWindowHeight, maxWindow.Height);
        return new SizeInt32((int)Math.Round(width), (int)Math.Round(height));
    }

    public static PointInt32? GetCenteredPosition(WindowId windowId, SizeInt32 size)
    {
        try
        {
            DisplayArea? displayArea = DisplayArea.GetFromWindowId(windowId, DisplayAreaFallback.Nearest);
            if (displayArea is null)
                return null;

            RectInt32 workArea = displayArea.WorkArea;
            int x = workArea.X + Math.Max(0, (workArea.Width - size.Width) / 2);
            int y = workArea.Y + Math.Max(0, (workArea.Height - size.Height) / 2);
            return new PointInt32(x, y);
        }
        catch
        {
            return null;
        }
    }

    private static SizeInt32 GetMaxWindowSize(
        WindowId windowId,
        double preferredMaxWidth,
        double preferredMaxHeight)
    {
        double screenMaxWidth = preferredMaxWidth;
        double screenMaxHeight = preferredMaxHeight;

        try
        {
            DisplayArea? displayArea = DisplayArea.GetFromWindowId(windowId, DisplayAreaFallback.Nearest);
            if (displayArea is not null)
            {
                screenMaxWidth = Math.Max(MinWindowWidth, displayArea.WorkArea.Width * MaxWindowScreenRatio);
                screenMaxHeight = Math.Max(MinWindowHeight, displayArea.WorkArea.Height * MaxWindowScreenRatio);
            }
        }
        catch
        {
            // DisplayArea is best-effort; fall back to the conservative per-kind caps.
        }

        int width = (int)Math.Round(Math.Max(MinWindowWidth, Math.Min(preferredMaxWidth, screenMaxWidth)));
        int height = (int)Math.Round(Math.Max(MinWindowHeight, Math.Min(preferredMaxHeight, screenMaxHeight)));
        return new SizeInt32(width, height);
    }
}
