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
        Glyph = item.IsFolder ? "\uE8B7" : "\uE8A5";
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
}
