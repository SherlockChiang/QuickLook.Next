using System.Runtime.InteropServices.WindowsRuntime;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.UI.Xaml.Shapes;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

// Office preview is intentionally an approximate native layout renderer. It draws the structured
// PPT/XLSX model from Rust into WinUI controls; it is not expected to match the Office rendering engine.
internal sealed class OfficePreviewPresenter
{
    private const int MaxCellsPerPage = 2048;
    private const int MaxLayoutItemsPerPage = 2048;
    private const int MaxOfficeImageReferences = 18;
    private const int MaxLegacyImageBase64Chars = 1024 * 1024;
    private static readonly SolidColorBrush OfficeWhiteBrush = new(Colors.White);
    private static readonly SolidColorBrush OfficeBlackBrush = new(Colors.Black);
    private static readonly SolidColorBrush UiGrayBrush = new(Colors.Gray);
    private static readonly SolidColorBrush OfficeBorderBrush = new(ColorHelper.FromArgb(255, 210, 210, 210));
    private static readonly SolidColorBrush OfficeCellBorderBrush = new(ColorHelper.FromArgb(255, 225, 225, 225));
    private static readonly SolidColorBrush OfficeHeaderBrush = new(ColorHelper.FromArgb(255, 246, 247, 249));
    private static readonly SolidColorBrush OfficeHeaderTextBrush = new(ColorHelper.FromArgb(255, 86, 92, 104));
    private static readonly SolidColorBrush OfficeFreezeBrush = new(ColorHelper.FromArgb(255, 0, 120, 212));

    private readonly ScrollViewer _scrollViewer;
    private readonly StackPanel _pagesPanel;
    private readonly Func<(bool Enabled, Windows.UI.Color Background, Windows.UI.Color Foreground)> _getHighContrast;
    private readonly Func<string, string, int, int, CancellationToken, Task<NativeRasterImage?>>? _loadOfficeImage;
    private readonly List<PageSlot> _pageSlots = [];
    private OfficeImageLoadSession? _imageLoadSession;
    private OfficeLayout? _layout;
    private double _maxPageWidth;
    private PreviewReady? _lastReady;
    private (double Width, double Height) _lastMaxContent;
    private bool _virtualUpdateQueued;

    public OfficePreviewPresenter(
        ScrollViewer scrollViewer,
        StackPanel pagesPanel,
        Func<(bool Enabled, Windows.UI.Color Background, Windows.UI.Color Foreground)> getHighContrast,
        Func<string, string, int, int, CancellationToken, Task<NativeRasterImage?>>? loadOfficeImage = null)
    {
        _scrollViewer = scrollViewer;
        _pagesPanel = pagesPanel;
        _getHighContrast = getHighContrast;
        _loadOfficeImage = loadOfficeImage;
        _scrollViewer.ViewChanged += (_, _) => QueueVirtualPageUpdate();
        _scrollViewer.SizeChanged += (_, _) => QueueVirtualPageUpdate();
    }

    public OfficePreviewResult Render(PreviewReady ready, (double Width, double Height) maxContent)
    {
        CancelImageLoads();
        _lastReady = ready;
        _lastMaxContent = maxContent;
        OfficeLayout layout = ready.OfficeLayout!;
        _layout = layout;

        _pagesPanel.Children.Clear();
        _pageSlots.Clear();
        _scrollViewer.ChangeView(0, 0, null, true);

        double maxPageWidth = Math.Max(360, maxContent.Width - 72);
        _maxPageWidth = maxPageWidth;
        if (_loadOfficeImage is not null && !string.IsNullOrWhiteSpace(ready.RequestId))
        {
            _imageLoadSession = new OfficeImageLoadSession(
                ready.RequestId,
                BuildImageDecodeTargets(layout, maxPageWidth));
        }
        int renderedPageCount = Math.Min(layout.Pages.Length, 16);
        foreach ((OfficePage page, int index) in layout.Pages.Take(16).Select((page, index) => (page, index)))
        {
            Border host = CreatePageHost(layout, page, maxPageWidth, index, renderedPageCount);
            var slot = new PageSlot(page, host);
            _pageSlots.Add(slot);
            _pagesPanel.Children.Add(host);
            if (index < 2)
                Materialize(slot);
        }
        QueueVirtualPageUpdate();

        var first = layout.Pages.FirstOrDefault();
        double firstWidth = first?.Width > 0 ? first.Width : layout.Width;
        double firstHeight = first?.Height > 0 ? first.Height : layout.Height;
        double scale = LayoutScale(layout, firstWidth, maxPageWidth);
        bool isWorkbook = layout.LayoutKind.Equals("workbook", StringComparison.OrdinalIgnoreCase);
        double headerWidth = isWorkbook ? 42 : 0;
        double headerHeight = isWorkbook ? 24 : 0;
        double contentWidth = Math.Min(maxContent.Width, firstWidth * scale + headerWidth + 64);
        double contentHeight = Math.Min(maxContent.Height, firstHeight * scale + headerHeight + 112);
        return new OfficePreviewResult(UiStrings.BuildPreviewStatus(ready.Kind, ready.Title), contentWidth, contentHeight);
    }

    public void RefreshPalette()
    {
        if (_lastReady is not null)
            Render(_lastReady, _lastMaxContent);
    }

    public void Clear()
    {
        CancelImageLoads();
        _layout = null;
        _lastReady = null;
        _lastMaxContent = default;
        _maxPageWidth = 0;
        _virtualUpdateQueued = false;
        _pageSlots.Clear();
        _pagesPanel.Children.Clear();
    }

    private static Border CreatePageHost(OfficeLayout layout, OfficePage page, double maxPageWidth, int index, int count)
    {
        double pageWidth = Math.Max(320, page.Width > 0 ? page.Width : layout.Width);
        double pageHeight = Math.Max(180, page.Height > 0 ? page.Height : layout.Height);
        double scale = LayoutScale(layout, pageWidth, maxPageWidth);
        bool isWorkbook = layout.LayoutKind.Equals("workbook", StringComparison.OrdinalIgnoreCase);
        double viewWidth = pageWidth * scale + (isWorkbook ? 42 : 0);
        double viewHeight = pageHeight * scale + (isWorkbook ? 24 : 0);
        var host = new Border
        {
            Width = viewWidth,
            Height = viewHeight + 24,
            HorizontalAlignment = HorizontalAlignment.Center,
        };
        string format = layout.LayoutKind.ToLowerInvariant() switch
        {
            "presentation" => UiStrings.OfficeSlideAccessibleNameFormat,
            "workbook" => UiStrings.OfficeSheetAccessibleNameFormat,
            _ => UiStrings.OfficePageAccessibleNameFormat,
        };
        AutomationProperties.SetName(host, UiStrings.Format(format, index + 1, count, page.Title));
        AutomationProperties.SetPositionInSet(host, index + 1);
        AutomationProperties.SetSizeOfSet(host, count);
        return host;
    }

    private void QueueVirtualPageUpdate()
    {
        if (_virtualUpdateQueued || _layout is null)
            return;
        _virtualUpdateQueued = true;
        if (!_pagesPanel.DispatcherQueue.TryEnqueue(() =>
        {
            _virtualUpdateQueued = false;
            UpdateVirtualPages();
        }))
        {
            _virtualUpdateQueued = false;
        }
    }

    private void UpdateVirtualPages()
    {
        if (_layout is null || _pageSlots.Count == 0)
            return;

        double viewport = _scrollViewer.ViewportHeight;
        if (!double.IsFinite(viewport) || viewport <= 0)
            return;
        double margin = viewport;
        PageSlot? pageToMaterialize = null;
        bool morePagesPending = false;
        foreach (PageSlot slot in _pageSlots)
        {
            bool nearby;
            try
            {
                double top = slot.Host.TransformToVisual(_scrollViewer).TransformPoint(new Windows.Foundation.Point()).Y;
                nearby = top < viewport + margin && top + slot.Host.Height > -margin;
            }
            catch
            {
                nearby = true;
            }

            if (!nearby)
                slot.Host.Child = null;
            else if (slot.Host.Child is null)
            {
                if (pageToMaterialize is null)
                    pageToMaterialize = slot;
                else
                    morePagesPending = true;
            }
        }
        if (pageToMaterialize is not null)
            Materialize(pageToMaterialize);
        if (morePagesPending)
            QueueVirtualPageUpdate();
    }

    private void Materialize(PageSlot slot)
    {
        if (slot.Host.Child is null && _layout is not null)
            slot.Host.Child = CreatePageView(_layout, slot.Page, _maxPageWidth);
    }

    private FrameworkElement CreatePageView(OfficeLayout layout, OfficePage page, double maxPageWidth)
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
            Foreground = ForegroundBrush(UiGrayBrush),
            Margin = new Thickness(2, 0, 0, 0),
        });

        SolidColorBrush pageBrush = DocumentBrush(page.BackgroundColor) ?? BackgroundBrush(OfficeWhiteBrush);
        var canvas = new Canvas
        {
            Width = viewWidth,
            Height = viewHeight,
            Background = pageBrush,
        };

        if (isWorkbook)
        {
            OfficeCell[] visibleCells = page.Cells.Take(MaxCellsPerPage).ToArray();
            AddWorkbookHeaders(canvas, page, visibleCells, scale, rowHeaderWidth, columnHeaderHeight, contentWidth, contentHeight);
            foreach (OfficeCell cell in visibleCells)
                AddCell(canvas, cell, scale, rowHeaderWidth, columnHeaderHeight);
            AddFreezePaneIndicators(canvas, page, visibleCells, scale, rowHeaderWidth, columnHeaderHeight, contentWidth, contentHeight);
        }

        foreach (OfficeLayoutItem item in page.Items.OrderBy(item => item.ZIndex).Take(MaxLayoutItemsPerPage))
            AddLayoutItem(canvas, item, scale, layout.LayoutKind, rowHeaderWidth, columnHeaderHeight);

        stack.Children.Add(new Border
        {
            Width = viewWidth,
            Height = viewHeight,
            Background = pageBrush,
            BorderBrush = ForegroundBrush(OfficeBorderBrush),
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

    private void AddWorkbookHeaders(
        Canvas canvas,
        OfficePage page,
        IReadOnlyList<OfficeCell> cells,
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
            Background = BackgroundBrush(OfficeHeaderBrush),
            BorderBrush = ForegroundBrush(OfficeCellBorderBrush),
            BorderThickness = new Thickness(0, 0, 1, 1),
        });

        var columnHeaders = cells
            .OrderBy(cell => cell.Column)
            .GroupBy(cell => cell.Column)
            .Select(group => group.First())
            .Take(32);
        foreach (OfficeCell cell in columnHeaders)
        {
            string column = ColumnName(cell.Column);
            var header = CreateHeaderCell(column, UiStrings.Format(UiStrings.OfficeColumnHeaderAccessibleNameFormat, column), cell.Width * scale, columnHeaderHeight);
            Canvas.SetLeft(header, rowHeaderWidth + cell.X * scale);
            Canvas.SetTop(header, 0);
            canvas.Children.Add(header);
        }

        var rowHeaders = cells
            .OrderBy(cell => cell.Row)
            .GroupBy(cell => cell.Row)
            .Select(group => group.First())
            .Take(128);
        foreach (OfficeCell cell in rowHeaders)
        {
            var header = CreateHeaderCell((cell.Row + 1).ToString(), UiStrings.Format(UiStrings.OfficeRowHeaderAccessibleNameFormat, cell.Row + 1), rowHeaderWidth, cell.Height * scale);
            Canvas.SetLeft(header, 0);
            Canvas.SetTop(header, columnHeaderHeight + cell.Y * scale);
            canvas.Children.Add(header);
        }

        var bottomLine = new Border
        {
            Width = contentWidth,
            Height = 1,
            Background = ForegroundBrush(OfficeCellBorderBrush),
        };
        Canvas.SetLeft(bottomLine, rowHeaderWidth);
        Canvas.SetTop(bottomLine, columnHeaderHeight + contentHeight);
        canvas.Children.Add(bottomLine);

        var rightLine = new Border
        {
            Width = 1,
            Height = contentHeight,
            Background = ForegroundBrush(OfficeCellBorderBrush),
        };
        Canvas.SetLeft(rightLine, rowHeaderWidth + contentWidth);
        Canvas.SetTop(rightLine, columnHeaderHeight);
        canvas.Children.Add(rightLine);
    }

    private Border CreateHeaderCell(string text, string accessibleName, double width, double height)
    {
        var textBlock = new TextBlock
        {
            Text = text,
            FontSize = 11,
            Foreground = ForegroundBrush(OfficeHeaderTextBrush),
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
            TextAlignment = TextAlignment.Center,
        };
        AutomationProperties.SetName(textBlock, accessibleName);
        return new Border
        {
            Width = Math.Max(12, width),
            Height = Math.Max(12, height),
            Background = BackgroundBrush(OfficeHeaderBrush),
            BorderBrush = ForegroundBrush(OfficeCellBorderBrush),
            BorderThickness = new Thickness(0, 0, 1, 1),
            Child = textBlock,
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

    private void AddCell(Canvas canvas, OfficeCell cell, double scale, double offsetX, double offsetY)
    {
        double width = Math.Max(12, cell.Width * scale);
        double height = Math.Max(12, cell.Height * scale);
        bool merged = cell.RowSpan > 1 || cell.ColumnSpan > 1;
        var textBlock = new TextBlock
        {
            Text = cell.Text,
            FontSize = FontSizeFor(cell.FontSize),
            FontWeight = new Windows.UI.Text.FontWeight { Weight = cell.Bold ? (ushort)600 : (ushort)400 },
            FontStyle = cell.Italic ? Windows.UI.Text.FontStyle.Italic : Windows.UI.Text.FontStyle.Normal,
            MaxWidth = Math.Max(4, width - 10),
            MaxHeight = Math.Max(4, height - 4),
            TextWrapping = cell.WrapText ? TextWrapping.Wrap : TextWrapping.NoWrap,
            TextTrimming = TextTrimming.WordEllipsis,
            Foreground = DocumentBrush(cell.TextColor) ?? ForegroundBrush(OfficeBlackBrush),
            TextAlignment = TextAlignmentFor(cell.HorizontalAlignment),
            HorizontalAlignment = HorizontalAlignmentFor(cell.HorizontalAlignment),
            VerticalAlignment = VerticalAlignmentFor(cell.VerticalAlignment),
        };
        string start = ColumnName(cell.Column) + (cell.Row + 1);
        string address = merged
            ? start + ":" + ColumnName(cell.Column + cell.ColumnSpan - 1) + (cell.Row + cell.RowSpan)
            : start;
        AutomationProperties.SetName(textBlock, UiStrings.Format(UiStrings.OfficeCellAccessibleNameFormat, address, cell.Text.Length == 0 ? UiStrings.TableBlankCell : cell.Text));
        var border = new Border
        {
            Width = width,
            Height = height,
            BorderBrush = ForegroundBrush(OfficeCellBorderBrush),
            BorderThickness = merged ? new Thickness(1.2) : new Thickness(0, 0, 1, 1),
            Padding = new Thickness(5, 2, 5, 2),
            Background = DocumentBrush(cell.FillColor)
                ?? (merged ? new SolidColorBrush(ColorHelper.FromArgb(255, 252, 253, 255)) : null),
            Child = textBlock,
        };
        Canvas.SetLeft(border, offsetX + cell.X * scale);
        Canvas.SetTop(border, offsetY + cell.Y * scale);
        canvas.Children.Add(border);
    }

    private static TextAlignment TextAlignmentFor(string? value)
        => value switch
        {
            "center" => TextAlignment.Center,
            "right" => TextAlignment.Right,
            "justify" or "distributed" => TextAlignment.Justify,
            _ => TextAlignment.Left,
        };

    private static double FontSizeFor(double? value)
        => value.HasValue ? Math.Clamp(value.Value, 6.0, 36.0) : 12.0;

    private static HorizontalAlignment HorizontalAlignmentFor(string? value)
        => value switch
        {
            "center" => HorizontalAlignment.Center,
            "right" => HorizontalAlignment.Right,
            _ => HorizontalAlignment.Left,
        };

    private static VerticalAlignment VerticalAlignmentFor(string? value)
        => value switch
        {
            "top" => VerticalAlignment.Top,
            "bottom" => VerticalAlignment.Bottom,
            _ => VerticalAlignment.Center,
        };

    private void AddFreezePaneIndicators(
        Canvas canvas,
        OfficePage page,
        IReadOnlyList<OfficeCell> cells,
        double scale,
        double offsetX,
        double offsetY,
        double contentWidth,
        double contentHeight)
    {
        if (page.FreezeColumns > 0)
        {
            double boundary = cells
                .Where(cell => cell.Column >= page.FreezeColumns)
                .Select(cell => cell.X)
                .DefaultIfEmpty(cells.Where(cell => cell.Column < page.FreezeColumns).Select(cell => cell.X + cell.Width).DefaultIfEmpty(0).Max())
                .Min();
            var line = new Border { Width = 2, Height = contentHeight, Background = OfficeFreezeBrush, Opacity = 0.72 };
            Canvas.SetLeft(line, offsetX + boundary * scale);
            Canvas.SetTop(line, offsetY);
            canvas.Children.Add(line);
        }

        if (page.FreezeRows > 0)
        {
            double boundary = cells
                .Where(cell => cell.Row >= page.FreezeRows)
                .Select(cell => cell.Y)
                .DefaultIfEmpty(cells.Where(cell => cell.Row < page.FreezeRows).Select(cell => cell.Y + cell.Height).DefaultIfEmpty(0).Max())
                .Min();
            var line = new Border { Width = contentWidth, Height = 2, Background = OfficeFreezeBrush, Opacity = 0.72 };
            Canvas.SetLeft(line, offsetX);
            Canvas.SetTop(line, offsetY + boundary * scale);
            canvas.Children.Add(line);
        }
    }

    private void AddLayoutItem(Canvas canvas, OfficeLayoutItem item, double scale, string layoutKind, double offsetX, double offsetY)
    {
        double x = offsetX + item.X * scale;
        double y = offsetY + item.Y * scale;
        double width = Math.Max(12, item.Width * scale);
        double height = Math.Max(12, item.Height * scale);

        if (item.Kind.Equals("image", StringComparison.OrdinalIgnoreCase))
        {
            if (_imageLoadSession is { } session
                && TryGetOfficeImageReference(item, out string imageRef)
                && session.Targets.TryGetValue(imageRef, out ImageDecodeTarget target))
            {
                Image image = CreateLayoutImage(item, width, height);
                Canvas.SetLeft(image, x);
                Canvas.SetTop(image, y);
                canvas.Children.Add(image);
                _ = PopulateLayoutImageAsync(image, imageRef, target, session);
                return;
            }

            // Compatibility only for PreviewReady JSON produced by the previous protocol version.
            // New native Office layouts carry imageRef and are decoded out-of-process into BGRA.
            if (!string.IsNullOrWhiteSpace(item.ImageBase64)
                && CreateImageSourceFromBase64(item.ImageBase64) is { } legacySource)
            {
                Image image = CreateLayoutImage(item, width, height);
                image.Source = legacySource;
                Canvas.SetLeft(image, x);
                Canvas.SetTop(image, y);
                canvas.Children.Add(image);
                return;
            }
        }

        if (item.Kind.Equals("shape", StringComparison.OrdinalIgnoreCase) && string.IsNullOrWhiteSpace(item.Text))
        {
            AddShape(canvas, item, x, y, width, height);
            return;
        }

        if (!string.IsNullOrWhiteSpace(item.Text))
        {
            Brush? fill = DocumentBrush(item.FillColor);
            Brush? stroke = DocumentBrush(item.StrokeColor);
            var textBox = new Border
            {
                Width = width,
                Height = height,
                Padding = layoutKind.Equals("presentation", StringComparison.OrdinalIgnoreCase)
                    ? new Thickness(6 * scale, 3 * scale, 6 * scale, 3 * scale)
                    : new Thickness(0),
                Background = fill,
                BorderBrush = stroke,
                BorderThickness = stroke is null ? new Thickness(0) : new Thickness(1),
                Child = new TextBlock
                {
                    Text = item.Text,
                    FontSize = LayoutItemFontSize(layoutKind, item, scale),
                    FontWeight = new Windows.UI.Text.FontWeight { Weight = item.Bold ? (ushort)600 : (ushort)400 },
                    FontStyle = item.Italic ? Windows.UI.Text.FontStyle.Italic : Windows.UI.Text.FontStyle.Normal,
                    TextWrapping = TextWrapping.Wrap,
                    TextTrimming = TextTrimming.WordEllipsis,
                    Foreground = ForegroundBrush(OfficeBlackBrush),
                    MaxWidth = width,
                    MaxHeight = height,
                },
            };
            Canvas.SetLeft(textBox, x);
            Canvas.SetTop(textBox, y);
            canvas.Children.Add(textBox);
        }
    }

    private static Image CreateLayoutImage(OfficeLayoutItem item, double width, double height)
    {
        var image = new Image
        {
            Width = width,
            Height = height,
            Stretch = Stretch.Uniform,
        };
        AutomationProperties.SetName(
            image,
            string.IsNullOrWhiteSpace(item.ImageName)
                ? UiStrings.OfficeEmbeddedImageAccessibleName
                : item.ImageName);
        return image;
    }

    private void AddShape(Canvas canvas, OfficeLayoutItem item, double x, double y, double width, double height)
    {
        Brush fill = DocumentBrush(item.FillColor) ?? BackgroundBrush(new SolidColorBrush(ColorHelper.FromArgb(28, 0, 0, 0)));
        Brush stroke = DocumentBrush(item.StrokeColor) ?? ForegroundBrush(OfficeBorderBrush);
        string shape = item.Shape?.ToLowerInvariant() ?? "rect";

        FrameworkElement element = shape switch
        {
            "ellipse" or "oval" => new Ellipse
            {
                Width = width,
                Height = height,
                Fill = fill,
                Stroke = stroke,
                StrokeThickness = 1,
            },
            "line" => new Line
            {
                X1 = 0,
                Y1 = 0,
                X2 = width,
                Y2 = height,
                Stroke = stroke,
                StrokeThickness = 1.5,
            },
            _ => new Border
            {
                Width = width,
                Height = height,
                Background = fill,
                BorderBrush = stroke,
                BorderThickness = new Thickness(1),
            },
        };

        Canvas.SetLeft(element, x);
        Canvas.SetTop(element, y);
        canvas.Children.Add(element);
    }

    private static double LayoutItemFontSize(string layoutKind, OfficeLayoutItem item, double scale)
    {
        if (!layoutKind.Equals("presentation", StringComparison.OrdinalIgnoreCase))
            return 12;

        return item.PlaceholderType switch
        {
            _ when item.FontSize is > 0 => Math.Clamp(item.FontSize.Value * scale, 8, 36),
            "title" or "ctrTitle" or "vertTitle" => Math.Clamp(28 * scale, 16, 30),
            "subTitle" => Math.Clamp(22 * scale, 13, 24),
            "body" or "obj" => Math.Clamp(16 * scale, 10, 18),
            _ => Math.Clamp(16 * scale, 10, 18),
        };
    }

    private static IReadOnlyDictionary<string, ImageDecodeTarget> BuildImageDecodeTargets(
        OfficeLayout layout,
        double maxPageWidth)
    {
        var targets = new Dictionary<string, ImageDecodeTarget>(StringComparer.Ordinal);
        foreach (OfficePage page in layout.Pages.Take(16))
        {
            double pageWidth = Math.Max(320, page.Width > 0 ? page.Width : layout.Width);
            double scale = LayoutScale(layout, pageWidth, maxPageWidth);
            foreach (OfficeLayoutItem item in page.Items
                         .OrderBy(candidate => candidate.ZIndex)
                         .Take(MaxLayoutItemsPerPage))
            {
                if (!item.Kind.Equals("image", StringComparison.OrdinalIgnoreCase)
                    || !TryGetOfficeImageReference(item, out string imageRef))
                {
                    continue;
                }

                int targetWidth = Math.Clamp(
                    (int)Math.Ceiling(Math.Max(12, item.Width * scale)),
                    1,
                    NativeAbi.MaxOfficeImageDimension);
                int targetHeight = Math.Clamp(
                    (int)Math.Ceiling(Math.Max(12, item.Height * scale)),
                    1,
                    NativeAbi.MaxOfficeImageDimension);
                if (targets.TryGetValue(imageRef, out ImageDecodeTarget existing))
                {
                    targets[imageRef] = new ImageDecodeTarget(
                        Math.Max(existing.Width, targetWidth),
                        Math.Max(existing.Height, targetHeight));
                }
                else if (targets.Count < MaxOfficeImageReferences)
                {
                    targets.Add(imageRef, new ImageDecodeTarget(targetWidth, targetHeight));
                }
            }
        }
        return targets;
    }

    private static bool TryGetOfficeImageReference(OfficeLayoutItem item, out string imageRef)
    {
        imageRef = item.ImageRef ?? "";
        if (imageRef.Length == 0
            || item.ImageByteLength <= 0
            || item.ImageByteLength > NativeAbi.MaxOfficeImageSourceBytes
            || imageRef.IndexOf('\0') >= 0
            || imageRef.IndexOf('\\') >= 0
            || imageRef.IndexOf(':') >= 0
            || imageRef.StartsWith("/", StringComparison.Ordinal)
            || System.Text.Encoding.UTF8.GetByteCount(imageRef) > NativeAbi.MaxOfficeImageRefUtf8Bytes)
        {
            imageRef = "";
            return false;
        }

        string[] segments = imageRef.Split('/');
        if (segments.Length < 3
            || segments[1] != "media"
            || segments.Any(segment => segment.Length == 0 || segment is "." or "..")
            || segments[0] is not ("word" or "ppt" or "xl"))
        {
            imageRef = "";
            return false;
        }

        return true;
    }

    private async Task PopulateLayoutImageAsync(
        Image image,
        string imageRef,
        ImageDecodeTarget target,
        OfficeImageLoadSession session)
    {
        if (_loadOfficeImage is null || session.Token.IsCancellationRequested)
            return;

        try
        {
            ImageSource? source = await session.GetOrAdd(
                imageRef,
                () => LoadOfficeImageSourceAsync(session, imageRef, target));
            if (source is null || !IsCurrentImageSession(session))
                return;
            AssignImageSource(image, source, session);
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "Office layout image load failed: " + ex.Message);
        }
    }

    private async Task<ImageSource?> LoadOfficeImageSourceAsync(
        OfficeImageLoadSession session,
        string imageRef,
        ImageDecodeTarget target)
    {
        if (_loadOfficeImage is null)
            return null;

        CancellationToken token = session.Token;
        await session.DecodeGate.WaitAsync(token).ConfigureAwait(false);
        try
        {
            token.ThrowIfCancellationRequested();
            NativeRasterImage? raster = await _loadOfficeImage(
                session.ParentPreviewRequestId,
                imageRef,
                target.Width,
                target.Height,
                token).ConfigureAwait(false);
            token.ThrowIfCancellationRequested();
            if (raster is null || !IsCurrentImageSession(session))
                return null;
            return await CreateBgraImageSourceAsync(raster, token).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (token.IsCancellationRequested)
        {
            return null;
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "Office imageRef decode failed: " + ex.Message);
            return null;
        }
        finally
        {
            session.DecodeGate.Release();
        }
    }

    private async Task<ImageSource?> CreateBgraImageSourceAsync(
        NativeRasterImage raster,
        CancellationToken token)
    {
        if (_pagesPanel.DispatcherQueue.HasThreadAccess)
            return CreateImageSourceFromBgra(raster);

        var completion = new TaskCompletionSource<ImageSource?>(
            TaskCreationOptions.RunContinuationsAsynchronously);
        if (!_pagesPanel.DispatcherQueue.TryEnqueue(() =>
        {
            completion.TrySetResult(token.IsCancellationRequested
                ? null
                : CreateImageSourceFromBgra(raster));
        }))
        {
            return null;
        }

        using CancellationTokenRegistration registration =
            token.Register(() => completion.TrySetCanceled(token));
        return await completion.Task.ConfigureAwait(false);
    }

    private void AssignImageSource(
        Image image,
        ImageSource source,
        OfficeImageLoadSession session)
    {
        void Assign()
        {
            if (IsCurrentImageSession(session))
                image.Source = source;
        }

        if (_pagesPanel.DispatcherQueue.HasThreadAccess)
            Assign();
        else
            _pagesPanel.DispatcherQueue.TryEnqueue(Assign);
    }

    private bool IsCurrentImageSession(OfficeImageLoadSession session)
        => ReferenceEquals(Volatile.Read(ref _imageLoadSession), session)
           && !session.Token.IsCancellationRequested;

    private void CancelImageLoads()
        => Interlocked.Exchange(ref _imageLoadSession, null)?.Cancel();

    private static ImageSource? CreateImageSourceFromBgra(NativeRasterImage raster)
    {
        if (raster.Width is <= 0 or > NativeAbi.MaxOfficeImageDimension
            || raster.Height is <= 0 or > NativeAbi.MaxOfficeImageDimension)
        {
            return null;
        }

        int expectedLength;
        try
        {
            expectedLength = checked(raster.Width * raster.Height * 4);
        }
        catch (OverflowException)
        {
            return null;
        }
        if (raster.Bgra.Length != expectedLength)
            return null;

        try
        {
            var bitmap = new WriteableBitmap(raster.Width, raster.Height);
            using (Stream stream = bitmap.PixelBuffer.AsStream())
                stream.Write(raster.Bgra, 0, expectedLength);
            bitmap.Invalidate();
            return bitmap;
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "Office BGRA bitmap creation failed: " + ex.Message);
            return null;
        }
    }

    private static ImageSource? CreateImageSourceFromBase64(string base64)
    {
        if (base64.Length > MaxLegacyImageBase64Chars)
            return null;

        try
        {
            byte[] bytes = Convert.FromBase64String(base64);
            if (bytes.LongLength > NativeAbi.MaxOfficeImageSourceBytes)
                return null;
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

    private static SolidColorBrush? BrushFromHex(string? value)
        => TryColorFromHex(value, out Windows.UI.Color color) ? new SolidColorBrush(color) : null;

    private SolidColorBrush? DocumentBrush(string? value)
        => _getHighContrast().Enabled ? null : BrushFromHex(value);

    private SolidColorBrush BackgroundBrush(SolidColorBrush fallback)
    {
        var highContrast = _getHighContrast();
        return highContrast.Enabled ? new SolidColorBrush(highContrast.Background) : fallback;
    }

    private SolidColorBrush ForegroundBrush(SolidColorBrush fallback)
    {
        var highContrast = _getHighContrast();
        return highContrast.Enabled ? new SolidColorBrush(highContrast.Foreground) : fallback;
    }

    private readonly record struct ImageDecodeTarget(int Width, int Height);

    private sealed class OfficeImageLoadSession
    {
        private readonly object _sync = new();
        private readonly Dictionary<string, Task<ImageSource?>> _loads = new(StringComparer.Ordinal);
        private readonly CancellationTokenSource _cancellation = new();
        private readonly CancellationToken _token;
        private bool _canceled;

        public OfficeImageLoadSession(
            string parentPreviewRequestId,
            IReadOnlyDictionary<string, ImageDecodeTarget> targets)
        {
            ParentPreviewRequestId = parentPreviewRequestId;
            Targets = targets;
            _token = _cancellation.Token;
        }

        public string ParentPreviewRequestId { get; }
        public IReadOnlyDictionary<string, ImageDecodeTarget> Targets { get; }
        public SemaphoreSlim DecodeGate { get; } = new(2, 2);
        public CancellationToken Token => _token;

        public Task<ImageSource?> GetOrAdd(
            string imageRef,
            Func<Task<ImageSource?>> factory)
        {
            lock (_sync)
            {
                if (_canceled)
                    return Task.FromResult<ImageSource?>(null);
                if (_loads.TryGetValue(imageRef, out Task<ImageSource?>? existing))
                    return existing;

                Task<ImageSource?> created = factory();
                _loads.Add(imageRef, created);
                return created;
            }
        }

        public void Cancel()
        {
            Task<ImageSource?>[] loads;
            lock (_sync)
            {
                if (_canceled)
                    return;
                _canceled = true;
                try
                {
                    _cancellation.Cancel();
                }
                catch
                {
                }
                loads = _loads.Values.ToArray();
            }

            if (loads.Length == 0)
            {
                DisposeResources();
                return;
            }

            _ = Task.WhenAll(loads).ContinueWith(
                _ => DisposeResources(),
                CancellationToken.None,
                TaskContinuationOptions.ExecuteSynchronously,
                TaskScheduler.Default);
        }

        private void DisposeResources()
        {
            DecodeGate.Dispose();
            _cancellation.Dispose();
        }
    }

    private sealed record PageSlot(OfficePage Page, Border Host);

    private static bool TryColorFromHex(string? value, out Windows.UI.Color color)
    {
        color = Colors.Transparent;
        if (string.IsNullOrWhiteSpace(value))
            return false;

        string hex = value.Trim().TrimStart('#');
        if (hex.Length != 6 || hex.Any(ch => !Uri.IsHexDigit(ch)))
            return false;

        byte r = Convert.ToByte(hex[..2], 16);
        byte g = Convert.ToByte(hex.Substring(2, 2), 16);
        byte b = Convert.ToByte(hex.Substring(4, 2), 16);
        color = ColorHelper.FromArgb(255, r, g, b);
        return true;
    }
}

internal readonly record struct OfficePreviewResult(string Status, double Width, double Height);
