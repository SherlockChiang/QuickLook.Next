using System.Text.RegularExpressions;

namespace QuickLook.Next.Core;

public static partial class DiagnosticsRedactor
{
    public static string RedactPaths(string message)
        => WindowsPath().Replace(message, static match =>
        {
            string path = match.Value.TrimEnd(' ', '.', ',', ')', ']', '}');
            string suffix = match.Value[path.Length..];
            string name = Path.GetFileName(path.TrimEnd('\\'));
            return $"<path:{(name.Length == 0 ? "redacted" : name)}>{suffix}";
        });

    [GeneratedRegex(@"(?<![A-Za-z0-9_])(?:[A-Za-z]:\\|\\\\)[^;\r\n]+", RegexOptions.CultureInvariant)]
    private static partial Regex WindowsPath();
}
