namespace QuickLook.Next.App;

internal static class ImageFilmstripPlanner
{
    public static IEnumerable<string> AdjacentPaths(string[] siblings, string currentPath, int radius)
    {
        int current = Array.FindIndex(siblings, p => string.Equals(p, currentPath, StringComparison.OrdinalIgnoreCase));
        if (current < 0)
            yield break;

        for (int distance = 1; distance <= radius; distance++)
        {
            int next = current + distance;
            if (next < siblings.Length)
                yield return siblings[next];

            int previous = current - distance;
            if (previous >= 0)
                yield return siblings[previous];
        }
    }

    public static IEnumerable<(string Path, int Distance)> PrioritizeWithDistance(string[] siblings, string currentPath)
    {
        int current = Array.FindIndex(siblings, p => string.Equals(p, currentPath, StringComparison.OrdinalIgnoreCase));
        if (current < 0)
            return siblings.Select((path, index) => (Path: path, Distance: index));

        return siblings
            .Select((path, index) => (Path: path, Distance: Math.Abs(index - current)))
            .OrderBy(i => i.Distance)
            .ThenBy(i => i.Path, StringComparer.CurrentCultureIgnoreCase);
    }
}
