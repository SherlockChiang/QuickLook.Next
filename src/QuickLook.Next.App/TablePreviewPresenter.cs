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

    private readonly ScrollViewer _scrollViewer;
    private readonly TextBlock _titleText;
    private readonly TextBlock _summaryText;
    private readonly Grid _grid;
    private readonly Func<ElementTheme> _getTheme;

    public TablePreviewPresenter(
        ScrollViewer scrollViewer,
        TextBlock titleText,
        TextBlock summaryText,
        Grid grid,
        Func<ElementTheme> getTheme)
    {
        _scrollViewer = scrollViewer;
        _titleText = titleText;
        _summaryText = summaryText;
        _grid = grid;
        _getTheme = getTheme;
    }

    public TablePreviewResult Render(PreviewReady ready, (double Width, double Height) maxContent)
    {
        PreviewTable table = ready.Table!;
        _titleText.Text = ready.Title;
        _summaryText.Text = BuildSummary(table);
        _grid.Children.Clear();
        _grid.RowDefinitions.Clear();
        _grid.ColumnDefinitions.Clear();
        _scrollViewer.ChangeView(0, 0, null, true);

        int columnCount = Math.Clamp(table.Headers.Length, 1, 64);
        double[] widths = EstimateColumnWidths(table, columnCount);

        _grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(RowHeaderWidth) });
        foreach (double width in widths)
            _grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(width) });

        _grid.RowDefinitions.Add(new RowDefinition { Height = new GridLength(HeaderHeight) });
        AddCell("", 0, 0, RowHeaderWidth, HeaderHeight, TableCellKind.Corner);
        for (int c = 0; c < columnCount; c++)
            AddCell(table.Headers.ElementAtOrDefault(c) ?? $"Column {c + 1}", 0, c + 1, widths[c], HeaderHeight, TableCellKind.Header);

        for (int r = 0; r < table.Rows.Length; r++)
        {
            _grid.RowDefinitions.Add(new RowDefinition { Height = new GridLength(RowHeight) });
            int rowIndex = r + 1;
            AddCell(rowIndex.ToString(), rowIndex, 0, RowHeaderWidth, RowHeight, TableCellKind.RowHeader);
            string[] cells = table.Rows[r].Cells;
            for (int c = 0; c < columnCount; c++)
            {
                string value = c < cells.Length ? cells[c] : "";
                AddCell(value, rowIndex, c + 1, widths[c], RowHeight, r % 2 == 0 ? TableCellKind.Cell : TableCellKind.AlternateCell);
            }
        }

        double tableWidth = RowHeaderWidth + widths.Sum();
        double tableHeight = HeaderHeight + table.Rows.Length * RowHeight;
        double widthTarget = Math.Clamp(Math.Min(tableWidth + 72, maxContent.Width), 560, maxContent.Width);
        double heightTarget = Math.Clamp(Math.Min(tableHeight + 132, maxContent.Height), 320, maxContent.Height);
        return new TablePreviewResult($"{ready.Kind}: {ready.Title}", widthTarget, heightTarget);
    }

    public void Clear()
    {
        _grid.Children.Clear();
        _grid.RowDefinitions.Clear();
        _grid.ColumnDefinitions.Clear();
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

    private void AddCell(string text, int row, int column, double width, double height, TableCellKind kind)
    {
        var border = new Border
        {
            Width = width,
            Height = height,
            Padding = kind is TableCellKind.Header or TableCellKind.RowHeader or TableCellKind.Corner
                ? new Thickness(9, 0, 9, 0)
                : new Thickness(10, 0, 10, 0),
            Background = BrushFor(kind),
            BorderBrush = BrushFor(TableCellKind.GridLine),
            BorderThickness = new Thickness(0, 0, 1, 1),
            Child = new TextBlock
            {
                Text = text,
                FontSize = kind is TableCellKind.Header or TableCellKind.RowHeader ? 12 : 13,
                FontWeight = new Windows.UI.Text.FontWeight { Weight = kind is TableCellKind.Header ? (ushort)600 : (ushort)400 },
                FontFamily = new FontFamily("Segoe UI"),
                Foreground = BrushFor(kind is TableCellKind.Header or TableCellKind.RowHeader or TableCellKind.Corner
                    ? TableCellKind.HeaderText
                    : TableCellKind.Text),
                VerticalAlignment = VerticalAlignment.Center,
                TextTrimming = TextTrimming.CharacterEllipsis,
                TextWrapping = TextWrapping.NoWrap,
                MaxWidth = Math.Max(8, width - 18),
            },
        };
        Grid.SetRow(border, row);
        Grid.SetColumn(border, column);
        _grid.Children.Add(border);
    }

    private SolidColorBrush BrushFor(TableCellKind kind)
    {
        bool dark = _getTheme() != ElementTheme.Light;
        Windows.UI.Color color = kind switch
        {
            TableCellKind.Header or TableCellKind.RowHeader or TableCellKind.Corner
                => dark ? ColorHelper.FromArgb(255, 45, 45, 48) : ColorHelper.FromArgb(255, 246, 247, 249),
            TableCellKind.AlternateCell
                => dark ? ColorHelper.FromArgb(22, 255, 255, 255) : ColorHelper.FromArgb(255, 250, 251, 252),
            TableCellKind.GridLine
                => dark ? ColorHelper.FromArgb(255, 62, 62, 66) : ColorHelper.FromArgb(255, 226, 230, 235),
            TableCellKind.HeaderText
                => dark ? ColorHelper.FromArgb(255, 218, 222, 230) : ColorHelper.FromArgb(255, 76, 83, 96),
            TableCellKind.Text
                => dark ? ColorHelper.FromArgb(255, 244, 244, 244) : ColorHelper.FromArgb(255, 28, 31, 36),
            _ => dark ? ColorHelper.FromArgb(255, 32, 32, 32) : Colors.White,
        };
        return new SolidColorBrush(color);
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
