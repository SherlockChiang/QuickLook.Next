using System.Text;
using QuickLook.Next.Contracts;

namespace QuickLook.Next.Core;

public static class TextSearchIndex
{
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
    {
        var text = new StringBuilder();
        foreach (PreviewMarkdownBlock block in document.Blocks)
            AppendMarkdownBlock(text, block);
        if (document.IsPartial)
            AppendLine(text, partialNotice);
        return text.ToString().TrimEnd();
    }

    public static string MarkdownInlineText(
        IReadOnlyList<PreviewMarkdownInline> inlines,
        string fallbackText)
    {
        if (inlines.Count == 0)
            return fallbackText;
        var text = new StringBuilder();
        foreach (PreviewMarkdownInline inline in inlines)
        {
            text.Append(inline.Children.Length == 0
                ? inline.Text
                : MarkdownInlineText(inline.Children, inline.Text));
            if (inline.Kind == "link" && !string.IsNullOrWhiteSpace(inline.Url))
                text.Append($" ({inline.Url})");
        }
        return text.ToString();
    }

    public static string MarkdownTableText(PreviewMarkdownBlock block)
    {
        var text = new StringBuilder();
        foreach (string header in block.TableHeaders)
            AppendLine(text, header);
        foreach (string[] row in block.TableRows.Take(120))
        {
            foreach (string cell in row)
                AppendLine(text, cell);
        }
        return text.ToString().TrimEnd();
    }

    private static void AppendMarkdownBlock(StringBuilder text, PreviewMarkdownBlock block)
    {
        switch (block.Kind)
        {
            case "unorderedList":
            case "orderedList":
                foreach (PreviewMarkdownBlock item in block.Children)
                    AppendLine(text, MarkdownInlineText(item.Inlines, item.Text));
                break;
            case "table":
                AppendLine(text, MarkdownTableText(block));
                break;
            case "thematicBreak":
                break;
            default:
                AppendLine(text, MarkdownInlineText(block.Inlines, block.Text));
                break;
        }
    }

    private static void AppendLine(StringBuilder text, string value)
    {
        text.Append(value);
        text.Append('\n');
    }
}
