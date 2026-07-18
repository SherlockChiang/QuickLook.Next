using System.Collections.ObjectModel;
using System.Text;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
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
    private const int MaxVirtualLineRuns = 512;
    private const int MaxSearchHighlightRanges = 5000;
    private const int MaxMarkdownBlocks = TextSearchIndex.MaxMarkdownBlocks;
    private const int MaxMarkdownSyntaxRuns = 10000;
    private const double OutlineWidth = 188;
    private const double OutlineGap = 10;

    private static readonly SolidColorBrush UiGrayBrush = new(Colors.Gray);
    private static readonly Dictionary<string, FontFamily> FontFamilyCache = new(StringComparer.Ordinal);

    private readonly RichTextBlock _textBlock;
    private readonly ScrollViewer _scrollViewer;
    private readonly Border _outlinePanel;
    private readonly ListView _outlineList;
    private readonly ListView _textListView;
    private readonly ListView _markdownListView;
    private readonly FrameworkElement _textPreviewContainer;
    private readonly Func<ElementTheme> _getTheme;
    private readonly Func<(bool Enabled, Windows.UI.Color Background, Windows.UI.Color Foreground)> _getHighContrast;
    private readonly ObservableCollection<MarkdownOutlineItem> _outlineItems = [];
    private readonly Dictionary<TokenKind, SolidColorBrush> _tokenBrushes = [];
    private readonly Dictionary<ListViewItem, RealizedTextLine> _realizedTextLines = [];
    private readonly Dictionary<ListViewItem, RealizedMarkdownItem> _realizedMarkdownItems = [];
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
    private readonly List<MarkdownSearchTarget> _markdownSearchTargets = [];
    private int _currentSearchMatch = -1;
    private MarkdownVisibleTextIndex? _markdownSearchIndex;
    private int _nextMarkdownSearchSegment;
    private int _markdownBlocksRendered;
    private bool _markdownRenderTruncated;
    private int _markdownSyntaxRunsRemaining;
    private bool _wrap;
    private bool _showLineNumbers;
    private TextLineIndex? _textLineIndex;
    private IReadOnlyList<TextLineItem>? _textLines;
    private MarkdownPresentation? _markdownPresentation;
    private IReadOnlyList<MarkdownListItem>? _markdownItems;

    public TextPreviewPresenter(
        RichTextBlock textBlock,
        ScrollViewer scrollViewer,
        ListView textListView,
        ListView markdownListView,
        FrameworkElement textPreviewContainer,
        Border outlinePanel,
        ListView outlineList,
        Func<ElementTheme> getTheme,
        Func<(bool Enabled, Windows.UI.Color Background, Windows.UI.Color Foreground)> getHighContrast)
    {
        _textBlock = textBlock;
        _scrollViewer = scrollViewer;
        _textListView = textListView;
        _markdownListView = markdownListView;
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
        _textListView.ContainerContentChanging += OnTextLineContainerChanging;
        _markdownListView.ContainerContentChanging += OnMarkdownContainerChanging;
        _markdownListView.LayoutUpdated += OnMarkdownListViewLayoutUpdated;
    }

    public TextPreviewResult Render(PreviewReady ready, (double Width, double Height) maxContent, bool wrap)
    {
        _lastReady = ready;
        _lastMaxContent = maxContent;
        bool isMarkdown = ready.TextFormat == "markdown" || ready.Markdown is not null;
        string text = isMarkdown ? TrimForDisplay(ready.TextContent ?? "") : ready.TextContent ?? "";
        _markdownPresentation = ready.Markdown is null
            ? null
            : MarkdownPresentationPolicy.Flatten(ready.Markdown, UiStrings.TextPreviewTruncated);
        _markdownSearchIndex = _markdownPresentation is null
            ? null
            : new MarkdownVisibleTextIndex(
                _markdownPresentation.Text,
                _markdownPresentation.Items.SelectMany(item => item.Segments).ToArray());
        _displayedText = _markdownSearchIndex?.Text ?? text;
        _markdownSearchAnchors.Clear();
        _markdownSearchTargets.Clear();
        _nextMarkdownSearchSegment = 0;
        _markdownBlocksRendered = 0;
        _markdownRenderTruncated = false;
        _markdownSyntaxRunsRemaining = MaxMarkdownSyntaxRuns;
        ClearSearch();
        int renderVersion = ++_renderVersion;
        DiagLog.Write("App", $"text preview: format={ready.TextFormat}; language={ready.TextLanguage}; chars={ready.TextContent?.Length ?? 0}; displayed={text.Length}");

        _textBlock.Blocks.Clear();
        _textBlock.IsTextSelectionEnabled = true;
        _textListView.ItemsSource = null;
        _markdownListView.ItemsSource = null;
        _textLines = null;
        _textLineIndex = null;
        _realizedTextLines.Clear();
        _markdownItems = null;
        _realizedMarkdownItems.Clear();
        ClearOutline();

        _wrap = wrap;
        bool isStructuredMarkdown = ready.Markdown is not null;
        _scrollViewer.Visibility = isMarkdown && !isStructuredMarkdown ? Visibility.Visible : Visibility.Collapsed;
        _textListView.Visibility = isMarkdown ? Visibility.Collapsed : Visibility.Visible;
        _markdownListView.Visibility = isStructuredMarkdown ? Visibility.Visible : Visibility.Collapsed;
        
        _scrollViewer.HorizontalScrollBarVisibility = wrap ? ScrollBarVisibility.Disabled : ScrollBarVisibility.Auto;
        _textBlock.FontFamily = FontFamilyFor(ready.TextFormat == "markdown" ? "Segoe UI" : "Cascadia Mono, Consolas");
        _textBlock.TextWrapping = wrap ? TextWrapping.Wrap : TextWrapping.NoWrap;

        try
        {
            if (ready.Markdown is not null)
                RenderVirtualMarkdown(_markdownPresentation!);
            else if (ready.TextFormat == "markdown")
                RenderMarkdown(text);
            else
                _ = RenderVirtualTextAsync(text, ready.TextLanguage ?? "text", renderVersion);
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "text render FAILED; falling back to plain text: " + ex);
            if (isMarkdown)
            {
                _scrollViewer.Visibility = Visibility.Visible;
                _textListView.Visibility = Visibility.Collapsed;
                _markdownListView.Visibility = Visibility.Collapsed;
                _ = RenderCodeOrPlainTextAsync(text, "text", renderVersion);
            }
            else
            {
                _ = RenderVirtualTextAsync(text, "text", renderVersion);
            }
        }

        ApplyOutlineVisibility();
        FrameworkElement focusTarget = isStructuredMarkdown
            ? _markdownListView
            : isMarkdown ? _textBlock : _textListView;
        focusTarget.Focus(FocusState.Programmatic);
        var size = EstimateTextPreviewSize(text, ready.TextFormat, wrap, maxContent);
        if (_outlineItems.Count > 0)
            size = (Math.Min(maxContent.Width, size.Width + OutlineWidth + OutlineGap), size.Height);
        return new TextPreviewResult($"{ready.Kind}: {ready.Title}", size.Width, size.Height);
    }

    public void Clear()
    {
        _lastReady = null;
        _displayedText = "";
        _markdownSearchIndex = null;
        _markdownSearchAnchors.Clear();
        _markdownSearchTargets.Clear();
        _nextMarkdownSearchSegment = 0;
        ClearSearch();
        _renderVersion++;
        _textBlock.Blocks.Clear();
        _textListView.ItemsSource = null;
        _markdownListView.ItemsSource = null;
        _textLines = null;
        _textLineIndex = null;
        _realizedTextLines.Clear();
        _markdownPresentation = null;
        _markdownItems = null;
        _realizedMarkdownItems.Clear();
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
        RefreshRealizedTextLines();
        RefreshRealizedMarkdownItems();
        foreach (MarkdownSearchTarget target in _markdownSearchTargets)
            target.TextBlock.TextHighlighters.Clear();
        return SearchState;
    }

    private TextSearchState SearchState
        => new(_currentSearchMatch >= 0 ? _currentSearchMatch + 1 : 0, _searchMatches.Count);

    private void ApplySearchHighlights()
    {
        _textBlock.TextHighlighters.Clear();
        foreach (MarkdownSearchTarget target in _markdownSearchTargets)
            target.TextBlock.TextHighlighters.Clear();
        if (_searchMatches.Count == 0)
            return;

        if (_lastReady?.Markdown is not null)
        {
            ApplyMarkdownSearchHighlights();
            return;
        }
        if (_lastReady?.TextFormat == "markdown")
            return;

        if (_textLines is not null)
        {
            RefreshRealizedTextLines();
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

    private void ApplyMarkdownSearchHighlights()
    {
        if (_markdownItems is not null)
        {
            RefreshRealizedMarkdownItems();
            return;
        }
        var allByTarget = new Dictionary<TextBlock, TextHighlighter>();
        int ranges = 0;
        int targetIndex = 0;
        foreach (int matchStart in _searchMatches)
        {
            int matchEnd = matchStart + _searchQuery.Length;
            while (targetIndex < _markdownSearchTargets.Count
                && _markdownSearchTargets[targetIndex].Start + _markdownSearchTargets[targetIndex].Length <= matchStart)
            {
                targetIndex++;
            }
            for (int index = targetIndex; index < _markdownSearchTargets.Count; index++)
            {
                MarkdownSearchTarget target = _markdownSearchTargets[index];
                if (target.Start >= matchEnd)
                    break;
                int start = Math.Max(matchStart, target.Start);
                int end = Math.Min(matchEnd, target.Start + target.Length);
                if (start >= end)
                    continue;
                if (!allByTarget.TryGetValue(target.TextBlock, out TextHighlighter? highlighter))
                {
                    highlighter = new TextHighlighter
                    {
                        Background = new SolidColorBrush(Windows.UI.Color.FromArgb(110, 255, 210, 64)),
                    };
                    allByTarget.Add(target.TextBlock, highlighter);
                    target.TextBlock.TextHighlighters.Add(highlighter);
                }
                highlighter.Ranges.Add(new TextRange
                {
                    StartIndex = target.LocalStart + start - target.Start,
                    Length = end - start,
                });
                if (++ranges >= MaxSearchHighlightRanges)
                    goto CurrentMatch;
            }
        }

    CurrentMatch:
        if (_currentSearchMatch < 0)
            return;
        int currentStart = _searchMatches[_currentSearchMatch];
        int currentEnd = currentStart + _searchQuery.Length;
        foreach (MarkdownSearchTarget target in _markdownSearchTargets)
        {
            int start = Math.Max(currentStart, target.Start);
            int end = Math.Min(currentEnd, target.Start + target.Length);
            if (start >= end)
                continue;
            var current = new TextHighlighter
            {
                Background = new SolidColorBrush(Windows.UI.Color.FromArgb(220, 255, 145, 48)),
            };
            current.Ranges.Add(new TextRange
            {
                StartIndex = target.LocalStart + start - target.Start,
                Length = end - start,
            });
            target.TextBlock.TextHighlighters.Add(current);
        }
    }

    private void ScrollToCurrentSearchMatch()
    {
        if (_currentSearchMatch < 0 || _displayedText.Length == 0)
            return;
        if (_textLines is not null && _textLineIndex is not null)
        {
            int lineIndex = _textLineIndex.FindLineIndex(_searchMatches[_currentSearchMatch]);
            _textListView.ScrollIntoView(_textLines[lineIndex], ScrollIntoViewAlignment.Leading);
            return;
        }
        if (_markdownItems is not null && _markdownPresentation is not null)
        {
            int match = _searchMatches[_currentSearchMatch];
            MarkdownListItem item = _markdownItems.LastOrDefault(candidate =>
                candidate.Item.Segments.Count > 0 && candidate.Item.Segments[0].Start <= match)
                ?? _markdownItems[0];
            _markdownListView.ScrollIntoView(item, ScrollIntoViewAlignment.Leading);
            return;
        }
        if (_scrollViewer.ScrollableHeight <= 0)
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
            Render(_lastReady, _lastMaxContent, _wrap);
    }

    public void SetWrapping(bool wrap)
    {
        _wrap = wrap;
        _scrollViewer.HorizontalScrollBarVisibility = wrap ? ScrollBarVisibility.Disabled : ScrollBarVisibility.Auto;
        _textBlock.TextWrapping = wrap ? TextWrapping.Wrap : TextWrapping.NoWrap;
        ScrollViewer.SetHorizontalScrollMode(_textListView, wrap ? ScrollMode.Disabled : ScrollMode.Enabled);
        ScrollViewer.SetHorizontalScrollBarVisibility(
            _textListView, wrap ? ScrollBarVisibility.Disabled : ScrollBarVisibility.Auto);
        RefreshRealizedTextLines();
    }

    public void SetLineNumbersVisible(bool visible)
    {
        _showLineNumbers = visible;
        RefreshRealizedTextLines();
    }

    public bool SupportsWrappingToggle
        => _lastReady is not null
            && _lastReady.Markdown is null
            && !string.Equals(_lastReady.TextFormat, "markdown", StringComparison.OrdinalIgnoreCase);

    public bool SupportsLineNumbers
        => _lastReady is not null
            && _lastReady.Markdown is null
            && !string.Equals(_lastReady.TextFormat, "markdown", StringComparison.OrdinalIgnoreCase);

    public bool ApplyWrappingMode(string mode)
    {
        if (_lastReady is null)
            return false;
        bool wrap = TextWrappingPolicy.ShouldWrap(mode, _lastReady.TextFormat, _lastReady.Markdown is not null);
        SetWrapping(wrap);
        return wrap;
    }

    private async Task RenderVirtualTextAsync(string text, string language, int renderVersion)
    {
        try
        {
            var result = await Task.Run(() => BuildVirtualTextLines(text, language));
            if (renderVersion != _renderVersion)
                return;
            _textLineIndex = result.Index;
            _textLines = result.Lines;
            _textListView.ItemsSource = result.Lines;
            ScrollViewer.SetHorizontalScrollMode(_textListView, _wrap ? ScrollMode.Disabled : ScrollMode.Enabled);
            ScrollViewer.SetHorizontalScrollBarVisibility(
                _textListView, _wrap ? ScrollBarVisibility.Disabled : ScrollBarVisibility.Auto);
            ScrollToCurrentSearchMatch();
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "virtual text render failed: " + ex.Message);
            if (renderVersion != _renderVersion)
                return;
            _textLineIndex = TextLineIndex.Create(text);
            _textLines = _textLineIndex.Lines
                .Select(line => new TextLineItem(line.Number, line.Start, text.Substring(line.Start, line.Length), []))
                .ToArray();
            _textListView.ItemsSource = _textLines;
        }
    }

    private static (TextLineIndex Index, TextLineItem[] Lines) BuildVirtualTextLines(string text, string language)
    {
        TextLineIndex index = TextLineIndex.Create(text);
        bool highlight = language is not ("text" or "log");
        List<(string Text, TokenKind Kind)> tokens = highlight
            ? MergeAdjacentSpans(SyntaxHighlighter.Highlight(text, language))
            : [];
        var positioned = new List<(int Start, int End, TokenKind Kind)>();
        int position = 0;
        foreach ((string tokenText, TokenKind kind) in tokens)
        {
            positioned.Add((position, position + tokenText.Length, kind));
            position += tokenText.Length;
        }
        if (highlight && position != text.Length)
            throw new InvalidDataException("Syntax highlighting did not preserve the virtualized text.");

        var lines = new TextLineItem[index.Lines.Count];
        int tokenIndex = 0;
        for (int lineIndex = 0; lineIndex < index.Lines.Count; lineIndex++)
        {
            TextLineRange line = index.Lines[lineIndex];
            int lineEnd = line.Start + line.Length;
            while (tokenIndex < positioned.Count && positioned[tokenIndex].End <= line.Start)
                tokenIndex++;
            var lineTokens = new List<TextLineToken>();
            for (int i = tokenIndex; i < positioned.Count && positioned[i].Start < lineEnd; i++)
            {
                int start = Math.Max(line.Start, positioned[i].Start);
                int end = Math.Min(lineEnd, positioned[i].End);
                if (start < end)
                    lineTokens.Add(new TextLineToken(start - line.Start, end - start, positioned[i].Kind));
                if (lineTokens.Count > MaxVirtualLineRuns)
                {
                    lineTokens.Clear();
                    break;
                }
            }
            lines[lineIndex] = new TextLineItem(
                line.Number,
                line.Start,
                text.Substring(line.Start, line.Length),
                lineTokens.ToArray());
        }
        return (index, lines);
    }

    private void OnTextLineContainerChanging(ListViewBase sender, ContainerContentChangingEventArgs args)
    {
        if (args.ItemContainer is not ListViewItem container)
            return;
        if (args.InRecycleQueue)
        {
            _realizedTextLines.Remove(container);
            return;
        }
        if (args.Item is not TextLineItem item
            || container.ContentTemplateRoot is not Grid root
            || root.FindName("LineNumberText") is not TextBlock lineNumber
            || root.FindName("LineContentText") is not TextBlock textBlock)
        {
            if (args.Phase == 0)
                args.RegisterUpdateCallback(OnTextLineContainerChanging);
            return;
        }
        var realized = new RealizedTextLine(item, root, lineNumber, textBlock);
        _realizedTextLines[container] = realized;
        ApplyTextLineVisual(realized);
    }

    private void RefreshRealizedTextLines()
    {
        foreach (RealizedTextLine line in _realizedTextLines.Values.ToArray())
            ApplyTextLineVisual(line);
    }

    private void ApplyTextLineVisual(RealizedTextLine realized)
    {
        realized.Root.ColumnDefinitions[0].Width = _showLineNumbers ? new GridLength(60) : new GridLength(0);
        realized.LineNumber.Visibility = _showLineNumbers ? Visibility.Visible : Visibility.Collapsed;
        TextBlock textBlock = realized.TextBlock;
        textBlock.TextWrapping = _wrap ? TextWrapping.Wrap : TextWrapping.NoWrap;
        textBlock.Inlines.Clear();
        textBlock.TextHighlighters.Clear();
        if (realized.Item.Tokens.Count == 0)
        {
            textBlock.Text = realized.Item.Text;
        }
        else
        {
            textBlock.Text = "";
            int position = 0;
            foreach (TextLineToken token in realized.Item.Tokens)
            {
                if (token.Start > position)
                    textBlock.Inlines.Add(new Run { Text = realized.Item.Text[position..token.Start] });
                textBlock.Inlines.Add(new Run
                {
                    Text = realized.Item.Text.Substring(token.Start, token.Length),
                    Foreground = BrushFor(token.Kind),
                });
                position = token.Start + token.Length;
            }
            if (position < realized.Item.Text.Length)
                textBlock.Inlines.Add(new Run { Text = realized.Item.Text[position..] });
        }
        ApplyTextLineSearchHighlights(realized.Item, textBlock);
    }

    private void ApplyTextLineSearchHighlights(TextLineItem item, TextBlock textBlock)
    {
        if (_searchMatches.Count == 0 || _searchQuery.Length == 0)
            return;
        int lineEnd = item.Start + item.Text.Length;
        var all = new TextHighlighter
        {
            Background = new SolidColorBrush(Windows.UI.Color.FromArgb(110, 255, 210, 64)),
        };
        foreach (int matchStart in _searchMatches.Take(MaxSearchHighlightRanges))
        {
            int start = Math.Max(matchStart, item.Start);
            int end = Math.Min(matchStart + _searchQuery.Length, lineEnd);
            if (start < end)
                all.Ranges.Add(new TextRange { StartIndex = start - item.Start, Length = end - start });
        }
        if (all.Ranges.Count > 0)
            textBlock.TextHighlighters.Add(all);
        if (_currentSearchMatch < 0)
            return;
        int currentStart = _searchMatches[_currentSearchMatch];
        int startCurrent = Math.Max(currentStart, item.Start);
        int endCurrent = Math.Min(currentStart + _searchQuery.Length, lineEnd);
        if (startCurrent < endCurrent)
        {
            var current = new TextHighlighter
            {
                Background = new SolidColorBrush(Windows.UI.Color.FromArgb(220, 255, 145, 48)),
            };
            current.Ranges.Add(new TextRange
            {
                StartIndex = startCurrent - item.Start,
                Length = endCurrent - startCurrent,
            });
            textBlock.TextHighlighters.Add(current);
        }
    }

    private void RenderVirtualMarkdown(MarkdownPresentation presentation)
    {
        _markdownItems = presentation.Items.Select(item => new MarkdownListItem(item)).ToArray();
        _markdownListView.ItemsSource = _markdownItems;
        foreach (MarkdownListItem item in _markdownItems)
        {
            if (item.Item.Block.Kind != "heading")
                continue;
            string title = TextSearchIndex.MarkdownInlineText(item.Item.Block.Inlines, item.Item.Block.Text).Trim();
            if (title.Length > 0)
                _outlineItems.Add(new MarkdownOutlineItem(title, Math.Clamp(item.Item.Block.Level, 1, 6), item.Item.Index));
        }
    }

    private void OnMarkdownContainerChanging(ListViewBase sender, ContainerContentChangingEventArgs args)
    {
        if (args.ItemContainer is not ListViewItem container)
            return;
        if (args.InRecycleQueue)
        {
            if (_realizedMarkdownItems.Remove(container, out RealizedMarkdownItem recycled))
            {
                foreach (VirtualMarkdownSearchTarget target in recycled.Targets)
                    target.TextBlock.TextHighlighters.Clear();
                recycled.Host.Content = null;
            }
            return;
        }
        if (args.Item is not MarkdownListItem item
            || container.ContentTemplateRoot is not FrameworkElement root
            || root.FindName("MarkdownContentHost") is not ContentControl host)
        {
            if (args.Phase == 0)
                args.RegisterUpdateCallback(OnMarkdownContainerChanging);
            return;
        }
        var targets = new List<VirtualMarkdownSearchTarget>();
        host.Content = CreateVirtualMarkdownElement(item.Item, targets);
        var realized = new RealizedMarkdownItem(item, container, host, targets);
        _realizedMarkdownItems[container] = realized;
        ApplyVirtualMarkdownSearchHighlights(realized);
    }

    private FrameworkElement CreateVirtualMarkdownElement(
        MarkdownRenderItem item, List<VirtualMarkdownSearchTarget> targets)
    {
        PreviewMarkdownBlock block = item.Block;
        if (block.Kind is "tableHeader" or "tableRow")
            return CreateVirtualMarkdownTableRow(item, targets);
        if (block.Kind == "thematicBreak")
            return new Border
            {
                Height = 1,
                Margin = new Thickness(0, 10, 0, 12),
                Background = new SolidColorBrush(ThemeSurfaceBorderColor()),
            };

        double fontSize = block.Kind == "heading" ? block.Level switch
        {
            <= 1 => 28,
            2 => 23,
            3 => 19,
            _ => 16,
        } : block.Kind == "partial" ? 12 : 14;
        var textBlock = new TextBlock
        {
            FontSize = fontSize,
            FontFamily = FontFamilyFor(block.Kind == "code" ? "Cascadia Mono, Consolas" : "Segoe UI"),
            FontWeight = block.Kind == "heading" ? Microsoft.UI.Text.FontWeights.Bold : Microsoft.UI.Text.FontWeights.Normal,
            TextWrapping = block.Kind == "code" ? TextWrapping.NoWrap : TextWrapping.Wrap,
            IsTextSelectionEnabled = true,
            Foreground = block.Kind is "blockquote" or "partial" ? UiGrayBrush : null,
            Margin = block.Kind switch
            {
                "heading" => new Thickness(0, block.Level <= 2 ? 16 : 10, 0, 8),
                "partial" => new Thickness(0, 12, 0, 0),
                _ => new Thickness(block.Kind == "listItem" ? 18 : 0, 0, 0, 9),
            },
        };
        string prefix = block.Kind == "blockquote" ? "| " : item.Prefix;
        if (prefix.Length > 0)
            textBlock.Inlines.Add(new Run { Text = prefix });
        if (block.Kind == "code")
        {
            string language = string.IsNullOrWhiteSpace(block.Language) ? "text" : block.Language;
            List<(string Text, TokenKind Kind)> spans = language is "text" or "log"
                ? []
                : MergeAdjacentSpans(SyntaxHighlighter.Highlight(block.Text, language));
            if (spans.Count is 0 or > MaxVirtualLineRuns)
                textBlock.Text = prefix + block.Text;
            else
                foreach ((string text, TokenKind kind) in spans)
                    textBlock.Inlines.Add(new Run { Text = text, Foreground = BrushFor(kind) });
        }
        else
        {
            AddMarkdownInlines(textBlock.Inlines, block.Inlines, block.Text);
        }
        if (item.Segments.Count > 0)
            targets.Add(new VirtualMarkdownSearchTarget(
                item.Segments[0].Start, item.Segments[0].Text.Length, textBlock, prefix.Length));
        if (block.Kind != "code")
            return textBlock;
        var scroller = new ScrollViewer
        {
            HorizontalScrollBarVisibility = ScrollBarVisibility.Auto,
            VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
            Content = textBlock,
        };
        var copyButton = new Button
        {
            Content = UiStrings.CopyAction,
            HorizontalAlignment = HorizontalAlignment.Right,
            Padding = new Thickness(10, 3, 10, 3),
        };
        AutomationProperties.SetName(copyButton, UiStrings.CopyAction);
        copyButton.Click += (_, _) =>
        {
            var package = new DataPackage();
            package.SetText(block.Text);
            Clipboard.SetContent(package);
            copyButton.Content = UiStrings.CopiedAction;
        };
        var codeGrid = new Grid();
        codeGrid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        codeGrid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        codeGrid.Children.Add(copyButton);
        Grid.SetRow(scroller, 1);
        codeGrid.Children.Add(scroller);
        return new Border
        {
            Padding = new Thickness(12),
            Margin = new Thickness(0, 8, 0, 12),
            CornerRadius = new CornerRadius(6),
            BorderThickness = new Thickness(1),
            BorderBrush = new SolidColorBrush(ThemeSurfaceBorderColor()),
            Background = new SolidColorBrush(ThemeCodeBackground()),
            Child = codeGrid,
        };
    }

    private FrameworkElement CreateVirtualMarkdownTableRow(
        MarkdownRenderItem item, List<VirtualMarkdownSearchTarget> targets)
    {
        string[] cells = item.Block.TableHeaders;
        var grid = new Grid { HorizontalAlignment = HorizontalAlignment.Left };
        for (int index = 0; index < cells.Length; index++)
        {
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(180) });
            var text = new TextBlock
            {
                Text = cells[index],
                TextWrapping = TextWrapping.Wrap,
                IsTextSelectionEnabled = true,
                FontWeight = item.Block.Kind == "tableHeader"
                    ? Microsoft.UI.Text.FontWeights.SemiBold
                    : Microsoft.UI.Text.FontWeights.Normal,
            };
            if (index < item.Segments.Count)
                targets.Add(new VirtualMarkdownSearchTarget(
                    item.Segments[index].Start, item.Segments[index].Text.Length, text, 0));
            var border = new Border
            {
                Padding = new Thickness(10, 7, 10, 7),
                BorderBrush = new SolidColorBrush(ThemeSurfaceBorderColor()),
                BorderThickness = new Thickness(0, 0, 1, 1),
                Background = item.Block.Kind == "tableHeader"
                    ? new SolidColorBrush(ThemeHeaderBackground())
                    : null,
                Child = text,
            };
            Grid.SetColumn(border, index);
            grid.Children.Add(border);
        }
        return grid;
    }

    private void RefreshRealizedMarkdownItems()
    {
        foreach (RealizedMarkdownItem item in _realizedMarkdownItems.Values.ToArray())
            ApplyVirtualMarkdownSearchHighlights(item);
    }

    private void ApplyVirtualMarkdownSearchHighlights(RealizedMarkdownItem item)
    {
        foreach (VirtualMarkdownSearchTarget target in item.Targets)
            target.TextBlock.TextHighlighters.Clear();
        if (_searchMatches.Count == 0 || _searchQuery.Length == 0)
            return;
        foreach (VirtualMarkdownSearchTarget target in item.Targets)
        {
            var all = new TextHighlighter
            {
                Background = new SolidColorBrush(Windows.UI.Color.FromArgb(110, 255, 210, 64)),
            };
            foreach (int matchStart in _searchMatches.Take(MaxSearchHighlightRanges))
            {
                int start = Math.Max(matchStart, target.Start);
                int end = Math.Min(matchStart + _searchQuery.Length, target.Start + target.Length);
                if (start < end)
                    all.Ranges.Add(new TextRange
                    {
                        StartIndex = target.LocalStart + start - target.Start,
                        Length = end - start,
                    });
            }
            if (all.Ranges.Count > 0)
                target.TextBlock.TextHighlighters.Add(all);
            if (_currentSearchMatch < 0)
                continue;
            int currentStart = _searchMatches[_currentSearchMatch];
            int localStart = Math.Max(currentStart, target.Start);
            int currentEnd = Math.Min(currentStart + _searchQuery.Length, target.Start + target.Length);
            if (localStart < currentEnd)
            {
                var current = new TextHighlighter
                {
                    Background = new SolidColorBrush(Windows.UI.Color.FromArgb(220, 255, 145, 48)),
                };
                current.Ranges.Add(new TextRange
                {
                    StartIndex = target.LocalStart + localStart - target.Start,
                    Length = currentEnd - localStart,
                });
                target.TextBlock.TextHighlighters.Add(current);
            }
        }
    }

    private void OnMarkdownListViewLayoutUpdated(object? sender, object e)
    {
        if (_updatingOutline || _outlineItems.Count == 0 || _markdownListView.Visibility != Visibility.Visible)
            return;
        int firstVisible;
        try
        {
            firstVisible = _realizedMarkdownItems.Values
                .Where(item => item.Container.TransformToVisual(_markdownListView)
                    .TransformPoint(new Windows.Foundation.Point()).Y <= 24)
                .Select(item => item.Item.Item.Index)
                .DefaultIfEmpty(0)
                .Max();
        }
        catch
        {
            return;
        }
        MarkdownOutlineItem? closest = _outlineItems.LastOrDefault(item => item.ItemIndex <= firstVisible)
            ?? _outlineItems.FirstOrDefault();
        if (closest is not null && _outlineList.SelectedItem != closest)
        {
            _updatingOutline = true;
            _outlineList.SelectedItem = closest;
            _outlineList.ScrollIntoView(closest);
            _updatingOutline = false;
        }
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
            var partialText = CreateMarkdownTextBlock(12, "Segoe UI");
            partialText.Foreground = UiGrayBrush;
            partialText.Inlines.Add(new Run { Text = UiStrings.TextPreviewTruncated });
            partial.Inlines.Add(new InlineUIContainer { Child = partialText });
            _textBlock.Blocks.Add(partial);
            RegisterMarkdownSearchTarget(partialText, partialText, 0);
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
        var text = CreateMarkdownTextBlock(size, "Segoe UI");
        text.FontWeight = Microsoft.UI.Text.FontWeights.Bold;
        AddMarkdownInlines(text.Inlines, block.Inlines, block.Text);
        p.Inlines.Add(new InlineUIContainer { Child = text });
        RegisterMarkdownSearchTarget(anchor, text, 0);
        _textBlock.Blocks.Add(p);
        AddOutlineItem(block, anchor);
    }

    private void AddMarkdownParagraph(PreviewMarkdownBlock block)
    {
        var p = CreateParagraph(14, "Segoe UI", 0, 9);
        FrameworkElement anchor = CreateHeadingAnchor();
        p.Inlines.Add(new InlineUIContainer { Child = anchor });
        var text = CreateMarkdownTextBlock(14, "Segoe UI");
        AddMarkdownInlines(text.Inlines, block.Inlines, block.Text);
        p.Inlines.Add(new InlineUIContainer { Child = text });
        RegisterMarkdownSearchTarget(anchor, text, 0);
        _textBlock.Blocks.Add(p);
    }

    private void AddMarkdownQuote(PreviewMarkdownBlock block)
    {
        var p = CreateParagraph(14, "Segoe UI", 4, 10);
        p.Foreground = UiGrayBrush;
        FrameworkElement anchor = CreateHeadingAnchor();
        p.Inlines.Add(new InlineUIContainer { Child = anchor });
        var text = CreateMarkdownTextBlock(14, "Segoe UI");
        text.Foreground = UiGrayBrush;
        text.Inlines.Add(new Run { Text = "| " });
        AddMarkdownInlines(text.Inlines, block.Inlines, block.Text);
        p.Inlines.Add(new InlineUIContainer { Child = text });
        RegisterMarkdownSearchTarget(anchor, text, 2);
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
            string prefix = ordered ? $"{index}. " : "- ";
            var text = CreateMarkdownTextBlock(14, "Segoe UI");
            text.Inlines.Add(new Run { Text = prefix });
            AddMarkdownInlines(text.Inlines, item.Inlines, item.Text);
            p.Inlines.Add(new InlineUIContainer { Child = text });
            RegisterMarkdownSearchTarget(anchor, text, prefix.Length);
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
            if (c < block.TableHeaders.Length)
                RegisterMarkdownSearchTarget(container, textBlock, 0);
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
                if (c < row.Length)
                    RegisterMarkdownSearchTarget(container, textBlock, 0);
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
        var p = new Paragraph();
        p.Inlines.Add(new InlineUIContainer { Child = container });
        _textBlock.Blocks.Add(p);
    }

    private void RegisterMarkdownSearchTarget(FrameworkElement anchor, TextBlock textBlock, int localStart)
    {
        if (_markdownSearchIndex is null || _nextMarkdownSearchSegment >= _markdownSearchIndex.Segments.Count)
            return;
        MarkdownVisibleSegment segment = _markdownSearchIndex.Segments[_nextMarkdownSearchSegment++];
        _markdownSearchAnchors.Add((segment.Start, anchor));
        _markdownSearchTargets.Add(new MarkdownSearchTarget(segment.Start, segment.Text.Length, textBlock, localStart));
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

        int version = _renderVersion;
        _updatingOutline = true;
        if (item.ItemIndex >= 0 && _markdownItems is not null && item.ItemIndex < _markdownItems.Count)
        {
            _markdownListView.ScrollIntoView(_markdownItems[item.ItemIndex], ScrollIntoViewAlignment.Leading);
        }
        else if (item.Anchor is not null)
        {
            _scrollViewer.UpdateLayout();
            _textBlock.UpdateLayout();
            double target = Math.Max(0, _scrollViewer.VerticalOffset
                + item.Anchor.TransformToVisual(_scrollViewer).TransformPoint(new Windows.Foundation.Point(0, 0)).Y - 8);
            _scrollViewer.ChangeView(null, target, null, disableAnimation: false);
        }

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
        if (_markdownItems is not null || _updatingOutline || _outlineItems.Count == 0 || e.IsIntermediate)
            return;

        try
        {
            MarkdownOutlineItem? closest = null;
            double closestY = double.MinValue;
            foreach (var item in _outlineItems)
            {
                if (item.Anchor is not FrameworkElement anchor)
                    continue;
                var point = anchor.TransformToVisual(_scrollViewer).TransformPoint(new Windows.Foundation.Point(0, 0));
                if (point.Y <= 24 && point.Y > closestY)
                {
                    closest = item;
                    closestY = point.Y;
                }
            }

            if (closest == null && _outlineItems.Count > 0)
            {
                var first = _outlineItems[0];
                if (first.Anchor is FrameworkElement anchor
                    && anchor.TransformToVisual(_scrollViewer).TransformPoint(new Windows.Foundation.Point(0, 0)).Y > 0)
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

        bool FlushParagraph()
        {
            if (paragraphBuffer.Count == 0) return true;
            if (!TryReserveMarkdownBlock())
            {
                paragraphBuffer.Clear();
                return false;
            }
            var p = CreateParagraph(14, "Segoe UI", 0, 8);
            AddInlineMarkdown(p, string.Join(" ", paragraphBuffer));
            _textBlock.Blocks.Add(p);
            paragraphBuffer.Clear();
            return true;
        }

        foreach (string raw in lines)
        {
            string line = raw.TrimEnd('\r');
            if (line.TrimStart().StartsWith("```", StringComparison.Ordinal))
            {
                if (inCode)
                {
                    if (!TryReserveMarkdownBlock())
                        break;
                    AddMarkdownCodeBlock(code.ToString().TrimEnd('\n'), codeLanguage);
                    code.Clear();
                    codeLanguage = "text";
                    inCode = false;
                }
                else
                {
                    if (!FlushParagraph())
                        break;
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
                if (!FlushParagraph())
                    break;
                continue;
            }

            if (trimmed.StartsWith("#", StringComparison.Ordinal))
            {
                if (!FlushParagraph() || !TryReserveMarkdownBlock())
                    break;
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
                if (!FlushParagraph() || !TryReserveMarkdownBlock())
                    break;
                var p = CreateParagraph(14, "Segoe UI", 4, 8);
                p.Foreground = UiGrayBrush;
                p.Inlines.Add(new Run { Text = "│ " });
                AddInlineMarkdown(p, trimmed[2..]);
                _textBlock.Blocks.Add(p);
                continue;
            }

            if (trimmed.StartsWith("- ", StringComparison.Ordinal) || trimmed.StartsWith("* ", StringComparison.Ordinal))
            {
                if (!FlushParagraph() || !TryReserveMarkdownBlock())
                    break;
                var p = CreateParagraph(14, "Segoe UI", 2, 4);
                p.Inlines.Add(new Run { Text = "• " });
                AddInlineMarkdown(p, trimmed[2..]);
                _textBlock.Blocks.Add(p);
                continue;
            }

            paragraphBuffer.Add(trimmed);
        }

        if (!_markdownRenderTruncated)
            FlushParagraph();
        if (!_markdownRenderTruncated && inCode && code.Length > 0 && TryReserveMarkdownBlock())
        {
            // For simplicity in this demo, markdown parsing is synchronous but code highlighting can be async
            // To preserve order properly we should await it, but RenderMarkdown is synchronous right now.
            // A quick fix is to fire-and-forget or keep markdown code highlighting synchronous if it's small,
            // but for full files, we use RenderCodeOrPlainTextAsync.
            AddMarkdownCodeBlock(code.ToString().TrimEnd('\n'), codeLanguage);
        }
        if (_markdownRenderTruncated)
        {
            var partial = CreateParagraph(12, "Segoe UI", 12, 0);
            partial.Foreground = UiGrayBrush;
            partial.Inlines.Add(new Run { Text = UiStrings.TextPreviewTruncated });
            _textBlock.Blocks.Add(partial);
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
                        + "\n\n" + UiStrings.Format(UiStrings.SyntaxHighlightingCharacterLimitFormat, MaxHighlightedChars),
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
                    paragraph.Inlines.Add(new Run { Text = code + "\n\n" + UiStrings.Format(UiStrings.SyntaxHighlightingSpanLimitFormat, MaxHighlightedRuns) });
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
                        + "\n\n" + UiStrings.Format(UiStrings.SyntaxHighlightingCharacterLimitFormat, MaxHighlightedChars),
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
            int runs = spans.Count(span => span.Text.Length > 0);
            if (runs > MaxHighlightedRuns || runs > _markdownSyntaxRunsRemaining)
            {
                DiagLog.Write("App", $"highlight run budget exceeded: language={language}; chars={code.Length}; runs={runs}; remaining={_markdownSyntaxRunsRemaining}");
                bodyText.Foreground = BrushFor(TokenKind.Default);
                bodyText.Inlines.Add(new Run { Text = code + "\n\n" + UiStrings.Format(UiStrings.SyntaxHighlightingSpanLimitFormat, MaxHighlightedRuns) });
            }
            else
            {
                _markdownSyntaxRunsRemaining -= runs;
                foreach (var (txt, kind) in spans)
                {
                    if (txt.Length > 0)
                        bodyText.Inlines.Add(new Run { Text = txt, Foreground = BrushFor(kind) });
                }
            }
        }
        
        bodyScroller.Content = bodyText;
        Grid.SetRow(bodyScroller, 1);
        grid.Children.Add(bodyScroller);
        
        container.Child = grid;

        if (_lastReady?.Markdown is not null)
            RegisterMarkdownSearchTarget(container, bodyText, 0);

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
            : text[..MaxHighlightedChars] + "\n\n" + UiStrings.Format(UiStrings.TextPreviewTruncatedAtCharacterCountFormat, MaxHighlightedChars);

    private static Paragraph CreateParagraph(double fontSize, string fontFamily, double top, double bottom)
        => new()
        {
            FontSize = fontSize,
            FontFamily = FontFamilyFor(fontFamily),
            Margin = new Thickness(0, top, 0, bottom),
        };

    private static TextBlock CreateMarkdownTextBlock(double fontSize, string fontFamily)
        => new()
        {
            FontSize = fontSize,
            FontFamily = FontFamilyFor(fontFamily),
            TextWrapping = TextWrapping.Wrap,
            IsTextSelectionEnabled = true,
            MaxWidth = 980,
        };

    private static FrameworkElement CreateHeadingAnchor()
        => new Border
        {
            Width = 0,
            Height = 0,
            IsHitTestVisible = false,
        };

    private readonly record struct MarkdownSearchTarget(
        int Start,
        int Length,
        TextBlock TextBlock,
        int LocalStart);

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

public readonly record struct TextLineToken(int Start, int Length, TokenKind Kind);

public sealed record TextLineItem(int LineNumber, int Start, string Text, IReadOnlyList<TextLineToken> Tokens);

internal readonly record struct RealizedTextLine(
    TextLineItem Item,
    Grid Root,
    TextBlock LineNumber,
    TextBlock TextBlock);

public sealed class MarkdownOutlineItem
{
    public MarkdownOutlineItem(string title, int level, FrameworkElement anchor)
    {
        Title = title;
        Level = level;
        Anchor = anchor;
        ItemIndex = -1;
    }

    public MarkdownOutlineItem(string title, int level, int itemIndex)
    {
        Title = title;
        Level = level;
        ItemIndex = itemIndex;
    }

    public string Title { get; }

    public int Level { get; }

    public FrameworkElement? Anchor { get; }

    public int ItemIndex { get; }

    public Thickness Margin => new(Math.Max(0, (Level - 1) * 12), 2, 0, 2);

    public double FontSize => Level <= 1 ? 13 : 12;

    public Windows.UI.Text.FontWeight FontWeight => new() { Weight = Level <= 2 ? (ushort)600 : (ushort)400 };
}

public sealed record MarkdownListItem(MarkdownRenderItem Item);

internal readonly record struct VirtualMarkdownSearchTarget(
    int Start,
    int Length,
    TextBlock TextBlock,
    int LocalStart);

internal readonly record struct RealizedMarkdownItem(
    MarkdownListItem Item,
    ListViewItem Container,
    ContentControl Host,
    IReadOnlyList<VirtualMarkdownSearchTarget> Targets);

internal readonly record struct TextPreviewResult(string Status, double Width, double Height);
