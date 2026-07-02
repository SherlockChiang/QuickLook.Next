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
    private static readonly SolidColorBrush OfficeHeaderBrush = new(ColorHelper.FromArgb(255, 246, 247, 249));
    private static readonly SolidColorBrush OfficeHeaderTextBrush = new(ColorHelper.FromArgb(255, 86, 92, 104));

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
        bool isWorkbook = layout.LayoutKind.Equals("workbook", StringComparison.OrdinalIgnoreCase);
        double headerWidth = isWorkbook ? 42 : 0;
        double headerHeight = isWorkbook ? 24 : 0;
        double contentWidth = Math.Min(maxContent.Width, firstWidth * scale + headerWidth + 64);
        double contentHeight = Math.Min(maxContent.Height, firstHeight * scale + headerHeight + 112);
        return new OfficePreviewResult($"{ready.Kind}: {ready.Title}", contentWidth, contentHeight);
    }

    private static FrameworkElement CreatePageView(OfficeLayout layout, OfficePage page, double maxPageWidth)
    {
        double pageWidth = Math.Max(320, page.Width > 0 ? page.Width : layout.Width);
        double pageHeight = Math.Max(180, page.Height > 0 ? page.Height : layout.Height);
        double scale = LayoutScale(layout, pageWidth, maxPageWidth);
        bool isWorkbook = layout.LayoutKind.Equals("workbook", StringComparison.OrdinalIgnoreCase);
        double rowHeaderWidth = isWorkbook ? 42 : 0;
        double columnHeaderHeight = isWorkbook ? 24 : 0;
        double contentWidth = pageWidth * scale;
        double contentHeight = pageHeight * scale;
        double viewWidth = contentWidth + rowHeaderWidth;
        double viewHeight = contentHeight + columnHeaderHeight;

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

        if (isWorkbook)
        {
            AddWorkbookHeaders(canvas, page, scale, rowHeaderWidth, columnHeaderHeight, contentWidth, contentHeight);
            foreach (OfficeCell cell in page.Cells)
                AddCell(canvas, cell, scale, rowHeaderWidth, columnHeaderHeight);
        }

        foreach (OfficeLayoutItem item in page.Items)
            AddLayoutItem(canvas, item, scale, layout.LayoutKind, rowHeaderWidth, columnHeaderHeight);

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

    private static void AddWorkbookHeaders(
        Canvas canvas,
        OfficePage page,
        double scale,
        double rowHeaderWidth,
        double columnHeaderHeight,
        double contentWidth,
        double contentHeight)
    {
        canvas.Children.Add(new Border
        {
            Width = rowHeaderWidth,
            Height = columnHeaderHeight,
            Background = OfficeHeaderBrush,
            BorderBrush = OfficeCellBorderBrush,
            BorderThickness = new Thickness(0, 0, 1, 1),
        });

        var columnHeaders = page.Cells
            .OrderBy(cell => cell.Column)
            .GroupBy(cell => cell.Column)
            .Select(group => group.First())
            .Take(32);
        foreach (OfficeCell cell in columnHeaders)
        {
            var header = CreateHeaderCell(ColumnName(cell.Column), cell.Width * scale, columnHeaderHeight);
            Canvas.SetLeft(header, rowHeaderWidth + cell.X * scale);
            Canvas.SetTop(header, 0);
            canvas.Children.Add(header);
        }

        var rowHeaders = page.Cells
            .OrderBy(cell => cell.Row)
            .GroupBy(cell => cell.Row)
            .Select(group => group.First())
            .Take(128);
        foreach (OfficeCell cell in rowHeaders)
        {
            var header = CreateHeaderCell((cell.Row + 1).ToString(), rowHeaderWidth, cell.Height * scale);
            Canvas.SetLeft(header, 0);
            Canvas.SetTop(header, columnHeaderHeight + cell.Y * scale);
            canvas.Children.Add(header);
        }

        var bottomLine = new Border
        {
            Width = contentWidth,
            Height = 1,
            Background = OfficeCellBorderBrush,
        };
        Canvas.SetLeft(bottomLine, rowHeaderWidth);
        Canvas.SetTop(bottomLine, columnHeaderHeight + contentHeight);
        canvas.Children.Add(bottomLine);

        var rightLine = new Border
        {
            Width = 1,
            Height = contentHeight,
            Background = OfficeCellBorderBrush,
        };
        Canvas.SetLeft(rightLine, rowHeaderWidth + contentWidth);
        Canvas.SetTop(rightLine, columnHeaderHeight);
        canvas.Children.Add(rightLine);
    }

    private static Border CreateHeaderCell(string text, double width, double height)
    {
        return new Border
        {
            Width = Math.Max(12, width),
            Height = Math.Max(12, height),
            Background = OfficeHeaderBrush,
            BorderBrush = OfficeCellBorderBrush,
            BorderThickness = new Thickness(0, 0, 1, 1),
            Child = new TextBlock
            {
                Text = text,
                FontSize = 11,
                Foreground = OfficeHeaderTextBrush,
                HorizontalAlignment = HorizontalAlignment.Center,
                VerticalAlignment = VerticalAlignment.Center,
                TextAlignment = TextAlignment.Center,
            },
        };
    }

    private static string ColumnName(int zeroBasedColumn)
    {
        int value = zeroBasedColumn + 1;
        string name = "";
        while (value > 0)
        {
            value--;
            name = (char)('A' + value % 26) + name;
            value /= 26;
        }
        return name;
    }

    private static void AddCell(Canvas canvas, OfficeCell cell, double scale, double offsetX, double offsetY)
    {
        double width = Math.Max(12, cell.Width * scale);
        double height = Math.Max(12, cell.Height * scale);
        var border = new Border
        {
            Width = width,
            Height = height,
            BorderBrush = OfficeCellBorderBrush,
            BorderThickness = new Thickness(0, 0, 1, 1),
            Padding = new Thickness(5, 2, 5, 2),
            Child = new TextBlock
            {
                Text = cell.Text,
                FontSize = 12,
                MaxWidth = Math.Max(4, width - 10),
                MaxHeight = Math.Max(4, height - 4),
                TextWrapping = TextWrapping.Wrap,
                TextTrimming = TextTrimming.WordEllipsis,
                Foreground = OfficeBlackBrush,
                VerticalAlignment = VerticalAlignment.Center,
            },
        };
        Canvas.SetLeft(border, offsetX + cell.X * scale);
        Canvas.SetTop(border, offsetY + cell.Y * scale);
        canvas.Children.Add(border);
    }

    private static void AddLayoutItem(Canvas canvas, OfficeLayoutItem item, double scale, string layoutKind, double offsetX, double offsetY)
    {
        double x = offsetX + item.X * scale;
        double y = offsetY + item.Y * scale;
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
            var textBox = new Border
            {
                Width = width,
                Height = height,
                Padding = layoutKind.Equals("presentation", StringComparison.OrdinalIgnoreCase)
                    ? new Thickness(6 * scale, 3 * scale, 6 * scale, 3 * scale)
                    : new Thickness(0),
                Child = new TextBlock
                {
                    Text = item.Text,
                    FontSize = layoutKind.Equals("presentation", StringComparison.OrdinalIgnoreCase) ? Math.Clamp(16 * scale, 10, 18) : 12,
                    TextWrapping = TextWrapping.Wrap,
                    TextTrimming = TextTrimming.WordEllipsis,
                    Foreground = OfficeBlackBrush,
                    MaxWidth = width,
                    MaxHeight = height,
                },
            };
            Canvas.SetLeft(textBox, x);
            Canvas.SetTop(textBox, y);
            canvas.Children.Add(textBox);
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
