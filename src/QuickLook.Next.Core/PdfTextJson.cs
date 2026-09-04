using System.Text;
using System.Text.Json;

namespace QuickLook.Next.Core;

public static class PdfTextJson
{
    public const int MaxPages = 2_048;
    public const int MaxSourcePages = 250_000;
    public const int MaxPageTextBytes = 64 * 1024;
    public const int MaxTotalTextBytes = 1024 * 1024;

    public static bool TryParse(string json, out PdfTextDocument? document, out string? error)
    {
        document = null;
        error = null;
        try
        {
            PdfTextDocument? parsed = JsonSerializer.Deserialize<PdfTextDocument>(json, ProtocolJson.Options);
            if (parsed is null || !IsValid(parsed))
            {
                error = "Native PDF text result violated its bounded contract.";
                return false;
            }
            document = parsed;
            return true;
        }
        catch (Exception ex) when (ex is JsonException or NotSupportedException or ArgumentException)
        {
            error = ex.Message;
            return false;
        }
    }

    private static bool IsValid(PdfTextDocument document)
    {
        if (document.Status is not ("ready" or "partial" or "noText" or "encrypted")
            || document.PageCount is < 0 or > MaxSourcePages
            || document.Pages is null
            || document.Pages.Length > MaxPages
            || document.FailedPages < 0
            || document.FailedPages > document.Pages.Length)
        {
            return false;
        }
        if (document.Status == "encrypted")
            return document.PageCount == 0 && document.Pages.Length == 0 && document.FailedPages == 0;

        int lastPageIndex = -1;
        int totalBytes = 0;
        foreach (PdfTextPage page in document.Pages)
        {
            if (page.PageIndex <= lastPageIndex
                || page.PageIndex < 0
                || page.PageIndex >= document.PageCount
                || page.Text is null)
            {
                return false;
            }
            int pageBytes = Encoding.UTF8.GetByteCount(page.Text);
            if (pageBytes > MaxPageTextBytes
                || totalBytes > MaxTotalTextBytes - pageBytes)
            {
                return false;
            }
            totalBytes += pageBytes;
            lastPageIndex = page.PageIndex;
        }
        if (!document.Truncated && document.Pages.Length != document.PageCount)
            return false;
        return true;
    }
}
