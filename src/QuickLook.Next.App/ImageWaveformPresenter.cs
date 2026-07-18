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
        if (!ImageWaveformBuilder.IsValid(waveform))
        {
            Clear();
            return;
        }

        ArgumentNullException.ThrowIfNull(waveform);
        int planeLength = checked(waveform.Width * waveform.Height);
        byte[] density = waveform.RgbDensity;

        var pixels = new byte[checked(planeLength * 4)];
        for (int i = 0; i < planeLength; i++)
        {
            pixels[i * 4] = density[planeLength * 2 + i];
            pixels[i * 4 + 1] = density[planeLength + i];
            pixels[i * 4 + 2] = density[i];
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
