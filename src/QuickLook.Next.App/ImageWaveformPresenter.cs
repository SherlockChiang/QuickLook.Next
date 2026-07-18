using System.Runtime.InteropServices.WindowsRuntime;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Imaging;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class ImageWaveformPresenter
{
    private readonly FrameworkElement _panel;
    private readonly Image _image;

    public ImageWaveformPresenter(FrameworkElement panel, Image image)
    {
        _panel = panel;
        _image = image;
    }

    public void Show(ImageWaveform? waveform)
    {
        if (waveform is null
            || waveform.Width != ImageWaveformDimensions.Width
            || waveform.Height != ImageWaveformDimensions.Height)
        {
            Clear();
            return;
        }

        int planeLength = checked(waveform.Width * waveform.Height);
        if (waveform.RgbDensity.Length != checked(planeLength * 3))
        {
            Clear();
            return;
        }

        var pixels = new byte[checked(planeLength * 4)];
        for (int i = 0; i < planeLength; i++)
        {
            pixels[i * 4] = waveform.RgbDensity[planeLength * 2 + i];
            pixels[i * 4 + 1] = waveform.RgbDensity[planeLength + i];
            pixels[i * 4 + 2] = waveform.RgbDensity[i];
            pixels[i * 4 + 3] = 255;
        }

        var bitmap = new WriteableBitmap(waveform.Width, waveform.Height);
        using (var stream = bitmap.PixelBuffer.AsStream())
            stream.Write(pixels);
        bitmap.Invalidate();
        _image.Source = bitmap;
        _panel.Visibility = Visibility.Visible;
    }

    public void Clear()
    {
        _image.Source = null;
        _panel.Visibility = Visibility.Collapsed;
    }
}

internal static class ImageWaveformDimensions
{
    public const int Width = 192;
    public const int Height = 96;
}
