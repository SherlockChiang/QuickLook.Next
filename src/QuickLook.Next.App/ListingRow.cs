using QuickLook.Next.Contracts;

namespace QuickLook.Next.App;

public sealed class ListingRow
{
    public ListingRow(PreviewListingItem item)
    {
        Name = item.Name;
        Path = item.Path;
        NativePath = item.NativePath;
        IsFolder = item.IsFolder;
        Glyph = item.IsFolder ? "\uE8B7" : "\uE8A5";
        TypeDisplay = item.IsFolder ? "文件夹" : item.Type;
        SizeDisplay = item.IsFolder ? "" : MainWindow.FormatBytes(item.Size);
        ModifiedDisplay = item.ModifiedUnix > 0
            ? DateTimeOffset.FromUnixTimeSeconds(item.ModifiedUnix).LocalDateTime.ToString("g")
            : "";
    }

    public string Name { get; }
    public string Path { get; }
    public string? NativePath { get; }
    public bool IsFolder { get; }
    public string Glyph { get; }
    public string ModifiedDisplay { get; }
    public string TypeDisplay { get; }
    public string SizeDisplay { get; }
}
