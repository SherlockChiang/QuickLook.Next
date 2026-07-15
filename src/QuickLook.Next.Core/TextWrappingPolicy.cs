namespace QuickLook.Next.Core;

public static class TextWrappingPolicy
{
    public static bool ShouldWrap(string mode, string? format, bool structuredMarkdown = false)
    {
        if (structuredMarkdown || string.Equals(format, "markdown", StringComparison.OrdinalIgnoreCase))
            return true;
        return mode switch
        {
            "always" => true,
            "never" => false,
            _ => string.Equals(format, "plain", StringComparison.OrdinalIgnoreCase),
        };
    }
}
