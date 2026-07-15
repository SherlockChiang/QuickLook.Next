using System.Text;
using QuickLook.Next.Contracts;

namespace QuickLook.Next.Core;

public sealed record MarkdownVisibleTextIndex(string Text, IReadOnlyList<MarkdownVisibleSegment> Segments);
public sealed record MarkdownVisibleSegment(int Start, string Text);

public static class TextSearchIndex
{
    public const int MaxMarkdownTableColumns = 64;
    public const int MaxMarkdownTableCells = 4096;
    public const int MaxMarkdownInlineDepth = 16;
    public const int MaxMarkdownBlocks = 2000;

    public static List<int> FindMatches(string text, string query)
    {
        var matches = new List<int>();
        query = query.Trim();
        if (query.Length == 0)
            return matches;
        int index = 0;
        while ((index = text.IndexOf(query, index, StringComparison.OrdinalIgnoreCase)) >= 0)
        {
            matches.Add(index);
            index += query.Length;
        }
        return matches;
    }

    public static string BuildMarkdownVisibleText(PreviewMarkdown document, string partialNotice)
        => BuildMarkdownVisibleTextIndex(document, partialNotice).Text;

    public static MarkdownVisibleTextIndex BuildMarkdownVisibleTextIndex(
        PreviewMarkdown document,
        string partialNotice)
    {
        var text = new StringBuilder();
        var segments = new List<MarkdownVisibleSegment>();
        int blocks = 0;
        bool truncated = false;
        foreach (PreviewMarkdownBlock block in document.Blocks)
        {
            if (block.Kind is "unorderedList" or "orderedList")
            {
                foreach (PreviewMarkdownBlock item in block.Children)
                {
                    if (blocks++ >= MaxMarkdownBlocks)
                    {
                        truncated = true;
                        break;
                    }
                    AppendSegment(text, segments, MarkdownInlineText(item.Inlines, item.Text));
                }
                if (truncated)
                    break;
                continue;
            }

            if (blocks++ >= MaxMarkdownBlocks)
            {
                truncated = true;
                break;
            }
            if (block.Kind == "table")
            {
                foreach (string cell in MarkdownTableCells(block))
                    AppendSegment(text, segments, cell);
            }
            else if (block.Kind != "thematicBreak")
            {
                AppendSegment(text, segments, MarkdownInlineText(block.Inlines, block.Text));
            }
        }
        if (document.IsPartial || truncated)
            AppendSegment(text, segments, partialNotice);
        return new MarkdownVisibleTextIndex(text.ToString(), segments);
    }

    public static string MarkdownInlineText(
        IReadOnlyList<PreviewMarkdownInline> inlines,
        string fallbackText)
        => MarkdownInlineText(inlines, fallbackText, 0);

    private static string MarkdownInlineText(
        IReadOnlyList<PreviewMarkdownInline> inlines,
        string fallbackText,
        int depth)
    {
        if (inlines.Count == 0 || depth >= MaxMarkdownInlineDepth)
            return fallbackText;
        var text = new StringBuilder();
        foreach (PreviewMarkdownInline inline in inlines)
        {
            text.Append(inline.Children.Length == 0
                ? inline.Text
                : MarkdownInlineText(inline.Children, inline.Text, depth + 1));
            if (inline.Kind == "link" && !string.IsNullOrWhiteSpace(inline.Url))
                text.Append($" ({inline.Url})");
        }
        return text.ToString();
    }

    public static string MarkdownTableText(PreviewMarkdownBlock block)
    {
        var text = new StringBuilder();
        foreach (string cell in MarkdownTableCells(block))
            AppendLine(text, cell);
        return text.ToString().TrimEnd();
    }

    public static IReadOnlyList<string> MarkdownTableCells(PreviewMarkdownBlock block)
    {
        var cells = new List<string>();
        int columns = Math.Min(
            MaxMarkdownTableColumns,
            Math.Max(block.TableHeaders.Length, block.TableRows.Select(row => row.Length).DefaultIfEmpty(0).Max()));
        if (columns == 0)
            return cells;
        foreach (string header in block.TableHeaders.Take(columns))
            cells.Add(header);
        int rows = Math.Min(120, Math.Max(0, MaxMarkdownTableCells / columns - 1));
        foreach (string[] row in block.TableRows.Take(rows))
        {
            foreach (string cell in row.Take(columns))
                cells.Add(cell);
        }
        return cells;
    }

    private static void AppendLine(StringBuilder text, string value)
    {
        text.Append(value);
        text.Append('\n');
    }

    private static void AppendSegment(
        StringBuilder text,
        List<MarkdownVisibleSegment> segments,
        string value)
    {
        if (segments.Count > 0)
            text.Append('\n');
        segments.Add(new MarkdownVisibleSegment(text.Length, value));
        text.Append(value);
    }
}
