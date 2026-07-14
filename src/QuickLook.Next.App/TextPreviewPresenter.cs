using System.Collections.ObjectModel;
using System.Text;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Windows.ApplicationModel.DataTransfer;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal readonly record struct TextSearchState(int Current, int Count);

internal sealed class TextPreviewPresenter
{
    private const int MaxHighlightedChars = 256 * 1024;
    private const int MaxHighlightedRuns = 7000;
    private const int MaxSearchHighlightRanges = 5000;
    private const int MaxMarkdownBlocks = 2000;
    private const double OutlineWidth = 188;
    private const double OutlineGap = 10;

    private static readonly SolidColorBrush UiGrayBrush = new(Colors.Gray);
    private static readonly Dictionary<string, FontFamily> FontFamilyCache = new(StringComparer.Ordinal);

    private readonly RichTextBlock _textBlock;
    private readonly ScrollViewer _scrollViewer;
    private readonly Border _outlinePanel;
    private readonly ListView _outlineList;
    private readonly ListView _textListView;
    private readonly FrameworkElement _textPreviewContainer;
    private readonly Func<ElementTheme> _getTheme;
    private readonly Func<(bool Enabled, Windows.UI.Color Background, Windows.UI.Color Foreground)> _getHighContrast;
    private readonly ObservableCollection<MarkdownOutlineItem> _outlineItems = [];
    private readonly Dictionary<TokenKind, SolidColorBrush> _tokenBrushes = [];
    private readonly Thickness _defaultScrollMargin;
    private bool? _brushThemeDark;
    private bool _updatingOutline;
    private int _renderVersion;
    private PreviewReady? _lastReady;
    private (double Width, double Height) _lastMaxContent;
    private string _displayedText = "";
    private string _searchQuery = "";
    private readonly List<int> _searchMatches = [];
    private readonly List<(int Start, FrameworkElement Anchor)> _markdownSearchAnchors = [];
    private int _currentSearchMatch = -1;
    private int _markdownSearchOffset;
    private int _markdownBlocksRendered;
    private bool _markdownRenderTruncated;

    public TextPreviewPresenter(
        RichTextBlock textBlock,
        ScrollViewer scrollViewer,
        ListView textListView,
        FrameworkElement textPreviewContainer,
        Border outlinePanel,
        ListView outlineList,
        Func<ElementTheme> getTheme,
        Func<(bool Enabled, Windows.UI.Color Background, Windows.UI.Color Foreground)> getHighContrast)
    {
        _textBlock = textBlock;
        _scrollViewer = scrollViewer;
        _textListView = textListView;
        _textPreviewContainer = textPreviewContainer;
        _outlinePanel = outlinePanel;
        _outlineList = outlineList;
        _getTheme = getTheme;
        _getHighContrast = getHighContrast;
        _defaultScrollMargin = textPreviewContainer.Margin;
        _outlineList.ItemsSource = _outlineItems;
        _outlineList.ItemClick += OnOutlineItemClick;
        _scrollViewer.ViewChanged += OnScrollViewerViewChanged;
        _textListView.KeyDown += OnTextListViewKeyDown;
    }

    public TextPreviewResult Render(PreviewReady ready, (double Width, double Height) maxContent)
    {
        _lastReady = ready;
        _lastMaxContent = maxContent;
        string text = TrimForDisplay(ready.TextContent ?? "");
        _displayedText = ready.Markdown is null
            ? text
            : TextSearchIndex.BuildMarkdownVisibleText(ready.Markdown, UiStrings.TextPreviewTruncated);
        _markdownSearchAnchors.Clear();
        _markdownSearchOffset = 0;
        _markdownBlocksRendered = 0;
        _markdownRenderTruncated = false;
        ClearSearch();
        int renderVersion = ++_renderVersion;
        DiagLog.Write("App", $"text preview: format={ready.TextFormat}; language={ready.TextLanguage}; chars={ready.TextContent?.Length ?? 0}; displayed={text.Length}");

        _textBlock.Blocks.Clear();
        _textBlock.IsTextSelectionEnabled = true;
        ClearOutline();

        bool isMarkdown = ready.TextFormat == "markdown" || ready.Markdown is not null;
        bool wrap = ready.TextFormat is "markdown" or "plain";
        _scrollViewer.Visibility = Visibility.Visible;
        _textListView.Visibility = Visibility.Collapsed;
        
        _scrollViewer.HorizontalScrollBarVisibility = wrap ? ScrollBarVisibility.Disabled : ScrollBarVisibility.Auto;
        _textBlock.FontFamily = FontFamilyFor(ready.TextFormat == "markdown" ? "Segoe UI" : "Cascadia Mono, Consolas");
        _textBlock.TextWrapping = wrap ? TextWrapping.Wrap : TextWrapping.NoWrap;

        try
        {
            if (ready.Markdown is not null)
                RenderMarkdownDocument(ready.Markdown);
            else if (ready.TextFormat == "markdown")
                RenderMarkdown(text);
            else
                _ = RenderCodeOrPlainTextAsync(text, ready.TextLanguage ?? "text", renderVersion);
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "text render FAILED; falling back to plain text: " + ex);
            _scrollViewer.Visibility = Visibility.Visible;
            _textListView.Visibility = Visibility.Collapsed;
            _ = RenderCodeOrPlainTextAsync(text, "text", renderVersion);
        }

        ApplyOutlineVisibility();
        _textBlock.Focus(FocusState.Programmatic);
        var size = EstimateTextPreviewSize(text, ready.TextFormat, wrap, maxContent);
        if (_outlineItems.Count > 0)
            size = (Math.Min(maxContent.Width, size.Width + OutlineWidth + OutlineGap), size.Height);
        return new TextPreviewResult($"{ready.Kind}: {ready.Title}", size.Width, size.Height);
    }

    public void Clear()
    {
        _lastReady = null;
        _displayedText = "";
        _markdownSearchAnchors.Clear();
        _markdownSearchOffset = 0;
        ClearSearch();
        _renderVersion++;
        _textBlock.Blocks.Clear();
        _textListView.ItemsSource = null;
        ClearOutline();
        ApplyOutlineVisibility();
    }

    public TextSearchState SetSearchQuery(string query)
    {
        _searchQuery = query.Trim();
        _searchMatches.Clear();
        _currentSearchMatch = -1;
        _searchMatches.AddRange(TextSearchIndex.FindMatches(_displayedText, _searchQuery));
        if (_searchMatches.Count > 0)
            _currentSearchMatch = 0;
        ApplySearchHighlights();
        ScrollToCurrentSearchMatch();
        return SearchState;
    }

    public TextSearchState MoveSearch(int delta)
    {
        if (_searchMatches.Count == 0)
            return SearchState;
        _currentSearchMatch = (_currentSearchMatch + delta + _searchMatches.Count) % _searchMatches.Count;
        ApplySearchHighlights();
        ScrollToCurrentSearchMatch();
        return SearchState;
    }

    public TextSearchState ClearSearch()
    {
        _searchQuery = "";
        _searchMatches.Clear();
        _currentSearchMatch = -1;
        _textBlock.TextHighlighters.Clear();
        return SearchState;
    }

    private TextSearchState SearchState
        => new(_currentSearchMatch >= 0 ? _currentSearchMatch + 1 : 0, _searchMatches.Count);

    private void ApplySearchHighlights()
    {
        _textBlock.TextHighlighters.Clear();
        if (_searchMatches.Count == 0
            || _lastReady?.Markdown is not null
            || _lastReady?.TextFormat == "markdown")
        {
            return;
        }

        var allMatches = new TextHighlighter
        {
            Background = new SolidColorBrush(Windows.UI.Color.FromArgb(110, 255, 210, 64)),
        };
        foreach (int start in _searchMatches.Take(MaxSearchHighlightRanges))
            allMatches.Ranges.Add(new TextRange { StartIndex = start, Length = _searchQuery.Length });
        _textBlock.TextHighlighters.Add(allMatches);

        if (_currentSearchMatch >= 0)
        {
            var current = new TextHighlighter
            {
                Background = new SolidColorBrush(Windows.UI.Color.FromArgb(220, 255, 145, 48)),
            };
            current.Ranges.Add(new TextRange
            {
                StartIndex = _searchMatches[_currentSearchMatch],
                Length = _searchQuery.Length,
            });
            _textBlock.TextHighlighters.Add(current);
        }
    }

    private void ScrollToCurrentSearchMatch()
    {
        if (_currentSearchMatch < 0 || _displayedText.Length == 0 || _scrollViewer.ScrollableHeight <= 0)
            return;
        if (_markdownSearchAnchors.Count > 0)
        {
            int match = _searchMatches[_currentSearchMatch];
            FrameworkElement anchor = _markdownSearchAnchors
                .LastOrDefault(candidate => candidate.Start <= match)
                .Anchor ?? _markdownSearchAnchors[0].Anchor;
            _scrollViewer.UpdateLayout();
            double target = Math.Max(
                0,
                _scrollViewer.VerticalOffset
                    + anchor.TransformToVisual(_scrollViewer).TransformPoint(new Windows.Foundation.Point(0, 0)).Y
                    - 8);
            _scrollViewer.ChangeView(null, target, null, disableAnimation: false);
            return;
        }
        double progress = (double)_searchMatches[_currentSearchMatch] / _displayedText.Length;
        _scrollViewer.ChangeView(null, _scrollViewer.ScrollableHeight * progress, null, disableAnimation: true);
    }

    public void RefreshPalette()
    {
        _tokenBrushes.Clear();
        if (_lastReady is not null)
            Render(_lastReady, _lastMaxContent);
    }

    private void RenderMarkdownDocument(PreviewMarkdown document)
    {
        foreach (PreviewMarkdownBlock block in document.Blocks)
        {
            if (block.Kind is "unorderedList" or "orderedList")
                AddMarkdownBlock(block);
            else if (TryReserveMarkdownBlock())
                AddMarkdownBlock(block);
            else
                break;
        }

        if (document.IsPartial || _markdownRenderTruncated)
        {
            var partial = CreateParagraph(12, "Segoe UI", 12, 0);
            partial.Foreground = UiGrayBrush;
            partial.Inlines.Add(new Run { Text = UiStrings.TextPreviewTruncated });
            _textBlock.Blocks.Add(partial);
        }
    }


    private void AddMarkdownBlock(PreviewMarkdownBlock block)
    {
        switch (block.Kind)
        {
            case "heading":
                AddMarkdownHeading(block);
                break;
            case "blockquote":
                AddMarkdownQuote(block);
                break;
            case "unorderedList":
                AddMarkdownList(block, ordered: false);
                break;
            case "orderedList":
                AddMarkdownList(block, ordered: true);
                break;
            case "code":
                AddMarkdownCodeBlock(block.Text, string.IsNullOrWhiteSpace(block.Language) ? "text" : block.Language);
                break;
            case "thematicBreak":
                AddMarkdownRule();
                break;
            case "table":
                AddMarkdownTable(block);
                break;
            default:
                AddMarkdownParagraph(block);
                break;
        }
    }

    private void AddMarkdownHeading(PreviewMarkdownBlock block)
    {
        double size = block.Level switch
        {
            <= 1 => 28,
            2 => 23,
            3 => 19,
            _ => 16,
        };
        var p = CreateParagraph(size, "Segoe UI", block.Level <= 2 ? 16 : 10, 8);
        FrameworkElement anchor = CreateHeadingAnchor();
        p.Inlines.Add(new InlineUIContainer { Child = anchor });
        RegisterMarkdownSearchAnchor(anchor, TextSearchIndex.MarkdownInlineText(block.Inlines, block.Text));
        var bold = new Bold();
        AddMarkdownInlines(bold.Inlines, block.Inlines, block.Text);
        p.Inlines.Add(bold);
        _textBlock.Blocks.Add(p);
        AddOutlineItem(block, anchor);
    }

    private void AddMarkdownParagraph(PreviewMarkdownBlock block)
    {
        var p = CreateParagraph(14, "Segoe UI", 0, 9);
        FrameworkElement anchor = CreateHeadingAnchor();
        p.Inlines.Add(new InlineUIContainer { Child = anchor });
        RegisterMarkdownSearchAnchor(anchor, TextSearchIndex.MarkdownInlineText(block.Inlines, block.Text));
        AddMarkdownInlines(p.Inlines, block.Inlines, block.Text);
        _textBlock.Blocks.Add(p);
    }

    private void AddMarkdownQuote(PreviewMarkdownBlock block)
    {
        var p = CreateParagraph(14, "Segoe UI", 4, 10);
        p.Foreground = UiGrayBrush;
        FrameworkElement anchor = CreateHeadingAnchor();
        p.Inlines.Add(new InlineUIContainer { Child = anchor });
        RegisterMarkdownSearchAnchor(anchor, TextSearchIndex.MarkdownInlineText(block.Inlines, block.Text));
        p.Inlines.Add(new Run { Text = "| " });
        AddMarkdownInlines(p.Inlines, block.Inlines, block.Text);
        _textBlock.Blocks.Add(p);
    }

    private void AddMarkdownList(PreviewMarkdownBlock block, bool ordered)
    {
        int index = 1;
        foreach (PreviewMarkdownBlock item in block.Children)
        {
            if (!TryReserveMarkdownBlock())
                break;
            var p = CreateParagraph(14, "Segoe UI", 0, 5);
            p.Margin = new Thickness(18, 0, 0, 5);
            FrameworkElement anchor = CreateHeadingAnchor();
            p.Inlines.Add(new InlineUIContainer { Child = anchor });
            RegisterMarkdownSearchAnchor(anchor, TextSearchIndex.MarkdownInlineText(item.Inlines, item.Text));
            p.Inlines.Add(new Run { Text = ordered ? $"{index}. " : "- " });
            AddMarkdownInlines(p.Inlines, item.Inlines, item.Text);
            _textBlock.Blocks.Add(p);
            index++;
        }
    }

    private bool TryReserveMarkdownBlock()
    {
        if (_markdownBlocksRendered < MaxMarkdownBlocks)
        {
            _markdownBlocksRendered++;
            return true;
        }
        _markdownRenderTruncated = true;
        return false;
    }

    private void AddMarkdownRule()
    {
        var p = CreateParagraph(12, "Segoe UI", 8, 10);
        p.Foreground = UiGrayBrush;
        p.Inlines.Add(new Run { Text = "----------------------------------------" });
        _textBlock.Blocks.Add(p);
    }

    private void AddMarkdownTable(PreviewMarkdownBlock block)
    {
        int columns = Math.Max(
            block.TableHeaders.Length,
            block.TableRows.Select(row => row.Length).DefaultIfEmpty(0).Max());
        columns = Math.Min(columns, TextSearchIndex.MaxMarkdownTableColumns);
        if (columns <= 0) return;

        var container = new Border
        {
            Margin = new Thickness(0, 12, 0, 12),
            CornerRadius = new CornerRadius(6),
            BorderThickness = new Thickness(1),
            BorderBrush = new SolidColorBrush(ThemeSurfaceBorderColor()),
            Background = new SolidColorBrush(ThemeCodeBackground()),
            HorizontalAlignment = HorizontalAlignment.Left
        };

        var grid = new Grid();
        for (int i = 0; i < columns; i++)
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        int currentRow = 0;
        int maxRows = Math.Min(
            block.TableRows.Count(),
            Math.Min(120, Math.Max(0, TextSearchIndex.MaxMarkdownTableCells / columns - 1)));

        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        for (int c = 0; c < columns; c++)
        {
            var cellBorder = new Border
            {
                Padding = new Thickness(12, 8, 12, 8),
                BorderBrush = new SolidColorBrush(ThemeSurfaceBorderColor()),
                BorderThickness = new Thickness(0, 0, c < columns - 1 ? 1 : 0, maxRows > 0 ? 1 : 0),
                Background = new SolidColorBrush(ThemeHeaderBackground())
            };
            var textBlock = new TextBlock
            {
                Text = block.TableHeaders.ElementAtOrDefault(c) ?? "",
                FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
                TextWrapping = TextWrapping.Wrap,
                MaxWidth = 400,
                IsTextSelectionEnabled = true
            };
            cellBorder.Child = textBlock;
            Grid.SetRow(cellBorder, currentRow);
            Grid.SetColumn(cellBorder, c);
            grid.Children.Add(cellBorder);
        }
        currentRow++;

        foreach (var row in block.TableRows.Take(maxRows))
        {
            grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            for (int c = 0; c < columns; c++)
            {
                var cellBorder = new Border
                {
                    Padding = new Thickness(12, 8, 12, 8),
                    BorderBrush = new SolidColorBrush(ThemeSurfaceBorderColor()),
                    BorderThickness = new Thickness(0, 0, c < columns - 1 ? 1 : 0, currentRow < maxRows ? 1 : 0)
                };
                var textBlock = new TextBlock
                {
                    Text = row.ElementAtOrDefault(c) ?? "",
                    TextWrapping = TextWrapping.Wrap,
                    MaxWidth = 400,
                    IsTextSelectionEnabled = true
                };
                cellBorder.Child = textBlock;
                Grid.SetRow(cellBorder, currentRow);
                Grid.SetColumn(cellBorder, c);
                grid.Children.Add(cellBorder);
            }
            currentRow++;
        }

        var scroller = new ScrollViewer
        {
            HorizontalScrollBarVisibility = ScrollBarVisibility.Auto,
            VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
            MaxWidth = 980
        };
        scroller.Content = grid;
        container.Child = scroller;
        RegisterMarkdownSearchAnchor(container, TextSearchIndex.MarkdownTableText(block));

        var p = new Paragraph();
        p.Inlines.Add(new InlineUIContainer { Child = container });
        _textBlock.Blocks.Add(p);
    }

    private void RegisterMarkdownSearchAnchor(FrameworkElement anchor, string text)
    {
        _markdownSearchAnchors.Add((_markdownSearchOffset, anchor));
        _markdownSearchOffset += text.Length + 1;
    }


    private void AddOutlineItem(PreviewMarkdownBlock block, FrameworkElement anchor)
    {
        string title = string.IsNullOrWhiteSpace(block.Text)
            ? string.Concat(block.Inlines.Select(i => i.Text)).Trim()
            : block.Text.Trim();
        if (title.Length == 0)
            return;

        int level = Math.Clamp(block.Level <= 0 ? 1 : block.Level, 1, 6);
        _outlineItems.Add(new MarkdownOutlineItem(title, level, anchor));
    }

    private void AddOutlineItem(string title, int level, FrameworkElement anchor)
    {
        title = title.Trim();
        if (title.Length == 0)
            return;
        _outlineItems.Add(new MarkdownOutlineItem(title, Math.Clamp(level, 1, 6), anchor));
    }

    private void ClearOutline()
    {
        _updatingOutline = true;
        _outlineList.SelectedItem = null;
        _outlineItems.Clear();
        _updatingOutline = false;
    }

    private void ApplyOutlineVisibility()
    {
        bool visible = _outlineItems.Count > 0;
        _outlinePanel.Visibility = visible ? Visibility.Visible : Visibility.Collapsed;
        _textPreviewContainer.Margin = visible
            ? new Thickness(_defaultScrollMargin.Left + OutlineWidth + OutlineGap, _defaultScrollMargin.Top, _defaultScrollMargin.Right, _defaultScrollMargin.Bottom)
            : _defaultScrollMargin;
    }

    private async void OnOutlineItemClick(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is not MarkdownOutlineItem item)
            return;

        _scrollViewer.UpdateLayout();
        _textBlock.UpdateLayout();
        
        // Disable outline sync temporarily while we animate to the target
        int version = _renderVersion;
        _updatingOutline = true;
        
        double target = Math.Max(0, _scrollViewer.VerticalOffset + item.Anchor.TransformToVisual(_scrollViewer).TransformPoint(new Windows.Foundation.Point(0, 0)).Y - 8);
        _scrollViewer.ChangeView(null, target, null, disableAnimation: false);

        try
        {
            await Task.Delay(300);
        }
        finally
        {
            if (version == _renderVersion)
                _updatingOutline = false;
        }
    }

    private void OnScrollViewerViewChanged(object? sender, ScrollViewerViewChangedEventArgs e)
    {
        if (_updatingOutline || _outlineItems.Count == 0 || e.IsIntermediate)
            return;

        try
        {
            MarkdownOutlineItem? closest = null;
            double closestY = double.MinValue;
            foreach (var item in _outlineItems)
            {
                var point = item.Anchor.TransformToVisual(_scrollViewer).TransformPoint(new Windows.Foundation.Point(0, 0));
                if (point.Y <= 24 && point.Y > closestY)
                {
                    closest = item;
                    closestY = point.Y;
                }
            }

            if (closest == null && _outlineItems.Count > 0)
            {
                var first = _outlineItems[0];
                if (first.Anchor.TransformToVisual(_scrollViewer).TransformPoint(new Windows.Foundation.Point(0, 0)).Y > 0)
                    closest = first;
            }

            if (closest != null && _outlineList.SelectedItem != closest)
            {
                _updatingOutline = true;
                _outlineList.SelectedItem = closest;
                _outlineList.ScrollIntoView(closest);
                _updatingOutline = false;
            }
        }
        catch
        {
        }
    }

    private void AddMarkdownInlines(InlineCollection target, IReadOnlyList<PreviewMarkdownInline> inlines, string fallbackText, int depth = 0)
    {
        if (inlines.Count == 0 || depth >= TextSearchIndex.MaxMarkdownInlineDepth)
        {
            target.Add(new Run { Text = fallbackText });
            return;
        }

        foreach (PreviewMarkdownInline inline in inlines)
            AddMarkdownInline(target, inline, depth);
    }

    private void AddMarkdownInline(InlineCollection target, PreviewMarkdownInline inline, int depth)
    {
        switch (inline.Kind)
        {
            case "code":
                target.Add(new Run
                {
                    Text = inline.Text,
                    FontFamily = FontFamilyFor("Cascadia Mono, Consolas"),
                    Foreground = BrushFor(TokenKind.Keyword),
                });
                break;
            case "strong":
                var bold = new Bold();
                AddMarkdownInlines(bold.Inlines, inline.Children, inline.Text, depth + 1);
                target.Add(bold);
                break;
            case "emphasis":
                var italic = new Italic();
                AddMarkdownInlines(italic.Inlines, inline.Children, inline.Text, depth + 1);
                target.Add(italic);
                break;
            case "link":
                var link = new Span { Foreground = BrushFor(TokenKind.Keyword) };
                AddMarkdownInlines(link.Inlines, inline.Children, inline.Text, depth + 1);
                if (!string.IsNullOrWhiteSpace(inline.Url))
                    link.Inlines.Add(new Run { Text = $" ({inline.Url})" });
                target.Add(link);
                break;
            default:
                target.Add(new Run { Text = inline.Text });
                break;
        }
    }

    private static (double Width, double Height) EstimateTextPreviewSize(
        string text,
        string? format,
        bool wrap,
        (double Width, double Height) maxContent)
    {
        string[] lines = text.Length == 0 ? [""] : NormalizeLines(text);
        int lineCount = Math.Max(1, lines.Length);
        int maxLineLength = lines
            .Take(500)
            .Select(line => line.Length)
            .DefaultIfEmpty(0)
            .Max();

        bool markdown = string.Equals(format, "markdown", StringComparison.OrdinalIgnoreCase);
        bool code = !wrap && !markdown;
        double charWidth = code ? 8.2 : 7.2;
        double lineHeight = code ? 20 : 22;

        double width;
        if (wrap)
        {
            double contentWeight = Math.Sqrt(Math.Min(text.Length, MaxHighlightedChars));
            width = 500 + Math.Min(360, contentWeight * 8);
            if (markdown)
                width = Math.Max(width, 620);
        }
        else
        {
            width = Math.Min(980, Math.Max(520, Math.Min(maxLineLength, 120) * charWidth + 96));
        }

        int desiredLines = lineCount <= 12 ? lineCount : Math.Min(lineCount, code ? 30 : 26);
        double height = desiredLines * lineHeight + (markdown ? 120 : 96);

        return (
            Math.Clamp(width, 460, maxContent.Width),
            Math.Clamp(height, 260, maxContent.Height));
    }

    private void RenderMarkdown(string text)
    {
        string[] lines = NormalizeLines(text);
        bool inCode = false;
        var code = new StringBuilder();
        string codeLanguage = "text";
        var paragraphBuffer = new List<string>();

        void FlushParagraph()
        {
            if (paragraphBuffer.Count == 0) return;
            var p = CreateParagraph(14, "Segoe UI", 0, 8);
            AddInlineMarkdown(p, string.Join(" ", paragraphBuffer));
            _textBlock.Blocks.Add(p);
            paragraphBuffer.Clear();
        }

        foreach (string raw in lines)
        {
            string line = raw.TrimEnd('\r');
            if (line.TrimStart().StartsWith("```", StringComparison.Ordinal))
            {
                if (inCode)
                {
                    AddMarkdownCodeBlock(code.ToString().TrimEnd('\n'), codeLanguage);
                    code.Clear();
                    codeLanguage = "text";
                    inCode = false;
                }
                else
                {
                    FlushParagraph();
                    inCode = true;
                    codeLanguage = SyntaxHighlighter.NormalizeLanguage(line.TrimStart()[3..].Trim());
                    if (codeLanguage.Length == 0)
                        codeLanguage = "text";
                }
                continue;
            }

            if (inCode)
            {
                code.AppendLine(raw);
                continue;
            }

            string trimmed = line.Trim();
            if (trimmed.Length == 0)
            {
                FlushParagraph();
                continue;
            }

            if (trimmed.StartsWith("#", StringComparison.Ordinal))
            {
                FlushParagraph();
                int level = Math.Min(6, trimmed.TakeWhile(c => c == '#').Count());
                string title = trimmed[level..].Trim();
                var p = CreateParagraph(level <= 1 ? 26 : level == 2 ? 22 : 18, "Segoe UI", 14, 8);
                FrameworkElement anchor = CreateHeadingAnchor();
                p.Inlines.Add(new InlineUIContainer { Child = anchor });
                p.Inlines.Add(new Bold { Inlines = { new Run { Text = title } } });
                _textBlock.Blocks.Add(p);
                AddOutlineItem(title, level, anchor);
                continue;
            }

            if (trimmed.StartsWith("> ", StringComparison.Ordinal))
            {
                FlushParagraph();
                var p = CreateParagraph(14, "Segoe UI", 4, 8);
                p.Foreground = UiGrayBrush;
                p.Inlines.Add(new Run { Text = "│ " });
                AddInlineMarkdown(p, trimmed[2..]);
                _textBlock.Blocks.Add(p);
                continue;
            }

            if (trimmed.StartsWith("- ", StringComparison.Ordinal) || trimmed.StartsWith("* ", StringComparison.Ordinal))
            {
                FlushParagraph();
                var p = CreateParagraph(14, "Segoe UI", 2, 4);
                p.Inlines.Add(new Run { Text = "• " });
                AddInlineMarkdown(p, trimmed[2..]);
                _textBlock.Blocks.Add(p);
                continue;
            }

            paragraphBuffer.Add(trimmed);
        }

        FlushParagraph();
        if (inCode && code.Length > 0)
        {
            // For simplicity in this demo, markdown parsing is synchronous but code highlighting can be async
            // To preserve order properly we should await it, but RenderMarkdown is synchronous right now.
            // A quick fix is to fire-and-forget or keep markdown code highlighting synchronous if it's small,
            // but for full files, we use RenderCodeOrPlainTextAsync.
            AddMarkdownCodeBlock(code.ToString().TrimEnd('\n'), codeLanguage);
        }
    }

    private void AddMarkdownCodeBlock(string code, string language)
        => AddHighlightedCode(code, language);

    private async Task RenderCodeOrPlainTextAsync(string text, string language, int renderVersion)
    {
        _textListView.ItemsSource = null;
        
        if (text.Length == 0) return;

        string code = text.TrimEnd('\r', '\n');
        bool noHighlight = language is "text" or "log" || code.Length > MaxHighlightedChars;
        var paragraph = CreateParagraph(13, "Cascadia Mono, Consolas", 0, 0);
        
        if (noHighlight)
        {
            paragraph.Foreground = BrushFor(TokenKind.Default);
            if (code.Length > MaxHighlightedChars)
            {
                paragraph.Inlines.Add(new Run
                {
                    Text = code[..MaxHighlightedChars]
                        + $"\n\n[Syntax highlighting disabled after {MaxHighlightedChars:N0} characters]",
                });
            }
            else
            {
                paragraph.Inlines.Add(new Run { Text = code });
            }
        }
        else
        {
            var spans = await Task.Run(() => MergeAdjacentSpans(SyntaxHighlighter.Highlight(code, language)));
            if (renderVersion != _renderVersion)
                return;

            int runs = 0;
            foreach (var (txt, kind) in spans)
            {
                if (txt.Length == 0) continue;
                if (++runs > MaxHighlightedRuns)
                {
                    DiagLog.Write("App", $"highlight run limit hit: language={language}; chars={code.Length}; runs>{MaxHighlightedRuns}");
                    paragraph.Inlines.Clear();
                    paragraph.Foreground = BrushFor(TokenKind.Default);
                    paragraph.Inlines.Add(new Run { Text = code + $"\n\n[Syntax highlighting disabled after {MaxHighlightedRuns:N0} spans]" });
                    break;
                }
                paragraph.Inlines.Add(new Run { Text = txt, Foreground = BrushFor(kind) });
            }
        }

        if (renderVersion != _renderVersion)
            return;
        _textBlock.Blocks.Add(paragraph);
    }

    private void AddHighlightedCode(string code, string language)
    {
        var container = new Border
        {
            Margin = new Thickness(0, 12, 0, 12),
            CornerRadius = new CornerRadius(8),
            BorderThickness = new Thickness(1),
            BorderBrush = new SolidColorBrush(ThemeSurfaceBorderColor()),
            Background = new SolidColorBrush(ThemeCodeBackground()),
            MaxWidth = 980,
            HorizontalAlignment = HorizontalAlignment.Left
        };

        var grid = new Grid();
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

        var header = new Grid
        {
            Padding = new Thickness(12, 6, 8, 6),
            Background = new SolidColorBrush(ThemeHeaderBackground()),
            CornerRadius = new CornerRadius(8, 8, 0, 0)
        };
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        var langText = new TextBlock
        {
            Text = language.ToUpperInvariant(),
            FontSize = 11,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
            Foreground = new SolidColorBrush(ThemeTextColorSecondary()),
            VerticalAlignment = VerticalAlignment.Center
        };
        Grid.SetColumn(langText, 0);
        header.Children.Add(langText);

        var copyBtn = new Button
        {
            Content = new StackPanel
            {
                Orientation = Orientation.Horizontal,
                Spacing = 6,
                Children =
                {
                    new FontIcon { Glyph = "\uE8C8", FontSize = 12 },
                    new TextBlock { Text = UiStrings.CopyAction, FontSize = 11 }
                }
            },
            Padding = new Thickness(8, 4, 8, 4),
            Background = new SolidColorBrush(Colors.Transparent),
            BorderThickness = new Thickness(0),
            VerticalAlignment = VerticalAlignment.Center,
            CornerRadius = new CornerRadius(4)
        };
        
        copyBtn.Click += async (s, e) =>
        {
            var package = new Windows.ApplicationModel.DataTransfer.DataPackage();
            package.SetText(code);
            Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(package);
            
            var sp = (StackPanel)copyBtn.Content;
            ((FontIcon)sp.Children[0]).Glyph = "\uE73E"; // Checkmark
            ((TextBlock)sp.Children[1]).Text = UiStrings.CopiedAction;
            int version = _renderVersion;
            await Task.Delay(2000);
            if (version != _renderVersion)
                return;
            ((FontIcon)sp.Children[0]).Glyph = "\uE8C8";
            ((TextBlock)sp.Children[1]).Text = UiStrings.CopyAction;
        };

        Grid.SetColumn(copyBtn, 2);
        header.Children.Add(copyBtn);
        
        Grid.SetRow(header, 0);
        grid.Children.Add(header);

        var bodyScroller = new ScrollViewer
        {
            HorizontalScrollBarVisibility = ScrollBarVisibility.Auto,
            VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
            Padding = new Thickness(16)
        };
        
        var bodyText = new TextBlock
        {
            FontFamily = FontFamilyFor("Cascadia Mono, Consolas"),
            FontSize = 13,
            IsTextSelectionEnabled = true,
            TextWrapping = TextWrapping.NoWrap
        };
        
        if (code.Length == 0)
        {
            bodyText.Inlines.Add(new Run { Text = " " });
        }
        else if (code.Length > MaxHighlightedChars || language is "text" or "log")
        {
            bodyText.Foreground = BrushFor(TokenKind.Default);
            if (code.Length > MaxHighlightedChars)
            {
                bodyText.Inlines.Add(new Run
                {
                    Text = code[..MaxHighlightedChars]
                        + $"\n\n[Syntax highlighting disabled after {MaxHighlightedChars:N0} characters]",
                });
            }
            else
            {
                bodyText.Inlines.Add(new Run { Text = code });
            }
        }
        else
        {
            var spans = MergeAdjacentSpans(SyntaxHighlighter.Highlight(code, language));
            int runs = 0;
            foreach (var (txt, kind) in spans)
            {
                if (txt.Length == 0) continue;
                if (++runs > MaxHighlightedRuns)
                {
                    DiagLog.Write("App", $"highlight run limit hit: language={language}; chars={code.Length}; runs>{MaxHighlightedRuns}");
                    bodyText.Inlines.Clear();
                    bodyText.Foreground = BrushFor(TokenKind.Default);
                    bodyText.Inlines.Add(new Run { Text = code + $"\n\n[Syntax highlighting disabled after {MaxHighlightedRuns:N0} spans]" });
                    break;
                }
                bodyText.Inlines.Add(new Run { Text = txt, Foreground = BrushFor(kind) });
            }
        }
        
        bodyScroller.Content = bodyText;
        Grid.SetRow(bodyScroller, 1);
        grid.Children.Add(bodyScroller);
        
        container.Child = grid;

        if (_lastReady?.Markdown is not null)
            RegisterMarkdownSearchAnchor(container, code);

        var p = new Paragraph();
        p.Inlines.Add(new InlineUIContainer { Child = container });
        _textBlock.Blocks.Add(p);
    }

    private static List<(string Text, TokenKind Kind)> MergeAdjacentSpans(
        IEnumerable<(string Text, TokenKind Kind)> spans)
    {
        var merged = new List<(string Text, TokenKind Kind)>();
        var text = new StringBuilder();
        TokenKind currentKind = TokenKind.Default;
        bool hasCurrent = false;
        foreach ((string value, TokenKind kind) in spans)
        {
            if (value.Length == 0)
                continue;
            if (hasCurrent && kind != currentKind)
            {
                merged.Add((text.ToString(), currentKind));
                text.Clear();
            }
            currentKind = kind;
            hasCurrent = true;
            text.Append(value);
        }
        if (hasCurrent)
            merged.Add((text.ToString(), currentKind));
        return merged;
    }

    private void OnTextListViewKeyDown(object sender, KeyRoutedEventArgs e)
    {
        bool controlDown = (Microsoft.UI.Input.InputKeyboardSource
            .GetKeyStateForCurrentThread(Windows.System.VirtualKey.Control)
            & Windows.UI.Core.CoreVirtualKeyStates.Down) == Windows.UI.Core.CoreVirtualKeyStates.Down;
        if (!controlDown || e.Key != Windows.System.VirtualKey.C)
            return;

        var selected = _textListView.SelectedItems
            .OfType<TextLineItem>()
            .OrderBy(item => item.LineNumber)
            .Select(item => item.Text)
            .ToArray();
        if (selected.Length == 0)
            return;

        var package = new DataPackage();
        package.SetText(string.Join(Environment.NewLine, selected));
        Clipboard.SetContent(package);
        e.Handled = true;
    }

    private static string TextFrom(TextBlock textBlock)
    {
        if (textBlock.Text.Length > 0)
            return textBlock.Text;
        return string.Concat(textBlock.Inlines.OfType<Run>().Select(run => run.Text));
    }

    private SolidColorBrush BrushFor(TokenKind kind)
    {
        var highContrast = _getHighContrast();
        if (highContrast.Enabled)
            return new SolidColorBrush(highContrast.Foreground);
        bool dark = _getTheme() != ElementTheme.Light;
        if (_brushThemeDark != dark) { _tokenBrushes.Clear(); _brushThemeDark = dark; }
        if (_tokenBrushes.TryGetValue(kind, out var brush))
            return brush;

        Windows.UI.Color c = kind switch
        {
            TokenKind.Keyword => dark ? Rgb(0x56, 0x9C, 0xD6) : Rgb(0x00, 0x00, 0xFF),
            TokenKind.Str => dark ? Rgb(0xCE, 0x91, 0x78) : Rgb(0xA3, 0x15, 0x15),
            TokenKind.Comment => dark ? Rgb(0x6A, 0x99, 0x55) : Rgb(0x00, 0x80, 0x00),
            TokenKind.Number => dark ? Rgb(0xB5, 0xCE, 0xA8) : Rgb(0x09, 0x86, 0x58),
            TokenKind.Type => dark ? Rgb(0x4E, 0xC9, 0xB0) : Rgb(0x2B, 0x91, 0xAF),
            TokenKind.Property => dark ? Rgb(0x9C, 0xDC, 0xFE) : Rgb(0x00, 0x16, 0x80),
            TokenKind.Punctuation => dark ? Rgb(0xD4, 0xD4, 0xD4) : Rgb(0x39, 0x39, 0x39),
            _ => ThemeTextColor(),
        };
        brush = new SolidColorBrush(c);
        _tokenBrushes[kind] = brush;
        return brush;
    }

    private static string TrimForDisplay(string text)
        => text.Length <= MaxHighlightedChars
            ? text
            : text[..MaxHighlightedChars] + $"\n\n[Preview truncated at {MaxHighlightedChars:N0} characters]";

    private static Paragraph CreateParagraph(double fontSize, string fontFamily, double top, double bottom)
        => new()
        {
            FontSize = fontSize,
            FontFamily = FontFamilyFor(fontFamily),
            Margin = new Thickness(0, top, 0, bottom),
        };

    private static FrameworkElement CreateHeadingAnchor()
        => new Border
        {
            Width = 0,
            Height = 0,
            IsHitTestVisible = false,
        };

    private static FontFamily FontFamilyFor(string fontFamily)
    {
        if (!FontFamilyCache.TryGetValue(fontFamily, out var cached))
        {
            cached = new FontFamily(fontFamily);
            FontFamilyCache[fontFamily] = cached;
        }
        return cached;
    }

    private static string[] NormalizeLines(string text)
        => text.Replace("\r\n", "\n").Replace('\r', '\n').Split('\n');

    private void AddInlineMarkdown(Paragraph paragraph, string text)
    {
        int i = 0;
        while (i < text.Length)
        {
            if (text[i] == '`')
            {
                int end = text.IndexOf('`', i + 1);
                if (end > i)
                {
                    paragraph.Inlines.Add(new Run
                    {
                        Text = text[(i + 1)..end],
                        FontFamily = FontFamilyFor("Cascadia Mono, Consolas"),
                        Foreground = BrushFor(TokenKind.Keyword),
                    });
                    i = end + 1;
                    continue;
                }
            }
            if (i + 1 < text.Length && text[i] == '*' && text[i + 1] == '*')
            {
                int end = text.IndexOf("**", i + 2, StringComparison.Ordinal);
                if (end > i)
                {
                    paragraph.Inlines.Add(new Bold { Inlines = { new Run { Text = text[(i + 2)..end] } } });
                    i = end + 2;
                    continue;
                }
            }
            if (text[i] == '*')
            {
                int end = text.IndexOf('*', i + 1);
                if (end > i)
                {
                    paragraph.Inlines.Add(new Italic { Inlines = { new Run { Text = text[(i + 1)..end] } } });
                    i = end + 1;
                    continue;
                }
            }

            int next = NextMarkdownToken(text, i + 1);
            paragraph.Inlines.Add(new Run { Text = text[i..next] });
            i = next;
        }
    }

    private static int NextMarkdownToken(string text, int start)
    {
        int next = text.Length;
        foreach (char c in new[] { '`', '*' })
        {
            int at = text.IndexOf(c, start);
            if (at >= 0 && at < next) next = at;
        }
        return next;
    }

    private static Windows.UI.Color Rgb(byte r, byte g, byte b) => ColorHelper.FromArgb(255, r, g, b);

    private Windows.UI.Color ThemeTextColor()
    {
        var highContrast = _getHighContrast();
        if (highContrast.Enabled) return highContrast.Foreground;
        return ThemeResourceColor("TextFillColorPrimaryBrush", Colors.Gainsboro);
    }

    private Windows.UI.Color ThemeTextColorSecondary()
    {
        var highContrast = _getHighContrast();
        if (highContrast.Enabled) return highContrast.Foreground;
        return ThemeResourceColor("TextFillColorSecondaryBrush", Colors.Gray);
    }

    private Windows.UI.Color ThemeSurfaceBorderColor()
    {
        var highContrast = _getHighContrast();
        if (highContrast.Enabled) return highContrast.Foreground;
        try { return (Windows.UI.Color)Application.Current.Resources["CardStrokeColorDefault"]; }
        catch { return Colors.LightGray; }
    }

    private Windows.UI.Color ThemeCodeBackground()
    {
        var highContrast = _getHighContrast();
        if (highContrast.Enabled) return highContrast.Background;
        try { return (Windows.UI.Color)Application.Current.Resources["ControlFillColorDefault"]; }
        catch { return Colors.Transparent; }
    }

    private Windows.UI.Color ThemeHeaderBackground()
    {
        var highContrast = _getHighContrast();
        if (highContrast.Enabled) return highContrast.Background;
        try { return (Windows.UI.Color)Application.Current.Resources["ControlFillColorSecondary"]; }
        catch { return Colors.Transparent; }
    }

    private static Windows.UI.Color ThemeResourceColor(string key, Windows.UI.Color fallback)
    {
        try
        {
            object value = Application.Current.Resources[key];
            return value switch
            {
                Windows.UI.Color color => color,
                SolidColorBrush brush => brush.Color,
                _ => fallback,
            };
        }
        catch
        {
            return fallback;
        }
    }
}

public sealed class TextLineItem
{
    public int LineNumber { get; set; }
    public string Text { get; set; } = "";
    public object? Content { get; set; }
}

public sealed class MarkdownOutlineItem
{
    public MarkdownOutlineItem(string title, int level, FrameworkElement anchor)
    {
        Title = title;
        Level = level;
        Anchor = anchor;
    }

    public string Title { get; }

    public int Level { get; }

    public FrameworkElement Anchor { get; }

    public Thickness Margin => new(Math.Max(0, (Level - 1) * 12), 2, 0, 2);

    public double FontSize => Level <= 1 ? 13 : 12;

    public Windows.UI.Text.FontWeight FontWeight => new() { Weight = Level <= 2 ? (ushort)600 : (ushort)400 };
}

internal readonly record struct TextPreviewResult(string Status, double Width, double Height);
