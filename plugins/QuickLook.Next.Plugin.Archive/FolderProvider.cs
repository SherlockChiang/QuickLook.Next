using QuickLook.Next.Contracts;

namespace QuickLook.Next.Plugin.Archive;

/// <summary>Shows the first directory level quickly; the app lazily loads child folders on navigation.</summary>
public sealed class FolderProvider : IPreviewProvider
{
    private const int MaxItems = 5000;

    public bool CanHandle(FileProbe probe)
        => probe.Kind.Equals("folder", StringComparison.OrdinalIgnoreCase) || Directory.Exists(probe.Path);

    public Task<PreviewResult> OpenAsync(string path, FileProbe probe, IPreviewContext context)
    {
        context.ReportStatus("FolderProvider: reading folder...");
        var root = new DirectoryInfo(path);
        var items = ReadFolderLevel(root.FullName, "", context.Cancellation, out long bytes, out int files, out int folders, out int skipped, out bool partial);

        string summary = $"{files:N0} files, {folders:N0} folders - {FormatBytes(bytes)}";
        if (skipped > 0)
            summary += $" - {skipped:N0} inaccessible";
        if (partial)
            summary += " - partial";

        var listing = new PreviewListing(string.IsNullOrWhiteSpace(root.Name) ? root.FullName : root.Name, root.FullName, "folder")
        {
            Summary = summary,
            IsPartial = partial,
            Items = items,
        };

        return Task.FromResult(new PreviewResult("folder", $"{listing.RootName} - {files:N0} files, {folders:N0} folders")
        {
            PreferredWidth = 980,
            PreferredHeight = 720,
            Listing = listing,
        });
    }

    private static PreviewListingItem[] ReadFolderLevel(
        string physicalPath,
        string virtualParent,
        CancellationToken cancellation,
        out long bytes,
        out int files,
        out int folders,
        out int skipped,
        out bool partial)
    {
        var items = new List<PreviewListingItem>();
        bytes = 0;
        files = 0;
        folders = 0;
        skipped = 0;
        partial = false;

        foreach (string dir in SafeEnumerateDirectories(physicalPath, ref skipped))
        {
            cancellation.ThrowIfCancellationRequested();
            if (items.Count >= MaxItems)
            {
                partial = true;
                break;
            }

            var info = new DirectoryInfo(dir);
            folders++;
            items.Add(new PreviewListingItem(info.Name, CombineVirtual(virtualParent, info.Name, isFolder: true), virtualParent, IsFolder: true)
            {
                ModifiedUnix = ToUnixSeconds(info.LastWriteTimeUtc),
                Type = "Folder",
                NativePath = info.FullName,
            });
        }

        if (!partial)
        {
            foreach (string file in SafeEnumerateFiles(physicalPath, ref skipped))
            {
                cancellation.ThrowIfCancellationRequested();
                if (items.Count >= MaxItems)
                {
                    partial = true;
                    break;
                }

                var info = new FileInfo(file);
                long size = SafeLength(info);
                files++;
                bytes += size;
                items.Add(new PreviewListingItem(info.Name, CombineVirtual(virtualParent, info.Name, isFolder: false), virtualParent, IsFolder: false)
                {
                    Size = size,
                    ModifiedUnix = ToUnixSeconds(info.LastWriteTimeUtc),
                    Type = TypeFor(info.Name),
                    NativePath = info.FullName,
                });
            }
        }

        return items
            .OrderByDescending(e => e.IsFolder)
            .ThenBy(e => e.Name, StringComparer.OrdinalIgnoreCase)
            .ToArray();
    }

    private static IEnumerable<string> SafeEnumerateDirectories(string path, ref int skipped)
    {
        try { return Directory.EnumerateDirectories(path).ToArray(); }
        catch { skipped++; return []; }
    }

    private static IEnumerable<string> SafeEnumerateFiles(string path, ref int skipped)
    {
        try { return Directory.EnumerateFiles(path).ToArray(); }
        catch { skipped++; return []; }
    }

    private static string CombineVirtual(string parent, string name, bool isFolder)
    {
        string path = string.IsNullOrEmpty(parent) ? name : parent.TrimEnd('/') + "/" + name;
        return isFolder ? path + "/" : path;
    }

    private static long SafeLength(FileInfo info)
    {
        try { return info.Length; }
        catch { return 0; }
    }

    private static string TypeFor(string name)
    {
        string ext = Path.GetExtension(name);
        return ext.Length > 0 ? $"{ext.TrimStart('.').ToUpperInvariant()} File" : "File";
    }

    private static long ToUnixSeconds(DateTime value)
        => value == default ? 0 : new DateTimeOffset(value, TimeSpan.Zero).ToUnixTimeSeconds();

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
