namespace QuickLook.Next.Core;

public enum PreviewHostKind
{
    Raster,
    Parser,
    ShellBroker,
}

public static class HostExecutablePath
{
    /// <summary>
    /// Resolves a host inside the deployed layout. If it is missing, returns the
    /// expected deployed path so the lazy launcher reports its normal startup failure.
    /// </summary>
    public static string Resolve(PreviewHostKind kind, string baseDirectory)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(baseDirectory);
        if (!Path.IsPathFullyQualified(baseDirectory))
            throw new ArgumentException("The App base directory must be absolute.", nameof(baseDirectory));
        string folder = kind switch
        {
            PreviewHostKind.Raster => "RasterHost",
            PreviewHostKind.Parser => "ParserHost",
            PreviewHostKind.ShellBroker => "ShellBroker",
            _ => throw new ArgumentOutOfRangeException(nameof(kind)),
        };
        string executable = $"QuickLook.Next.{folder}.exe";
        string deployed = Path.GetFullPath(Path.Combine(baseDirectory, folder, executable));
        if (File.Exists(deployed))
            return deployed;
        string flat = Path.GetFullPath(Path.Combine(baseDirectory, executable));
        if (File.Exists(flat))
            return flat;
#if DEBUG
        // src/App/bin/Debug/<tfm>/<rid> -> sibling project output under src.
        string framework = kind == PreviewHostKind.Parser ? "net10.0-windows" : "net10.0-windows10.0.19041.0";
        string development = Path.GetFullPath(Path.Combine(baseDirectory,
            "..", "..", "..", "..", "..", $"QuickLook.Next.{folder}",
            "bin", "Debug", framework, "win-x64", executable));
        if (File.Exists(development))
            return development;
#endif
        return deployed;
    }
}
