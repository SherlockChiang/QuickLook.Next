using QuickLook.Next.Contracts;

namespace QuickLook.Next.Core;

public static class TablePresentationPolicy
{
    public const int MaxRows = 4_000;
    public const int MaxColumns = 64;
    public const int MaxCells = 65_536;
    public const int MaxCharacters = 512 * 1024;
    public const int MaxCellCharacters = 240;

    public static PreviewTable Bound(PreviewTable source)
    {
        int representedColumns = source.Rows
            .Take(MaxRows)
            .Select(row => row.Cells.Length)
            .DefaultIfEmpty(0)
            .Max();
        int columns = Math.Clamp(Math.Max(1, Math.Max(source.Headers.Length, representedColumns)), 1, MaxColumns);
        int characters = 0;
        bool partial = source.IsPartial || source.Headers.Length > columns;
        string[] headers = Enumerable.Range(0, columns)
            .Select(index => BoundText(source.Headers.ElementAtOrDefault(index) ?? "", ref characters, ref partial))
            .ToArray();
        var rows = new List<PreviewTableRow>();
        int cells = 0;
        foreach (PreviewTableRow row in source.Rows)
        {
            int rowCells = Math.Min(row.Cells.Length, columns);
            partial |= row.Cells.Length > columns;
            if (rows.Count >= MaxRows || cells > MaxCells - rowCells)
            {
                partial = true;
                break;
            }
            var bounded = new List<string>(rowCells);
            for (int index = 0; index < rowCells; index++)
            {
                bounded.Add(BoundText(row.Cells[index] ?? "", ref characters, ref partial));
                if (characters >= MaxCharacters)
                {
                    partial = true;
                    break;
                }
            }
            rows.Add(new PreviewTableRow(bounded.ToArray()));
            cells += rowCells;
            if (characters >= MaxCharacters)
                break;
        }
        partial |= rows.Count < source.Rows.Length;
        return source with
        {
            Headers = headers,
            Rows = rows.ToArray(),
            TotalRows = Math.Max(source.TotalRows, rows.Count),
            TotalColumns = Math.Max(source.TotalColumns, columns),
            IsPartial = partial,
        };
    }

    private static string BoundText(string value, ref int characters, ref bool partial)
    {
        int remaining = Math.Max(0, MaxCharacters - characters);
        int length = Math.Min(value.Length, Math.Min(MaxCellCharacters, remaining));
        if (length < value.Length)
            partial = true;
        characters += length;
        return length == value.Length ? value : value[..length];
    }
}
