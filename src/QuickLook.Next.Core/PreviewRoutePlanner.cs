namespace QuickLook.Next.Core;

public enum PreviewRoute
{
    CloudMetadata,
    Media,
    ParserHost,
    NativeThenRaster,
    RasterHost,
}

public static class PreviewRoutePlanner
{
    public static PreviewRoute Plan(string? kind, bool mayRequireHydration, bool forceRaster)
    {
        if (mayRequireHydration)
        {
            if (IsMedia(kind)
                || string.Equals(kind, "unknown", StringComparison.OrdinalIgnoreCase)
                || !PreviewFormatPolicy.UsesCloudParserHost(kind)
                    && !string.Equals(kind, "image", StringComparison.OrdinalIgnoreCase)
                    && !string.Equals(kind, "pdf", StringComparison.OrdinalIgnoreCase))
            {
                return PreviewRoute.CloudMetadata;
            }
        }

        if (IsMedia(kind))
            return PreviewRoute.Media;
        if (PreviewFormatPolicy.UsesParserHost(kind))
            return PreviewRoute.ParserHost;
        return forceRaster ? PreviewRoute.RasterHost : PreviewRoute.NativeThenRaster;
    }

    public static bool IsMedia(string? kind)
        => string.Equals(kind, "video", StringComparison.OrdinalIgnoreCase)
            || string.Equals(kind, "audio", StringComparison.OrdinalIgnoreCase)
            || string.Equals(kind, "media", StringComparison.OrdinalIgnoreCase);
}
