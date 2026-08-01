using System.Runtime.InteropServices.WindowsRuntime;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Imaging;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class ImageWaveformPresenter
{
    private const int PixelLength = ImageWaveformBuilder.ScopeWidth * ImageWaveformBuilder.ScopeHeight * 4;

    private readonly FrameworkElement _panel;
    private readonly Image _image;
    private readonly byte[] _pixels = new byte[PixelLength];
    private WriteableBitmap? _bitmap;

    public ImageWaveformPresenter(FrameworkElement panel, Image image)
    {
        _panel = panel;
        _image = image;
    }

    public void Show(ImageWaveform? waveform)
    {
        if (!ImageWaveformBuilder.IsValid(waveform))
        {
            Clear();
            return;
        }

        ArgumentNullException.ThrowIfNull(waveform);
        int planeLength = checked(waveform.Width * waveform.Height);
        byte[] density = waveform.RgbDensity;

        for (int i = 0; i < planeLength; i++)
        {
            _pixels[i * 4] = density[planeLength * 2 + i];
            _pixels[i * 4 + 1] = density[planeLength + i];
            _pixels[i * 4 + 2] = density[i];
            _pixels[i * 4 + 3] = 255;
        }

        _bitmap ??= new WriteableBitmap(waveform.Width, waveform.Height);
        using (var stream = _bitmap.PixelBuffer.AsStream())
            stream.Write(_pixels);
        _bitmap.Invalidate();
        if (!ReferenceEquals(_image.Source, _bitmap))
            _image.Source = _bitmap;
        _panel.Visibility = Visibility.Visible;
    }

    public void Clear()
    {
        _image.Source = null;
        _panel.Visibility = Visibility.Collapsed;
    }
}
