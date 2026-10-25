using System.Text;
using System.Text.Json.Nodes;
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
        => Assert.Throws<FormatException>(() => UpdateMetadata.Parse(Encoding.UTF8.GetBytes(ValidJson.Replace(from, to))));

    [Theory]
    [InlineData("")]
    [InlineData("{")]
    [InlineData("{}")]
    [InlineData("[]")]
    [InlineData("null")]
    [InlineData("1")]
    [InlineData("\"metadata\"")]
    public void Invalid_documents_use_the_callers_format_failure_contract(string json)
        => Assert.Throws<FormatException>(() => UpdateMetadata.Parse(Encoding.UTF8.GetBytes(json)));

    [Fact]
    public void Every_required_field_rejects_missing_null_and_wrong_types_as_format_failures()
    {
        JsonObject valid = JsonNode.Parse(ValidJson)!.AsObject();
        foreach (string name in valid.Select(pair => pair.Key))
        {
            JsonObject missing = (JsonObject)valid.DeepClone();
            missing.Remove(name);
            AssertInvalid(missing);
            bool numeric = name is "schemaVersion" or "minimumWindowsBuild";
            foreach (string value in numeric
                ? new[] { "null", "\"1\"", "true", "[]", "{}", "1.5", "2147483648" }
                : new[] { "null", "1", "true", "[]", "{}" })
            {
                JsonObject malformed = (JsonObject)valid.DeepClone();
                malformed[name] = JsonNode.Parse(value);
                AssertInvalid(malformed);
            }
        }

        static void AssertInvalid(JsonObject document)
            => Assert.Throws<FormatException>(() => UpdateMetadata.Parse(Encoding.UTF8.GetBytes(document.ToJsonString())));
    }

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
