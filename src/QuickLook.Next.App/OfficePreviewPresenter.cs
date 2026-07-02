using System.Runtime.InteropServices.WindowsRuntime;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class OfficePreviewPresenter
{
    private static readonly SolidColorBrush OfficeWhiteBrush = new(Colors.White);
    private static readonly SolidColorBrush OfficeBlackBrush = new(Colors.Black);
    private static readonly SolidColorBrush UiGrayBrush = new(Colors.Gray);
    private static readonly SolidColorBrush OfficeBorderBrush = new(ColorHelper.FromArgb(255, 210, 210, 210));
    private static readonly SolidColorBrush OfficeCellBorderBrush = new(ColorHelper.FromArgb(255, 225, 225, 225));

    private readonly ScrollViewer _scrollViewer;
    private readonly StackPanel _pagesPanel;

    public OfficePreviewPresenter(ScrollViewer scrollViewer, StackPanel pagesPanel)
    {
        _scrollViewer = scrollViewer;
        _pagesPanel = pagesPanel;
    }

    public OfficePreviewResult Render(PreviewReady ready, (double Width, double Height) maxContent)
    {
        OfficeLayout layout = ready.OfficeLayout!;

        _pagesPanel.Children.Clear();
        _scrollViewer.ChangeView(0, 0, null, true);

        double maxPageWidth = Math.Max(360, maxContent.Width - 72);
        foreach (OfficePage page in layout.Pages.Take(16))
            _pagesPanel.Children.Add(CreatePageView(layout, page, maxPageWidth));

        var first = layout.Pages.FirstOrDefault();
        double firstWidth = first?.Width > 0 ? first.Width : layout.Width;
        double firstHeight = first?.Height > 0 ? first.Height : layout.Height;
        double scale = LayoutScale(layout, firstWidth, maxPageWidth);
        double contentWidth = Math.Min(maxContent.Width, firstWidth * scale + 64);
        double contentHeight = Math.Min(maxContent.Height, firstHeight * scale + 112);
        return new OfficePreviewResult($"{ready.Kind}: {ready.Title}", contentWidth, contentHeight);
    }

    private static FrameworkElement CreatePageView(OfficeLayout layout, OfficePage page, double maxPageWidth)
    {
        double pageWidth = Math.Max(320, page.Width > 0 ? page.Width : layout.Width);
        double pageHeight = Math.Max(180, page.Height > 0 ? page.Height : layout.Height);
        double scale = LayoutScale(layout, pageWidth, maxPageWidth);
        double viewWidth = pageWidth * scale;
        double viewHeight = pageHeight * scale;

        var stack = new StackPanel { Spacing = 6 };
        stack.Children.Add(new TextBlock
        {
            Text = page.Title,
            FontSize = 12,
            Foreground = UiGrayBrush,
            Margin = new Thickness(2, 0, 0, 0),
        });

        var canvas = new Canvas
        {
            Width = viewWidth,
            Height = viewHeight,
            Background = OfficeWhiteBrush,
        };

        if (layout.LayoutKind.Equals("workbook", StringComparison.OrdinalIgnoreCase))
        {
            foreach (OfficeCell cell in page.Cells)
                AddCell(canvas, cell, scale);
        }

        foreach (OfficeLayoutItem item in page.Items)
            AddLayoutItem(canvas, item, scale, layout.LayoutKind);

        stack.Children.Add(new Border
        {
            Width = viewWidth,
            Height = viewHeight,
            Background = OfficeWhiteBrush,
            BorderBrush = OfficeBorderBrush,
            BorderThickness = new Thickness(1),
            Child = canvas,
        });
        return stack;
    }

    private static double LayoutScale(OfficeLayout layout, double pageWidth, double maxPageWidth)
    {
        double target = layout.LayoutKind.Equals("presentation", StringComparison.OrdinalIgnoreCase)
            ? Math.Min(1.0, maxPageWidth / Math.Max(1, pageWidth))
            : Math.Min(1.0, maxPageWidth / Math.Max(1, pageWidth));
        return Math.Clamp(target, 0.35, 1.0);
    }

    private static void AddCell(Canvas canvas, OfficeCell cell, double scale)
    {
        var border = new Border
        {
            Width = Math.Max(12, cell.Width * scale),
            Height = Math.Max(12, cell.Height * scale),
            BorderBrush = OfficeCellBorderBrush,
            BorderThickness = new Thickness(0, 0, 1, 1),
            Padding = new Thickness(5, 2, 5, 2),
            Child = new TextBlock
            {
                Text = cell.Text,
                FontSize = 12,
                TextTrimming = TextTrimming.CharacterEllipsis,
                Foreground = OfficeBlackBrush,
                VerticalAlignment = VerticalAlignment.Center,
            },
        };
        Canvas.SetLeft(border, cell.X * scale);
        Canvas.SetTop(border, cell.Y * scale);
        canvas.Children.Add(border);
    }

    private static void AddLayoutItem(Canvas canvas, OfficeLayoutItem item, double scale, string layoutKind)
    {
        double x = item.X * scale;
        double y = item.Y * scale;
        double width = Math.Max(12, item.Width * scale);
        double height = Math.Max(12, item.Height * scale);

        if (item.Kind.Equals("image", StringComparison.OrdinalIgnoreCase)
            && !string.IsNullOrWhiteSpace(item.ImageBase64)
            && CreateImageSourceFromBase64(item.ImageBase64) is { } source)
        {
            var image = new Image
            {
                Source = source,
                Width = width,
                Height = height,
                Stretch = Stretch.Uniform,
            };
            Canvas.SetLeft(image, x);
            Canvas.SetTop(image, y);
            canvas.Children.Add(image);
            return;
        }

        if (!string.IsNullOrWhiteSpace(item.Text))
        {
            var text = new TextBlock
            {
                Text = item.Text,
                FontSize = layoutKind.Equals("presentation", StringComparison.OrdinalIgnoreCase) ? Math.Max(12, 15 * scale) : 12,
                TextWrapping = TextWrapping.Wrap,
                Foreground = OfficeBlackBrush,
                MaxWidth = width,
                MaxHeight = height,
            };
            Canvas.SetLeft(text, x);
            Canvas.SetTop(text, y);
            canvas.Children.Add(text);
        }
    }

    private static ImageSource? CreateImageSourceFromBase64(string base64)
    {
        try
        {
            byte[] bytes = Convert.FromBase64String(base64);
            var bitmap = new BitmapImage();
            using var memory = new MemoryStream(bytes);
            bitmap.SetSource(memory.AsRandomAccessStream());
            return bitmap;
        }
        catch
        {
            return null;
        }
    }
}

internal readonly record struct OfficePreviewResult(string Status, double Width, double Height);
