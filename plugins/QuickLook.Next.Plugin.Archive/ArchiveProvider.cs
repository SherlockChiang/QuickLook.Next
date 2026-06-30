using System.IO.Compression;
using QuickLook.Next.Contracts;

namespace QuickLook.Next.Plugin.Archive;

/// <summary>Lists ZIP-family archives as a compact file-manager style tree.</summary>
public sealed class ArchiveProvider : IPreviewProvider
{
    private const int MaxEntriesToRender = 5000;

    private static readonly HashSet<string> ZipExts = new(StringComparer.OrdinalIgnoreCase)
    {
        ".zip", ".jar", ".apk", ".epub", ".nupkg", ".vsix", ".whl", ".cbz", ".xpi",
    };

    public bool CanHandle(FileProbe probe)
    {
        if (ZipExts.Contains(probe.Extension)) return true;
        return probe.Kind.Equals("archive", StringComparison.OrdinalIgnoreCase)
               && probe.MagicPrefix.Length >= 2
               && probe.MagicPrefix[0] == 0x50
               && probe.MagicPrefix[1] == 0x4B;
    }

    public Task<PreviewResult> OpenAsync(string path, FileProbe probe, IPreviewContext context)
    {
        context.ReportStatus("ArchiveProvider: reading archive...");
        using var zip = ZipFile.OpenRead(path);

        var entries = new Dictionary<string, PreviewListingItem>(StringComparer.OrdinalIgnoreCase);
        long uncompressed = 0;
        long compressed = 0;
        int fileCount = 0;
        int folderCount = 0;
        int seen = 0;
        bool partial = false;

        foreach (var entry in zip.Entries)
        {
            string fullName = NormalizePath(entry.FullName);
            if (fullName.Length == 0) continue;

            bool isFolder = IsFolder(entry, fullName);
            if (isFolder)
            {
                if (entries.Count >= MaxEntriesToRender)
                {
                    partial = true;
                    continue;
                }

                AddParentFolders(fullName, entries);
                if (entries.Count < MaxEntriesToRender && AddFolder(entries, EnsureTrailingSlash(fullName)))
                    folderCount++;
            }
            else
            {
                fileCount++;
                uncompressed += entry.Length;
                compressed += entry.CompressedLength;
                if (seen++ < MaxEntriesToRender && entries.Count < MaxEntriesToRender)
                {
                    AddParentFolders(fullName, entries);
                    if (entries.Count >= MaxEntriesToRender)
                    {
                        partial = true;
                        continue;
                    }

                    entries[fullName] = new PreviewListingItem(
                        Path.GetFileName(fullName),
                        fullName,
                        ParentOf(fullName),
                        IsFolder: false)
                    {
                        Size = entry.Length,
                        PackedSize = entry.CompressedLength,
                        ModifiedUnix = ToUnixSeconds(entry.LastWriteTime),
                        Type = TypeFor(fullName),
                    };
                }
                else partial = true;
            }
        }

        folderCount = entries.Values.Count(e => e.IsFolder);
        var file = new FileInfo(path);
        string summary = $"{fileCount:N0} files, {folderCount:N0} folders";
        if (uncompressed > 0)
            summary += $" - {FormatBytes(uncompressed)} uncompressed";
        if (compressed > 0 && uncompressed > 0)
            summary += $" - {Math.Clamp(100.0 - compressed * 100.0 / uncompressed, 0, 100):0.#}% saved";

        var listing = new PreviewListing(Path.GetFileName(path), "", "archive")
        {
            Summary = summary,
            IsPartial = partial,
            Items = entries.Values
                .OrderByDescending(e => e.IsFolder)
                .ThenBy(e => e.Path, StringComparer.OrdinalIgnoreCase)
                .ToArray(),
        };

        return Task.FromResult(new PreviewResult("archive", $"{Path.GetFileName(path)} - {zip.Entries.Count:N0} entries")
        {
            PreferredWidth = 980,
            PreferredHeight = 720,
            Listing = listing,
        });
    }

    private static bool IsFolder(ZipArchiveEntry entry, string normalizedPath)
        => normalizedPath.EndsWith('/') || string.IsNullOrEmpty(entry.Name);

    private static string NormalizePath(string path)
        => path.Replace('\\', '/').TrimStart('/');

    private static string EnsureTrailingSlash(string path)
        => path.EndsWith('/') ? path : path + "/";

    private static void AddParentFolders(string path, Dictionary<string, PreviewListingItem> entries)
    {
        int index = 0;
        while ((index = path.IndexOf('/', index)) >= 0)
        {
            if (entries.Count >= MaxEntriesToRender)
                return;

            AddFolder(entries, path[..(index + 1)]);
            index++;
        }
    }

    private static bool AddFolder(Dictionary<string, PreviewListingItem> entries, string path)
    {
        path = EnsureTrailingSlash(path);
        if (entries.ContainsKey(path))
            return false;

        string trimmed = path.TrimEnd('/');
        entries[path] = new PreviewListingItem(
            Path.GetFileName(trimmed),
            path,
            ParentOf(trimmed),
            IsFolder: true)
        {
            Type = "Folder",
        };
        return true;
    }

    private static string ParentOf(string path)
    {
        path = path.TrimEnd('/');
        int slash = path.LastIndexOf('/');
        return slash < 0 ? "" : path[..(slash + 1)];
    }

    private static string TypeFor(string path)
    {
        string ext = Path.GetExtension(path);
        return ext.Length > 0 ? $"{ext.TrimStart('.').ToUpperInvariant()} File" : "File";
    }

    private static long ToUnixSeconds(DateTimeOffset value)
        => value == default ? 0 : value.ToUnixTimeSeconds();

    private static string FormatBytes(long bytes)
    {
        string[] units = ["B", "KB", "MB", "GB", "TB"];
        double value = bytes;
        int unit = 0;
        while (value >= 1024 && unit < units.Length - 1)
        {
            value /= 1024;
            unit++;
        }
        return unit == 0 ? $"{bytes:N0} B" : $"{value:0.##} {units[unit]}";
    }
}
