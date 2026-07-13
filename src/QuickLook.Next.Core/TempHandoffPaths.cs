namespace QuickLook.Next.Core;

/// <summary>Validates ParserHost-owned temporary files before the App consumes them.</summary>
public static class TempHandoffPaths
{
    private const int MaxPathChars = 32 * 1024;

    public static bool IsArchiveExtractPath(string path, string? tempRoot = null)
        => IsValidPath(path, Path.Combine(tempRoot ?? Path.GetTempPath(), "QuickLookNext", "archive-preview"),
            directory => directory.StartsWith("extract-", StringComparison.Ordinal),
            file => file.StartsWith("entry-", StringComparison.Ordinal));

    public static bool IsHeroRasterPath(string path, string requestId, string? tempRoot = null)
    {
        if (requestId.Length != 32 || !requestId.All(static c => char.IsAsciiHexDigit(c)))
            return false;

        return IsValidPath(path, Path.Combine(tempRoot ?? Path.GetTempPath(), "QuickLookNext", "parser-raster"),
            directory => string.Equals(directory, "raster-" + requestId, StringComparison.Ordinal),
            file => string.Equals(file, "hero.bgra", StringComparison.Ordinal));
    }

    public static bool IsRasterAnimationPath(string path, string requestId, string? tempRoot = null)
    {
        if (requestId.Length != 32 || !requestId.All(static c => char.IsAsciiHexDigit(c)))
            return false;

        return IsValidPath(path, Path.Combine(tempRoot ?? Path.GetTempPath(), "QuickLookNext", "raster-animation"),
            directory => string.Equals(directory, "frames-" + requestId, StringComparison.Ordinal),
            file => string.Equals(file, "frames.bin", StringComparison.Ordinal));
    }

    private static bool IsValidPath(string path, string rootPath, Func<string, bool> isDirectoryNameValid, Func<string, bool> isFileNameValid)
    {
        if (string.IsNullOrWhiteSpace(path) || path.Length > MaxPathChars)
            return false;

        try
        {
            string root = Path.GetFullPath(rootPath);
            string fullPath = Path.GetFullPath(path);
            string prefix = root.EndsWith(Path.DirectorySeparatorChar) ? root : root + Path.DirectorySeparatorChar;
            if (!fullPath.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)
                || !File.Exists(fullPath)
                || IsReparsePoint(root)
                || IsReparsePoint(fullPath))
                return false;

            string? directory = Path.GetDirectoryName(fullPath);
            return directory is not null
                && string.Equals(Path.GetDirectoryName(directory), root, StringComparison.OrdinalIgnoreCase)
                && isDirectoryNameValid(Path.GetFileName(directory))
                && isFileNameValid(Path.GetFileName(fullPath))
                && !IsReparsePoint(directory);
        }
        catch (Exception ex) when (ex is ArgumentException or IOException or UnauthorizedAccessException or NotSupportedException)
        {
            return false;
        }
    }

    private static bool IsReparsePoint(string path) => (File.GetAttributes(path) & FileAttributes.ReparsePoint) != 0;
}
