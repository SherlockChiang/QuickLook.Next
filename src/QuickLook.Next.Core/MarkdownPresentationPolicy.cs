using System.Text;
using QuickLook.Next.Contracts;

namespace QuickLook.Next.Core;

public sealed record MarkdownPresentation(string Text, IReadOnlyList<MarkdownRenderItem> Items);
public sealed record MarkdownRenderItem(
    int Index,
    PreviewMarkdownBlock Block,
    string Prefix,
    IReadOnlyList<MarkdownVisibleSegment> Segments,
    bool IsPartial = false);

public static class MarkdownPresentationPolicy
{
    public static MarkdownPresentation Flatten(PreviewMarkdown document, string partialNotice)
    {
        var text = new StringBuilder();
        var items = new List<MarkdownRenderItem>();
        bool hasSegment = false;
        bool truncated = false;
        foreach (PreviewMarkdownBlock block in document.Blocks)
        {
            if (block.Kind == "table")
            {
                int columns = Math.Min(
                    TextSearchIndex.MaxMarkdownTableColumns,
                    Math.Max(block.TableHeaders.Length, block.TableRows.Select(row => row.Length).DefaultIfEmpty(0).Max()));
                int maxRows = columns == 0
                    ? 0
                    : Math.Max(0, TextSearchIndex.MaxMarkdownTableCells / columns - 1);
                string[][] rows = [block.TableHeaders, .. block.TableRows.Take(maxRows)];
                foreach ((string[] row, int rowIndex) in rows.Select((row, index) => (row, index)))
                {
                    if (items.Count >= TextSearchIndex.MaxMarkdownBlocks)
                    {
                        truncated = true;
                        break;
                    }
                    var rowBlock = new PreviewMarkdownBlock(rowIndex == 0 ? "tableHeader" : "tableRow")
                    {
                        TableHeaders = row.Take(TextSearchIndex.MaxMarkdownTableColumns).ToArray(),
                    };
                    items.Add(CreateItem(items.Count, rowBlock, "", text, ref hasSegment));
                }
                if (truncated)
                    break;
            }
            else if (block.Kind is "unorderedList" or "orderedList")
            {
                int number = 1;
                foreach (PreviewMarkdownBlock child in block.Children)
                {
                    if (items.Count >= TextSearchIndex.MaxMarkdownBlocks)
                    {
                        truncated = true;
                        break;
                    }
                    string prefix = block.Kind == "orderedList" ? $"{number++}. " : "- ";
                    items.Add(CreateItem(items.Count, child, prefix, text, ref hasSegment));
                }
                if (truncated)
                    break;
            }
            else
            {
                if (items.Count >= TextSearchIndex.MaxMarkdownBlocks)
                {
                    truncated = true;
                    break;
                }
                items.Add(CreateItem(items.Count, block, "", text, ref hasSegment));
            }
        }
        if (document.IsPartial || truncated)
        {
            var partial = new PreviewMarkdownBlock("partial") { Text = partialNotice };
            items.Add(CreateItem(items.Count, partial, "", text, ref hasSegment, isPartial: true));
        }
        return new MarkdownPresentation(text.ToString(), items);
    }

    private static MarkdownRenderItem CreateItem(
        int index, PreviewMarkdownBlock block, string prefix, StringBuilder text,
        ref bool hasSegment, bool isPartial = false)
    {
        var segments = new List<MarkdownVisibleSegment>();
        if (block.Kind is "table" or "tableHeader" or "tableRow")
        {
            IReadOnlyList<string> cells = block.Kind == "table"
                ? TextSearchIndex.MarkdownTableCells(block)
                : block.TableHeaders;
            foreach (string cell in cells)
                AppendSegment(text, segments, cell, ref hasSegment);
        }
        else if (block.Kind != "thematicBreak")
        {
            AppendSegment(text, segments, TextSearchIndex.MarkdownInlineText(block.Inlines, block.Text), ref hasSegment);
        }
        return new MarkdownRenderItem(index, block, prefix, segments, isPartial);
    }

    private static void AppendSegment(
        StringBuilder text, List<MarkdownVisibleSegment> segments, string value, ref bool hasSegment)
    {
        if (hasSegment)
            text.Append('\n');
        segments.Add(new MarkdownVisibleSegment(text.Length, value));
        text.Append(value);
        hasSegment = true;
    }
}
