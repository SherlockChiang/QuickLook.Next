namespace QuickLook.Next.Core;

public static class PreviewFormatPolicy
{
    private static readonly HashSet<string> ParserHostKinds = new(StringComparer.OrdinalIgnoreCase)
    {
        "archive", "package", "office", "text", "ebook", "executable", "torrent", "certificate",
    };

    public static bool UsesParserHost(string? kind)
        => kind is not null && ParserHostKinds.Contains(kind);
}
