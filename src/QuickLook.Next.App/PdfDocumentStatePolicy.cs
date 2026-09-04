namespace QuickLook.Next.App;

internal enum PdfDocumentNoticeKind
{
    None,
    Empty,
    Partial,
    Failed,
}

internal readonly record struct PdfDocumentNotice(
    PdfDocumentNoticeKind Kind,
    int FailedPages,
    int PageCount);

internal static class PdfDocumentStatePolicy
{
    public static PdfDocumentNotice Evaluate(int pageCount, int failedPages)
    {
        int boundedPageCount = Math.Max(0, pageCount);
        if (boundedPageCount == 0)
            return new(PdfDocumentNoticeKind.Empty, 0, 0);

        int boundedFailedPages = Math.Clamp(failedPages, 0, boundedPageCount);
        PdfDocumentNoticeKind kind = boundedFailedPages switch
        {
            0 => PdfDocumentNoticeKind.None,
            _ when boundedFailedPages == boundedPageCount => PdfDocumentNoticeKind.Failed,
            _ => PdfDocumentNoticeKind.Partial,
        };
        return new(kind, boundedFailedPages, boundedPageCount);
    }
}
