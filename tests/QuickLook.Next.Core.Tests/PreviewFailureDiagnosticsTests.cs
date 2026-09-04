using System.Text.Json;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class PreviewFailureDiagnosticsTests
{
    [Fact]
    public void Create_PreservesOnlyBoundedPathFreeFacts()
    {
        PreviewFailureDiagnostic diagnostic = PreviewFailureDiagnostics.Create(
            PreviewErrorCodes.ImageCodecRequired,
            "route-selected",
            42,
            "0.3.7",
            null,
            ".HEIC",
            "image",
            20 * 1024 * 1024);

        Assert.Equal(1, diagnostic.SchemaVersion);
        Assert.Equal(PreviewErrorCodes.ImageCodecRequired, diagnostic.ErrorCode);
        Assert.Equal("route-selected", diagnostic.Phase);
        Assert.Equal(42, diagnostic.CorrelationId);
        Assert.Equal("0.3.7", diagnostic.Version);
        Assert.Equal("heic", diagnostic.Format);
        Assert.Equal("16-64-mib", diagnostic.SizeBucket);
    }

    [Fact]
    public void Create_RejectsHostileTokensWithoutAcceptingAPath()
    {
        PreviewFailureDiagnostic diagnostic = PreviewFailureDiagnostics.Create(
            @"C:\Users\alice\secret.txt",
            "route\r\npath=secret",
            -1,
            "version with spaces",
            @"C:\Private\photo.png",
            "..\\secret",
            "image/path",
            -1);
        string payload = PreviewFailureDiagnostics.Serialize(diagnostic);

        Assert.Equal(PreviewErrorCodes.Unknown, diagnostic.ErrorCode);
        Assert.Equal("unknown", diagnostic.Phase);
        Assert.Equal(0, diagnostic.CorrelationId);
        Assert.Equal("unknown", diagnostic.Version);
        Assert.Equal("unknown", diagnostic.Format);
        Assert.Equal("unknown", diagnostic.SizeBucket);
        Assert.DoesNotContain("alice", payload, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("secret", payload, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("private", payload, StringComparison.OrdinalIgnoreCase);
        Assert.InRange(payload.Length, 1, PreviewFailureDiagnostics.MaxClipboardChars);
    }

    [Fact]
    public void Serialize_UsesAStableMinimalSchema()
    {
        PreviewFailureDiagnostic diagnostic = PreviewFailureDiagnostics.Create(
            PreviewErrorCodes.TimedOut,
            "probe-complete",
            7,
            "0.3.7",
            "pdf",
            null,
            null,
            3 * 1024 * 1024);

        using JsonDocument json = JsonDocument.Parse(PreviewFailureDiagnostics.Serialize(diagnostic));
        string[] properties = json.RootElement.EnumerateObject().Select(property => property.Name).ToArray();

        Assert.Equal(
            ["schemaVersion", "errorCode", "phase", "correlationId", "version", "format", "sizeBucket"],
            properties);
        Assert.Equal("1-16-mib", json.RootElement.GetProperty("sizeBucket").GetString());
    }

    [Theory]
    [InlineData(-1, "unknown")]
    [InlineData(0, "empty")]
    [InlineData(1, "lt-1-mib")]
    [InlineData(1024 * 1024, "1-16-mib")]
    [InlineData(16 * 1024 * 1024, "16-64-mib")]
    [InlineData(64 * 1024 * 1024, "64-256-mib")]
    [InlineData(256 * 1024 * 1024, "gte-256-mib")]
    public void GetSizeBucket_UsesCoarseStableBoundaries(long sizeBytes, string expected)
        => Assert.Equal(expected, PreviewFailureDiagnostics.GetSizeBucket(sizeBytes));
}
