using System.Text;

namespace QuickLook.Next.Core;

public static class FallbackFileProbe
{
    private static readonly byte[] ZipSignature = [0x50, 0x4B, 0x03, 0x04];
    private static readonly byte[] PngSignature = [0x89, 0x50, 0x4E, 0x47];
    private static readonly byte[] JpegSignature = [0xFF, 0xD8, 0xFF];
    private static readonly byte[] ElfSignature = [0x7F, (byte)'E', (byte)'L', (byte)'F'];

    private static readonly HashSet<string> TextExtensions = new(StringComparer.OrdinalIgnoreCase)
    {
        ".txt", ".md", ".markdown", ".log", ".csv", ".tsv", ".env", ".json", ".xml", ".xaml",
        ".xsd", ".resx", ".config", ".manifest", ".policy", ".settings", ".ini", ".cfg", ".conf",
        ".cnf", ".inf", ".url", ".desktop", ".service", ".reg", ".rdp", ".rc", ".prefs",
        ".properties", ".yml", ".yaml", ".toml", ".bat", ".cmd", ".ps1", ".sh", ".bash", ".zsh",
        ".cs", ".csproj", ".sln", ".props", ".targets", ".rs", ".js", ".jsx", ".mjs", ".cjs",
        ".ts", ".tsx", ".css", ".scss", ".sass", ".less", ".html", ".htm", ".py", ".c", ".h",
        ".cc", ".cpp", ".cxx", ".hpp", ".hxx", ".java", ".go", ".php", ".rb", ".pl", ".swift",
        ".kt", ".kts", ".sql", ".lua", ".fs", ".fsx", ".vb", ".dart", ".scala", ".r", ".dockerfile",
    };

    private static readonly HashSet<string> TextFileNames = new(StringComparer.OrdinalIgnoreCase)
    {
        "Dockerfile", "Containerfile", "Makefile", "CMakeLists.txt", ".editorconfig", ".gitignore",
        ".gitattributes", ".dockerignore", ".env",
    };

    public static bool IsText(string path, ReadOnlySpan<byte> prefix)
    {
        if (TextExtensions.Contains(Path.GetExtension(path)) || TextFileNames.Contains(Path.GetFileName(path)))
            return true;
        if (prefix.IsEmpty)
            return false;
        if (HasKnownBinarySignature(prefix))
            return false;
        if (prefix.Length >= 2 && prefix[0] == 0xFF && prefix[1] == 0xFE)
            return IsPrintableUtf16(prefix[2..], littleEndian: true);
        if (prefix.Length >= 2 && prefix[0] == 0xFE && prefix[1] == 0xFF)
            return IsPrintableUtf16(prefix[2..], littleEndian: false);
        if (prefix.Contains((byte)0))
            return false;

        try
        {
            string text = new UTF8Encoding(false, true).GetString(prefix);
            return IsPrintable(text);
        }
        catch (DecoderFallbackException)
        {
            return false;
        }
    }

    private static bool HasKnownBinarySignature(ReadOnlySpan<byte> prefix)
        => prefix.StartsWith("MZ"u8)
           || prefix.StartsWith("%PDF"u8)
           || prefix.StartsWith(ZipSignature)
           || prefix.StartsWith(PngSignature)
           || prefix.StartsWith(JpegSignature)
           || prefix.StartsWith("GIF8"u8)
           || prefix.StartsWith("SQLite format 3\0"u8)
           || prefix.StartsWith(ElfSignature);

    private static bool IsPrintableUtf16(ReadOnlySpan<byte> bytes, bool littleEndian)
    {
        if (bytes.Length < 2 || bytes.Length % 2 != 0)
            return false;
        char[] chars = new char[bytes.Length / 2];
        for (int i = 0; i < chars.Length; i++)
        {
            int offset = i * 2;
            chars[i] = (char)(littleEndian
                ? bytes[offset] | bytes[offset + 1] << 8
                : bytes[offset] << 8 | bytes[offset + 1]);
        }
        return IsPrintable(new string(chars));
    }

    private static bool IsPrintable(string text)
    {
        if (text.Length == 0)
            return false;
        int printable = text.Count(static ch => ch is '\t' or '\r' or '\n' || !char.IsControl(ch));
        return printable * 100 / text.Length >= 90;
    }
}
