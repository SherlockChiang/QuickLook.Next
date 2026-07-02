using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class TextPreviewPresenter
{
    private const int MaxHighlightedChars = 256 * 1024;
    private const int MaxHighlightedRuns = 7000;

    private static readonly SolidColorBrush UiGrayBrush = new(Colors.Gray);
    private static readonly Dictionary<string, FontFamily> FontFamilyCache = new(StringComparer.Ordinal);

    private readonly RichTextBlock _textBlock;
    private readonly ScrollViewer _scrollViewer;
    private readonly Func<ElementTheme> _getTheme;
    private readonly Dictionary<TokenKind, SolidColorBrush> _tokenBrushes = new();
    private bool? _brushThemeDark;

    public TextPreviewPresenter(
        RichTextBlock textBlock,
        ScrollViewer scrollViewer,
        Func<ElementTheme> getTheme)
    {
        _textBlock = textBlock;
        _scrollViewer = scrollViewer;
        _getTheme = getTheme;
    }

    public TextPreviewResult Render(PreviewReady ready, (double Width, double Height) maxContent)
    {
        string text = TrimForDisplay(ready.TextContent ?? "");
        DiagLog.Write("App", $"text preview: format={ready.TextFormat}; language={ready.TextLanguage}; chars={ready.TextContent?.Length ?? 0}; displayed={text.Length}");

        _textBlock.Blocks.Clear();
        _textBlock.IsTextSelectionEnabled = true;

        bool wrap = ready.TextFormat is "markdown" or "plain";
        _scrollViewer.HorizontalScrollBarVisibility = wrap ? ScrollBarVisibility.Disabled : ScrollBarVisibility.Auto;
        _textBlock.FontFamily = FontFamilyFor(ready.TextFormat == "markdown" ? "Segoe UI" : "Cascadia Mono, Consolas");
        _textBlock.TextWrapping = wrap ? TextWrapping.Wrap : TextWrapping.NoWrap;

        try
        {
            if (ready.TextFormat == "markdown")
                RenderMarkdown(text);
            else
                RenderCodeOrPlainText(text, ready.TextLanguage ?? "text");
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "text render FAILED; falling back to plain text: " + ex);
            _textBlock.Blocks.Clear();
            _textBlock.FontFamily = FontFamilyFor("Cascadia Mono, Consolas");
            _textBlock.TextWrapping = TextWrapping.NoWrap;
            _scrollViewer.HorizontalScrollBarVisibility = ScrollBarVisibility.Auto;
            AddCodeBlock(text);
        }

        _textBlock.Focus(FocusState.Programmatic);
        var size = EstimateTextPreviewSize(text, ready.TextFormat, wrap, maxContent);
        return new TextPreviewResult($"{ready.Kind}: {ready.Title}", size.Width, size.Height);
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
        string code = "";
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
                    AddMarkdownCodeBlock(code.TrimEnd('\n'), codeLanguage);
                    code = "";
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
                code += raw + "\n";
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
                p.Inlines.Add(new Bold { Inlines = { new Run { Text = title } } });
                _textBlock.Blocks.Add(p);
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
            AddMarkdownCodeBlock(code.TrimEnd('\n'), codeLanguage);
    }

    private void AddMarkdownCodeBlock(string code, string language)
    {
        if (language is "text" or "log")
            AddCodeBlock(code);
        else
            AddHighlightedCode(code, language);
    }

    private void RenderCodeOrPlainText(string text, string language)
    {
        var header = CreateParagraph(12, "Segoe UI", 0, 10);
        header.Foreground = UiGrayBrush;
        header.Inlines.Add(new Run { Text = language });
        _textBlock.Blocks.Add(header);

        string code = text.TrimEnd('\r', '\n');
        if (language is "text" or "log")
            AddCodeBlock(code);
        else
            AddHighlightedCode(code, language);
    }

    private void AddHighlightedCode(string code, string language)
    {
        var p = CreateParagraph(13, "Cascadia Mono, Consolas", 2, 10);
        if (code.Length == 0)
        {
            p.Inlines.Add(new Run { Text = " " });
        }
        else if (code.Length > MaxHighlightedChars)
        {
            p.Foreground = BrushFor(TokenKind.Default);
            p.Inlines.Add(new Run
            {
                Text = code[..MaxHighlightedChars]
                    + $"\n\n[Syntax highlighting disabled after {MaxHighlightedChars:N0} characters]",
            });
        }
        else
        {
            int runs = 0;
            foreach (var (txt, kind) in SyntaxHighlighter.Highlight(code, language))
            {
                if (txt.Length == 0) continue;
                if (++runs > MaxHighlightedRuns)
                {
                    DiagLog.Write("App", $"highlight run limit hit: language={language}; chars={code.Length}; runs>{MaxHighlightedRuns}");
                    p.Inlines.Clear();
                    p.Foreground = BrushFor(TokenKind.Default);
                    p.Inlines.Add(new Run { Text = code + $"\n\n[Syntax highlighting disabled after {MaxHighlightedRuns:N0} spans]" });
                    break;
                }

                p.Inlines.Add(new Run { Text = txt, Foreground = BrushFor(kind) });
            }
        }
        _textBlock.Blocks.Add(p);
    }

    private SolidColorBrush BrushFor(TokenKind kind)
    {
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

    private void AddCodeBlock(string code)
    {
        code = TrimForDisplay(code);
        var p = CreateParagraph(13, "Cascadia Mono, Consolas", 2, 10);
        p.Foreground = BrushFor(TokenKind.Default);
        p.Inlines.Add(new Run { Text = code.Length == 0 ? " " : code });
        _textBlock.Blocks.Add(p);
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

    private static Windows.UI.Color ThemeTextColor()
    {
        try { return (Windows.UI.Color)Application.Current.Resources["TextFillColorPrimary"]; }
        catch { return Colors.Gainsboro; }
    }
}

internal readonly record struct TextPreviewResult(string Status, double Width, double Height);
