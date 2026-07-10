using System.Text;
using QuickLook.Next.Contracts;

namespace QuickLook.Next.Core;

public static class FallbackFileProbe
{
    private const int MaxTextPreviewBytes = 512 * 1024;
    private const string Windows1252Controls = "€\u0081‚ƒ„…†‡ˆ‰Š‹Œ\u008DŽ\u008F\u0090‘’“”•–—˜™š›œ\u009DžŸ";
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

    public static bool IsText(string path, ReadOnlySpan<byte> prefix, bool isEmptyFile = false)
    {
        if (HasKnownBinarySignature(prefix))
            return false;
        if (TextExtensions.Contains(Path.GetExtension(path)) || TextFileNames.Contains(Path.GetFileName(path)))
            return true;
        if (isEmptyFile)
            return true;
        if (prefix.IsEmpty)
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

    public static PreviewReady? TryCreateTextPreview(string requestId, string path, CancellationToken cancellationToken)
    {
        try
        {
            using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
            int length = checked((int)Math.Min(stream.Length, MaxTextPreviewBytes + 1L));
            byte[] bytes = new byte[length];
            stream.ReadExactly(bytes);
            cancellationToken.ThrowIfCancellationRequested();

            bool truncated = bytes.Length > MaxTextPreviewBytes;
            if (truncated)
                Array.Resize(ref bytes, MaxTextPreviewBytes);
            string text = DecodeText(bytes);
            if (truncated)
                text += $"\n\n[Preview truncated at {MaxTextPreviewBytes} bytes]";

            (string format, string language) = GetTextFormat(path);
            return new PreviewReady(requestId, "text", Path.GetFileName(path), 820, 620)
            {
                TextContent = text,
                TextFormat = format,
                TextLanguage = language,
            };
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or NotSupportedException or DecoderFallbackException)
        {
            return null;
        }
    }

    private static string DecodeText(ReadOnlySpan<byte> bytes)
    {
        if (bytes.StartsWith(Encoding.UTF8.Preamble))
            return Encoding.UTF8.GetString(bytes[Encoding.UTF8.Preamble.Length..]);
        if (bytes.Length >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE)
            return Encoding.Unicode.GetString(bytes[2..]);
        if (bytes.Length >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF)
            return Encoding.BigEndianUnicode.GetString(bytes[2..]);
        try
        {
            return new UTF8Encoding(false, true).GetString(bytes);
        }
        catch (DecoderFallbackException)
        {
            return DecodeWindows1252(bytes);
        }
    }

    private static string DecodeWindows1252(ReadOnlySpan<byte> bytes)
    {
        return string.Create(bytes.Length, bytes.ToArray(), (chars, source) =>
        {
            for (int i = 0; i < source.Length; i++)
                chars[i] = source[i] is >= 0x80 and <= 0x9F ? Windows1252Controls[source[i] - 0x80] : (char)source[i];
        });
    }

    private static (string Format, string Language) GetTextFormat(string path)
    {
        string fileName = Path.GetFileName(path);
        if (fileName.Equals(".editorconfig", StringComparison.OrdinalIgnoreCase)) return ("code", "ini");
        if (fileName.Equals(".env", StringComparison.OrdinalIgnoreCase)) return ("code", "env");
        string extension = Path.GetExtension(path).ToLowerInvariant();
        return extension switch
        {
            ".md" or ".markdown" => ("markdown", "markdown"),
            ".json" => ("code", "json"),
            ".xml" or ".xaml" or ".xsd" or ".resx" or ".config" or ".manifest" or ".policy" or ".settings" or ".csproj" or ".props" or ".targets" => ("code", "xml"),
            ".ini" or ".cfg" or ".conf" or ".cnf" or ".inf" or ".url" or ".desktop" or ".service" or ".reg" => ("code", "ini"),
            ".rdp" or ".rc" or ".prefs" or ".properties" => ("code", "properties"),
            ".yml" or ".yaml" => ("code", "yaml"),
            ".toml" => ("code", "toml"),
            ".ps1" => ("code", "powershell"),
            ".bat" or ".cmd" => ("code", "batch"),
            _ => ("plain", "text"),
        };
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
        try
        {
            var encoding = new UnicodeEncoding(!littleEndian, false, true);
            return IsPrintable(encoding.GetString(bytes));
        }
        catch (DecoderFallbackException)
        {
            return false;
        }
    }

    private static bool IsPrintable(string text)
    {
        if (text.Length == 0)
            return false;
        int printable = text.Count(static ch => ch is '\t' or '\r' or '\n' || !char.IsControl(ch));
        return printable * 100 / text.Length >= 90;
    }
}
