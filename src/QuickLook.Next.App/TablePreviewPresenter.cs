using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class TablePreviewPresenter
{
    private const double RowHeaderWidth = 48;
    private const double MinColumnWidth = 96;
    private const double MaxColumnWidth = 240;
    private const double HeaderHeight = 34;
    private const double RowHeight = 32;
    private static readonly FontFamily TableFontFamily = new("Segoe UI");

    private readonly ScrollViewer _scrollViewer;
    private readonly TextBlock _titleText;
    private readonly TextBlock _summaryText;
    private readonly Canvas _canvas;
    private readonly Func<ElementTheme> _getTheme;
    private PreviewTable? _table;
    private double[] _widths = [];
    private TablePalette? _palette;
    private int _firstRenderedRow = -1;
    private int _lastRenderedRow = -1;
    private int _firstRenderedColumn = -1;
    private int _lastRenderedColumn = -1;
    private bool _headerRendered;

    public TablePreviewPresenter(
        ScrollViewer scrollViewer,
        TextBlock titleText,
        TextBlock summaryText,
        Canvas canvas,
        Func<ElementTheme> getTheme)
    {
        _scrollViewer = scrollViewer;
        _titleText = titleText;
        _summaryText = summaryText;
        _canvas = canvas;
        _getTheme = getTheme;
        _scrollViewer.ViewChanged += OnScrollViewerViewChanged;
        _scrollViewer.SizeChanged += OnScrollViewerSizeChanged;
    }

    public TablePreviewResult Render(PreviewReady ready, (double Width, double Height) maxContent)
    {
        PreviewTable table = ready.Table!;
        _titleText.Text = ready.Title;
        _summaryText.Text = BuildSummary(table);
        _scrollViewer.ChangeView(0, 0, null, true);

        int columnCount = Math.Clamp(table.Headers.Length, 1, 64);
        _table = table;
        _widths = EstimateColumnWidths(table, columnCount);
        _palette = new TablePalette(_getTheme() != ElementTheme.Light);

        double tableWidth = RowHeaderWidth + _widths.Sum();
        double tableHeight = HeaderHeight + table.Rows.Length * RowHeight;
        _canvas.Width = tableWidth;
        _canvas.Height = tableHeight;
        ResetRenderedRange();
        RenderViewport();
        double widthTarget = Math.Clamp(Math.Min(tableWidth + 72, maxContent.Width), 560, maxContent.Width);
        double heightTarget = Math.Clamp(Math.Min(tableHeight + 132, maxContent.Height), 320, maxContent.Height);
        return new TablePreviewResult($"{ready.Kind}: {ready.Title}", widthTarget, heightTarget);
    }

    public void Clear()
    {
        _table = null;
        _widths = [];
        _palette = null;
        ResetRenderedRange();
        _canvas.Children.Clear();
        _canvas.Width = 0;
        _canvas.Height = 0;
        _titleText.Text = "";
        _summaryText.Text = "";
    }

    private static string BuildSummary(PreviewTable table)
    {
        string summary = $"{table.TotalRows:N0} rows x {table.TotalColumns:N0} columns";
        if (table.IsPartial)
            summary += $" - showing {table.Rows.Length:N0} rows";
        return $"{table.Format.ToUpperInvariant()} table - {summary}";
    }

    private static double[] EstimateColumnWidths(PreviewTable table, int columnCount)
    {
        var widths = new double[columnCount];
        for (int c = 0; c < columnCount; c++)
        {
            int chars = table.Headers.ElementAtOrDefault(c)?.Length ?? 8;
            foreach (PreviewTableRow row in table.Rows.Take(80))
            {
                if (c < row.Cells.Length)
                    chars = Math.Max(chars, Math.Min(row.Cells[c].Length, 32));
            }
            widths[c] = Math.Clamp(chars * 7.8 + 28, MinColumnWidth, MaxColumnWidth);
        }
        return widths;
    }

    private void OnScrollViewerViewChanged(object? sender, ScrollViewerViewChangedEventArgs e) => RenderViewport();

    private void OnScrollViewerSizeChanged(object sender, SizeChangedEventArgs e) => RenderViewport();

    private void RenderViewport()
    {
        if (_table is null || _palette is null || _widths.Length == 0)
            return;

        var canvasOrigin = _canvas.TransformToVisual(_scrollViewer).TransformPoint(new Windows.Foundation.Point());
        double left = Math.Max(0, -canvasOrigin.X);
        double top = Math.Max(0, -canvasOrigin.Y);
        double right = left + _scrollViewer.ViewportWidth;
        double bottom = top + _scrollViewer.ViewportHeight;
        if (right <= left || bottom <= top)
            return;

        int firstColumn = 0;
        double columnLeft = RowHeaderWidth;
        while (firstColumn < _widths.Length && columnLeft + _widths[firstColumn] <= left)
        {
            columnLeft += _widths[firstColumn];
            firstColumn++;
        }

        int lastColumn = firstColumn;
        double columnRight = columnLeft;
        while (lastColumn < _widths.Length && columnRight < right)
            columnRight += _widths[lastColumn++];

        int firstRow = Math.Max(0, (int)Math.Floor((top - HeaderHeight) / RowHeight));
        int lastRow = Math.Min(_table.Rows.Length, (int)Math.Ceiling((bottom - HeaderHeight) / RowHeight));
        bool showHeader = top < HeaderHeight;
        if (firstRow == _firstRenderedRow
            && lastRow == _lastRenderedRow
            && firstColumn == _firstRenderedColumn
            && lastColumn == _lastRenderedColumn
            && showHeader == _headerRendered)
            return;

        _firstRenderedRow = firstRow;
        _lastRenderedRow = lastRow;
        _firstRenderedColumn = firstColumn;
        _lastRenderedColumn = lastColumn;
        _headerRendered = showHeader;
        _canvas.Children.Clear();

        if (showHeader)
        {
            AddCell("", 0, 0, RowHeaderWidth, HeaderHeight, TableCellKind.Corner, _palette);
            double x = columnLeft;
            for (int c = firstColumn; c < lastColumn; c++)
            {
                AddCell(_table.Headers.ElementAtOrDefault(c) ?? $"Column {c + 1}", x, 0, _widths[c], HeaderHeight, TableCellKind.Header, _palette);
                x += _widths[c];
            }
        }

        for (int r = firstRow; r < lastRow; r++)
        {
            double y = HeaderHeight + r * RowHeight;
            AddCell((r + 1).ToString(), 0, y, RowHeaderWidth, RowHeight, TableCellKind.RowHeader, _palette);
            string[] cells = _table.Rows[r].Cells;
            double x = columnLeft;
            for (int c = firstColumn; c < lastColumn; c++)
            {
                AddCell(c < cells.Length ? cells[c] : "", x, y, _widths[c], RowHeight, r % 2 == 0 ? TableCellKind.Cell : TableCellKind.AlternateCell, _palette);
                x += _widths[c];
            }
        }
    }

    private void ResetRenderedRange()
    {
        _firstRenderedRow = -1;
        _lastRenderedRow = -1;
        _firstRenderedColumn = -1;
        _lastRenderedColumn = -1;
        _headerRendered = false;
    }

    private void AddCell(string text, double x, double y, double width, double height, TableCellKind kind, TablePalette palette)
    {
        var border = new Border
        {
            Width = width,
            Height = height,
            Padding = kind is TableCellKind.Header or TableCellKind.RowHeader or TableCellKind.Corner
                ? new Thickness(9, 0, 9, 0)
                : new Thickness(10, 0, 10, 0),
            Background = palette.For(kind),
            BorderBrush = palette.GridLine,
            BorderThickness = new Thickness(0, 0, 1, 1),
            Child = new TextBlock
            {
                Text = text,
                FontSize = kind is TableCellKind.Header or TableCellKind.RowHeader ? 12 : 13,
                FontWeight = new Windows.UI.Text.FontWeight { Weight = kind is TableCellKind.Header ? (ushort)600 : (ushort)400 },
                FontFamily = TableFontFamily,
                Foreground = palette.For(kind is TableCellKind.Header or TableCellKind.RowHeader or TableCellKind.Corner
                    ? TableCellKind.HeaderText
                    : TableCellKind.Text),
                VerticalAlignment = VerticalAlignment.Center,
                TextTrimming = TextTrimming.CharacterEllipsis,
                TextWrapping = TextWrapping.NoWrap,
                MaxWidth = Math.Max(8, width - 18),
            },
        };
        Canvas.SetLeft(border, x);
        Canvas.SetTop(border, y);
        _canvas.Children.Add(border);
    }

    private sealed class TablePalette
    {
        private readonly bool _dark;
        private readonly Dictionary<TableCellKind, SolidColorBrush> _brushes;

        public TablePalette(bool dark)
        {
            _dark = dark;
            _brushes = [];
            GridLine = For(TableCellKind.GridLine);
        }

        public SolidColorBrush GridLine { get; }

        public SolidColorBrush For(TableCellKind kind) => _brushes.TryGetValue(kind, out var brush)
            ? brush
            : _brushes[kind] = new SolidColorBrush(kind switch
            {
                TableCellKind.Header or TableCellKind.RowHeader or TableCellKind.Corner => _dark ? ColorHelper.FromArgb(255, 45, 45, 48) : ColorHelper.FromArgb(255, 246, 247, 249),
                TableCellKind.AlternateCell => _dark ? ColorHelper.FromArgb(22, 255, 255, 255) : ColorHelper.FromArgb(255, 250, 251, 252),
                TableCellKind.GridLine => _dark ? ColorHelper.FromArgb(255, 62, 62, 66) : ColorHelper.FromArgb(255, 226, 230, 235),
                TableCellKind.HeaderText => _dark ? ColorHelper.FromArgb(255, 218, 222, 230) : ColorHelper.FromArgb(255, 76, 83, 96),
                TableCellKind.Text => _dark ? ColorHelper.FromArgb(255, 244, 244, 244) : ColorHelper.FromArgb(255, 28, 31, 36),
                _ => _dark ? ColorHelper.FromArgb(255, 32, 32, 32) : Colors.White,
            });
    }

    private enum TableCellKind
    {
        Cell,
        AlternateCell,
        Header,
        RowHeader,
        Corner,
        GridLine,
        HeaderText,
        Text,
    }
}

internal readonly record struct TablePreviewResult(string Status, double Width, double Height);
