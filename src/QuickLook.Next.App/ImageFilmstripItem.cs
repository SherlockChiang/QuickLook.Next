using System.ComponentModel;
using System.Runtime.CompilerServices;
using Microsoft.UI.Xaml.Media;

namespace QuickLook.Next.App;

public sealed class ImageFilmstripItem : INotifyPropertyChanged
{
    private ImageSource? _thumbnail;

    public event PropertyChangedEventHandler? PropertyChanged;

    public required string Path { get; init; }
    public required string Name { get; init; }

    public ImageSource? Thumbnail
    {
        get => _thumbnail;
        set
        {
            if (ReferenceEquals(_thumbnail, value))
                return;
            _thumbnail = value;
            OnPropertyChanged();
        }
    }

    private void OnPropertyChanged([CallerMemberName] string? name = null)
        => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}
