namespace QuickLook.Next.Core;

public static class PreviewFormatPolicy
{
    private static readonly HashSet<string> ParserHostKinds = new(StringComparer.OrdinalIgnoreCase)
    {
        "archive", "package", "office", "text", "ebook", "executable", "torrent", "certificate", "database",
    };

    private static readonly HashSet<string> CloudParserHostKinds = new(StringComparer.OrdinalIgnoreCase)
    {
        "text", "ebook", "executable", "torrent", "certificate", "database",
    };

    public static bool UsesParserHost(string? kind)
        => kind is not null && ParserHostKinds.Contains(kind);

    public static bool UsesCloudParserHost(string? kind)
        => kind is not null && CloudParserHostKinds.Contains(kind);
}
