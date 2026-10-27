namespace QuickLook.Next.Core;

public static class TextPreviewPolicy
{
    // Keep ordinary XAML text layout bounded even when the parser returns a larger
    // source payload. The parser and fallback reader retain their own byte limits;
    // this is the presentation boundary.
    public const int MaxDisplayedCharacters = 128 * 1024;

    public static string TrimForDisplay(string text, string truncationMarker)
    {
        ArgumentNullException.ThrowIfNull(text);
        ArgumentNullException.ThrowIfNull(truncationMarker);

        if (text.Length <= MaxDisplayedCharacters)
            return text;

        return text[..MaxDisplayedCharacters] + "\n\n" + truncationMarker;
    }
}
