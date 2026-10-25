using System.Text.Json;

namespace QuickLook.Next.Core;

public sealed record UpdateMetadata(
    ReleaseVersion Version,
    string VersionText,
    int MinimumWindowsBuild,
    DateTimeOffset PublishedAt)
{
    private const string RepositoryPrefix = "https://github.com/SherlockChiang/QuickLook.Next/releases/download/";

    public static UpdateMetadata Parse(ReadOnlyMemory<byte> utf8)
    {
        try
        {
            return ParseDocument(utf8);
        }
        catch (JsonException ex)
        {
            throw new FormatException("Invalid update metadata JSON.", ex);
        }
    }

    private static UpdateMetadata ParseDocument(ReadOnlyMemory<byte> utf8)
    {
        using JsonDocument document = JsonDocument.Parse(utf8, new JsonDocumentOptions { MaxDepth = 8 });
        JsonElement root = document.RootElement;
        if (root.ValueKind != JsonValueKind.Object)
            throw new FormatException("Update metadata must be an object.");
        int schema = RequiredInt32(root, "schemaVersion");
        string versionText = RequiredString(root, "version", 128);
        string tag = RequiredString(root, "tag", 130);
        string channel = RequiredString(root, "channel", 16);
        string architecture = RequiredString(root, "architecture", 16);
        int minimumWindowsBuild = RequiredInt32(root, "minimumWindowsBuild");
        string publishedAtText = RequiredString(root, "publishedAt", 64);
        string file = RequiredString(root, "file", 180);
        string sha256 = RequiredString(root, "sha256", 64);
        string downloadUrl = RequiredString(root, "downloadUrl", 512);
        if (schema != 1 || channel != "stable" || architecture != "x64" || minimumWindowsBuild <= 0)
            throw new FormatException("Unsupported update metadata.");
        if (!ReleaseVersion.TryParse(versionText, out ReleaseVersion? version)
            || tag != "v" + versionText
            || file != $"QuickLook.Next-Installer-{versionText}-win-x64.zip"
            || sha256.Length != 64
            || sha256.Any(static character => !Uri.IsHexDigit(character))
            || !DateTimeOffset.TryParse(publishedAtText, out DateTimeOffset publishedAt)
            || !Uri.TryCreate(downloadUrl, UriKind.Absolute, out Uri? uri)
            || uri.Scheme != Uri.UriSchemeHttps
            || !downloadUrl.Equals($"{RepositoryPrefix}{tag}/{file}", StringComparison.Ordinal))
            throw new FormatException("Invalid update metadata.");
        return new UpdateMetadata(version!, versionText, minimumWindowsBuild, publishedAt);
    }

    private static string RequiredString(JsonElement root, string name, int maxLength)
    {
        if (!root.TryGetProperty(name, out JsonElement property) || property.ValueKind != JsonValueKind.String)
            throw new FormatException($"Invalid {name}.");
        string? value = property.GetString();
        if (string.IsNullOrWhiteSpace(value) || value.Length > maxLength)
            throw new FormatException($"Invalid {name}.");
        return value;
    }

    private static int RequiredInt32(JsonElement root, string name)
    {
        if (!root.TryGetProperty(name, out JsonElement property)
            || property.ValueKind != JsonValueKind.Number
            || !property.TryGetInt32(out int value))
            throw new FormatException($"Invalid {name}.");
        return value;
    }
}
