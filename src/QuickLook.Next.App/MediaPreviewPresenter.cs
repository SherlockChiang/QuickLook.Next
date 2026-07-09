using Microsoft.UI.Xaml.Controls;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class MediaPreviewPresenter
{
    private readonly MediaPlayerElement _mediaElement;

    public MediaPreviewPresenter(MediaPlayerElement mediaElement)
    {
        _mediaElement = mediaElement;
    }

    public MediaPreviewResult Render(PreviewReady ready, (double Width, double Height) maxContent)
    {
        Clear();

        try
        {
            var uri = new Uri(ready.MediaPath!);
            _mediaElement.Source = Windows.Media.Core.MediaSource.CreateFromUri(uri);
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "media load failed: " + ex);
        }

        double width = ready.PreferredWidth > 0 ? ready.PreferredWidth : 800;
        double height = ready.PreferredHeight > 0 ? ready.PreferredHeight : 450;
        double scale = width > 0 && height > 0
            ? Math.Min(1.0, Math.Min(maxContent.Width / width, maxContent.Height / height))
            : 1.0;
        return new MediaPreviewResult($"{ready.Kind}: {ready.Title}", width * scale, height * scale);
    }

    public void Clear()
    {
        _mediaElement.MediaPlayer?.Pause();
        var source = _mediaElement.Source;
        _mediaElement.Source = null;
        if (source is IDisposable disposableSource)
            disposableSource.Dispose();
    }

    public static bool IsMediaProbe(FileProbe probe)
        => probe.Kind.Equals("video", StringComparison.OrdinalIgnoreCase)
           || probe.Kind.Equals("audio", StringComparison.OrdinalIgnoreCase)
           || probe.Kind.Equals("media", StringComparison.OrdinalIgnoreCase);
}

internal readonly record struct MediaPreviewResult(string Status, double Width, double Height);
