using System.Text;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class UpdateMetadataTests
{
    [Theory]
    [InlineData("0.2.17", "0.2.18", -1)]
    [InlineData("0.2.18", "0.2.18", 0)]
    [InlineData("0.2.19", "0.2.18", 1)]
    [InlineData("0.2.18-beta.2", "0.2.18-beta.10", -1)]
    [InlineData("0.2.18-beta.10", "0.2.18", -1)]
    [InlineData("0.2.18+build.4", "0.2.18", 0)]
    public void Release_versions_follow_semantic_ordering(string leftText, string rightText, int expected)
    {
        Assert.True(ReleaseVersion.TryParse(leftText, out ReleaseVersion? left));
        Assert.True(ReleaseVersion.TryParse(rightText, out ReleaseVersion? right));
        Assert.Equal(expected, Math.Sign(left!.CompareTo(right)));
    }

    [Theory]
    [InlineData("")]
    [InlineData("v0.2.18")]
    [InlineData("0.2")]
    [InlineData("01.2.3")]
    [InlineData("0.2.18-")]
    public void Release_versions_reject_unsupported_forms(string value)
        => Assert.False(ReleaseVersion.TryParse(value, out _));

    [Fact]
    public void Stable_update_metadata_requires_consistent_github_asset_identity()
    {
        UpdateMetadata metadata = UpdateMetadata.Parse(Encoding.UTF8.GetBytes(ValidJson));
        Assert.Equal("0.2.18", metadata.VersionText);
        Assert.Equal(17763, metadata.MinimumWindowsBuild);
    }

    [Theory]
    [InlineData("\"schemaVersion\": 1", "\"schemaVersion\": 2")]
    [InlineData("\"channel\": \"stable\"", "\"channel\": \"beta\"")]
    [InlineData("\"architecture\": \"x64\"", "\"architecture\": \"arm64\"")]
    [InlineData("\"tag\": \"v0.2.18\"", "\"tag\": \"v0.2.17\"")]
    [InlineData("github.com/SherlockChiang/QuickLook.Next", "example.com/SherlockChiang/QuickLook.Next")]
    [InlineData("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "invalid")]
    public void Stable_update_metadata_rejects_inconsistent_or_untrusted_fields(string from, string to)
        => Assert.ThrowsAny<Exception>(() => UpdateMetadata.Parse(Encoding.UTF8.GetBytes(ValidJson.Replace(from, to))));

    private const string ValidJson = """
        {
          "schemaVersion": 1,
          "version": "0.2.18",
          "tag": "v0.2.18",
          "channel": "stable",
          "architecture": "x64",
          "minimumWindowsBuild": 17763,
          "publishedAt": "2026-07-29T00:00:00Z",
          "downloadUrl": "https://github.com/SherlockChiang/QuickLook.Next/releases/download/v0.2.18/QuickLook.Next-Installer-0.2.18-win-x64.zip",
          "file": "QuickLook.Next-Installer-0.2.18-win-x64.zip",
          "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }
        """;
}
