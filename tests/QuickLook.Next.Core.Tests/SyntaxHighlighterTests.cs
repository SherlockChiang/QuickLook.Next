using Xunit;

namespace QuickLook.Next.App;

public sealed class SyntaxHighlighterTests
{
    [Fact]
    public void BuildTokensFillsGapsSoTokenTextConcatenatesToTheInput()
    {
        List<SyntaxHighlighter.NativeSpan> spans =
        [
            new(0, 3, TokenKind.Keyword),
            new(6, 2, TokenKind.Number),
        ];

        var tokens = SyntaxHighlighter.BuildTokens("let x=42;", spans);

        Assert.Equal("let x=42;", string.Concat(tokens.Select(token => token.Text)));
        Assert.Equal(
            new[] { (TokenKind.Keyword, "let"), (TokenKind.Default, " x="), (TokenKind.Number, "42"), (TokenKind.Default, ";") },
            tokens.Select(token => (token.Kind, token.Text)).ToArray());
    }

    [Fact]
    public void TrailingTextAfterTheLastSpanStaysDefault()
    {
        List<SyntaxHighlighter.NativeSpan> spans = [new(0, 2, TokenKind.Comment)];

        var tokens = SyntaxHighlighter.BuildTokens("// hi", spans);

        Assert.Equal("// hi", string.Concat(tokens.Select(token => token.Text)));
        Assert.Equal(2, tokens.Count);
        Assert.Equal(TokenKind.Comment, tokens[0].Kind);
        Assert.Equal(TokenKind.Default, tokens[1].Kind);
    }

    [Fact]
    public void OverlappingOrOutOfRangeSpansDegradeToOneDefaultToken()
    {
        List<SyntaxHighlighter.NativeSpan> overlapping = [new(0, 3, TokenKind.Keyword), new(2, 2, TokenKind.Number)];
        List<SyntaxHighlighter.NativeSpan> outOfRange = [new(0, 99, TokenKind.Keyword)];

        var overlappingTokens = SyntaxHighlighter.BuildTokens("let x", overlapping);
        var outOfRangeTokens = SyntaxHighlighter.BuildTokens("let x", outOfRange);

        Assert.Equal(new[] { (TokenKind.Default, "let x") }, overlappingTokens.Select(t => (t.Kind, t.Text)).ToArray());
        Assert.Equal(new[] { (TokenKind.Default, "let x") }, outOfRangeTokens.Select(t => (t.Kind, t.Text)).ToArray());
    }

    [Fact]
    public void EmptyInputProducesNoTokensWithoutCallingNative()
    {
        Assert.Empty(SyntaxHighlighter.Highlight("", "rust"));
    }

    [Theory]
    [InlineData("let value = 42; // done", "rust")]
    [InlineData("{\"a\": 1}", "json")]
    [InlineData("<div class=\"a\">x</div>", "html")]
    [InlineData("a,\"b,c\",d", "csv")]
    public void HighlightPreservesTheExactInputTextRegardlessOfNativeAvailability(string code, string language)
    {
        var tokens = SyntaxHighlighter.Highlight(code, language);

        Assert.True(tokens.Count > 0);
        Assert.Equal(code, string.Concat(tokens.Select(token => token.Text)));
        Assert.All(tokens, token => Assert.True(token.Text.Length > 0));
    }

    [Theory]
    [InlineData("cs", "csharp")]
    [InlineData(".TS", "typescript")]
    [InlineData("  PS1 ", "powershell")]
    [InlineData("yml", "yaml")]
    [InlineData("csproj", "xml")]
    [InlineData("cxx", "cpp")]
    [InlineData("plaintext", "plaintext")]
    public void NormalizeLanguageMapsExtensionsOntoTokenizerIds(string input, string expected)
    {
        Assert.Equal(expected, SyntaxHighlighter.NormalizeLanguage(input));
    }
}
