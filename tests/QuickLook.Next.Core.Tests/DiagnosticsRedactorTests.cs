using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class DiagnosticsRedactorTests
{
    [Theory]
    [InlineData(@"open path=C:\Users\alice\Client\report.pdf; size=42", "open path=<path:report.pdf>; size=42")]
    [InlineData(@"open \\server\private\photo.jpg", "open <path:photo.jpg>")]
    [InlineData("no path here", "no path here")]
    public void RedactPaths_RemovesDirectoryInformation(string message, string expected)
        => Assert.Equal(expected, DiagnosticsRedactor.RedactPaths(message));

    [Fact]
    public void RedactPaths_RedactsMultiplePaths()
    {
        const string message = @"from C:\Secret\one.txt; to D:\Other\two.txt";

        Assert.Equal("from <path:one.txt>; to <path:two.txt>", DiagnosticsRedactor.RedactPaths(message));
    }
}
