using QuickLook.Next.Contracts;

namespace QuickLook.Next.Core;

public static class ListingFilter
{
    public const int MaxItems = 5000;

    public static IReadOnlyList<PreviewListingItem> CurrentLevel(
        IReadOnlyList<PreviewListingItem> items,
        string parentPath,
        string query)
    {
        query = query.Trim();
        parentPath = NormalizePath(parentPath);
        return items
            .Take(MaxItems)
            .Where(item => string.Equals(NormalizePath(item.ParentPath), parentPath, StringComparison.OrdinalIgnoreCase))
            .Where(item => query.Length == 0 || item.Name.Contains(query, StringComparison.OrdinalIgnoreCase))
            .ToArray();
    }

    private static string NormalizePath(string path)
    {
        path = path.Replace('\\', '/').TrimStart('/');
        return path.Length == 0 || path.EndsWith('/') ? path : path + "/";
    }
}
