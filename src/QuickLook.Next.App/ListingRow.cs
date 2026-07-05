using QuickLook.Next.Contracts;
using Microsoft.UI.Xaml.Media;
using System.ComponentModel;
using System.Runtime.CompilerServices;

namespace QuickLook.Next.App;

public sealed class ListingRow : INotifyPropertyChanged
{
    public ListingRow(PreviewListingItem item)
    {
        Name = item.Name;
        Path = item.Path;
        NativePath = item.NativePath;
        IsFolder = item.IsFolder;
        Glyph = ChooseGlyph(item);
        TypeDisplay = item.IsFolder ? UiStrings.FolderTypeDisplay : item.Type;
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

    private ImageSource? _iconSource;
    public ImageSource? IconSource
    {
        get => _iconSource;
        set
        {
            if (ReferenceEquals(_iconSource, value))
                return;
            _iconSource = value;
            OnPropertyChanged();
        }
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    private void OnPropertyChanged([CallerMemberName] string? propertyName = null)
        => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));

    private static string ChooseGlyph(PreviewListingItem item)
    {
        if (item.IsFolder)
            return "\uE8B7";

        string ext = System.IO.Path.GetExtension(item.Name).ToLowerInvariant();
        return ext switch
        {
            ".png" or ".jpg" or ".jpeg" or ".gif" or ".bmp" or ".webp" or ".tif" or ".tiff" or ".ico" => "\uEB9F",
            ".mp4" or ".mkv" or ".mov" or ".avi" or ".webm" or ".wmv" => "\uE8B2",
            ".mp3" or ".wav" or ".flac" or ".aac" or ".m4a" or ".ogg" => "\uE8D6",
            ".zip" or ".rar" or ".7z" or ".tar" or ".tgz" or ".gz" => "\uF012",
            ".pdf" => "\uEA90",
            ".epub" or ".fb2" or ".mobi" or ".azw" or ".azw3" => "\uE8A5",
            ".doc" or ".docx" or ".xls" or ".xlsx" or ".ppt" or ".pptx" or ".odt" or ".ods" or ".odp" => "\uE8A5",
            ".txt" or ".md" or ".log" or ".csv" or ".json" or ".xml" or ".yaml" or ".yml" => "\uE8A5",
            ".cs" or ".rs" or ".js" or ".ts" or ".py" or ".java" or ".cpp" or ".h" or ".ps1" or ".bat" or ".cmd" or ".sh" => "\uE943",
            ".exe" or ".dll" or ".msi" or ".appx" or ".msix" or ".apk" => "\uE756",
            ".cer" or ".crt" or ".pem" or ".pfx" => "\uEB95",
            ".torrent" => "\uE896",
            ".iso" or ".img" => "\uEDA2",
            _ => "\uE8A5",
        };
    }
}
