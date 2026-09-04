namespace QuickLook.Next.App;

internal readonly record struct OfficeCoverage(
    int RepresentedPages,
    int TotalPages,
    int PageLimit,
    bool IsPartial);

internal static class OfficeCoveragePolicy
{
    public static OfficeCoverage Evaluate(
        int availablePages,
        int reportedRepresentedPages,
        int reportedTotalPages,
        int reportedPageLimit,
        int presentationPageLimit)
    {
        int boundedAvailablePages = Math.Max(0, availablePages);
        int boundedPresentationLimit = Math.Max(1, presentationPageLimit);
        int representedPages = Math.Min(boundedAvailablePages, boundedPresentationLimit);
        int sourceRepresentedPages = reportedRepresentedPages > 0
            ? Math.Clamp(reportedRepresentedPages, representedPages, boundedAvailablePages)
            : boundedAvailablePages;
        int totalPages = reportedTotalPages > 0
            ? Math.Max(reportedTotalPages, sourceRepresentedPages)
            : sourceRepresentedPages;
        totalPages = Math.Max(totalPages, representedPages);

        int sourcePageLimit = reportedPageLimit > 0
            ? Math.Max(reportedPageLimit, representedPages)
            : boundedPresentationLimit;
        int pageLimit = Math.Min(sourcePageLimit, boundedPresentationLimit);
        return new(
            representedPages,
            totalPages,
            pageLimit,
            representedPages < totalPages);
    }
}
