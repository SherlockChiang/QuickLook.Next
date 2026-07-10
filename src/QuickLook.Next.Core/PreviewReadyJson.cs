using System.Text.Json;
using QuickLook.Next.Contracts;

namespace QuickLook.Next.Core;

public static class PreviewReadyJson
{
    public static bool TryParse(string requestId, string json, out PreviewReady? preview, out string? error)
    {
        preview = null;
        error = null;
        try
        {
            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            string kind = root.GetProperty("kind").GetString() ?? "unknown";
            string title = root.GetProperty("title").GetString() ?? kind;
            double width = kind is "archive" or "folder" or "package" or "table" ? 760 : 720;
            double height = kind is "archive" or "folder" or "package" or "table" ? 560 : 500;
            var ready = new PreviewReady(requestId, kind, title, width, height);

            if (root.TryGetProperty("table", out var table))
            {
                preview = ready with { Table = JsonSerializer.Deserialize<PreviewTable>(table.GetRawText(), ProtocolJson.Options) };
                return true;
            }
            if (root.TryGetProperty("listing", out var listing))
            {
                preview = ready with { Listing = JsonSerializer.Deserialize<PreviewListing>(listing.GetRawText(), ProtocolJson.Options) };
                return true;
            }

            OfficeLayout? officeLayout = root.TryGetProperty("officeLayout", out var layout)
                ? JsonSerializer.Deserialize<OfficeLayout>(layout.GetRawText(), ProtocolJson.Options)
                : null;
            PreviewMarkdown? markdown = root.TryGetProperty("markdown", out var markdownElement)
                ? JsonSerializer.Deserialize<PreviewMarkdown>(markdownElement.GetRawText(), ProtocolJson.Options)
                : null;

            preview = root.TryGetProperty("text", out var text)
                ? ready with
                {
                    TextContent = text.GetString(),
                    TextFormat = root.TryGetProperty("format", out var format) ? format.GetString() : "plain",
                    TextLanguage = root.TryGetProperty("language", out var language) ? language.GetString() : "text",
                    OfficeLayout = officeLayout,
                    Markdown = markdown,
                }
                : markdown is not null ? ready with { Markdown = markdown }
                : officeLayout is not null ? ready with { OfficeLayout = officeLayout }
                : null;
            if (preview is null)
                error = "Native preview result contained no supported content.";
            return preview is not null;
        }
        catch (Exception ex)
        {
            error = ex.Message;
            return false;
        }
    }
}
