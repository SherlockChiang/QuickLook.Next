using System.Buffers;
using System.Runtime.InteropServices;
using System.Text;

namespace QuickLook.Next.App;

public enum TokenKind { Default, Keyword, Str, Comment, Number, Type, Property, Punctuation }

/// <summary>
/// Token spans come from the native tokenizer (<c>ql_highlight_spans</c>), which reports UTF-16
/// offsets so this adapter slices the caller's original string directly. Token text plus the
/// inter-span gaps therefore always concatenates back to the input, so selection/copy remains
/// exact. If the native call is unavailable the whole input degrades to one Default token.
/// </summary>
internal static class SyntaxHighlighter
{
    private const string Dll = "quicklook_next_native";
    private const int MaxNativeHighlightSpans = 16384;
    private const int MaxSpanPacketBytes = 4 + MaxNativeHighlightSpans * 12;
    private const int InitialSpanPacketBytes = 16 * 1024;
    private static bool _nativeUnavailable;

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_highlight_spans(
        byte[] textUtf8,
        nuint textLen,
        byte[] langUtf8,
        nuint langLen,
        byte[] outBuf,
        nuint outCap);

    internal readonly record struct NativeSpan(int Start, int Length, TokenKind Kind);

    public static string NormalizeLanguage(string language)
    {
        string value = language.Trim().TrimStart('.').ToLowerInvariant();
        return value switch
        {
            "cs" => "csharp",
            "js" or "mjs" or "cjs" or "jsx" => "javascript",
            "ts" or "tsx" => "typescript",
            "ps1" => "powershell",
            "sh" or "bash" or "zsh" => "shell",
            "cmd" or "bat" => "batch",
            "yml" => "yaml",
            "htm" => "html",
            "xhtml" => "html",
            "csproj" or "props" or "targets" or "config" or "resx" or "xsd" => "xml",
            "cxx" or "cc" or "hpp" or "hxx" => "cpp",
            "kt" or "kts" => "kotlin",
            "rb" => "ruby",
            "pl" => "perl",
            "fs" or "fsx" => "fsharp",
            "dockerfile" => "dockerfile",
            "makefile" => "makefile",
            _ => value,
        };
    }

    public static List<(string Text, TokenKind Kind)> Highlight(string code, string language)
    {
        language = NormalizeLanguage(language);
        List<NativeSpan>? spans = code.Length == 0
            ? []
            : TryGetNativeSpans(code, language);
        return spans is null ? [(code, TokenKind.Default)] : BuildTokens(code, spans);
    }

    internal static List<(string Text, TokenKind Kind)> BuildTokens(string code, List<NativeSpan> spans)
    {
        var tokens = new List<(string, TokenKind)>(spans.Count + 1);
        int position = 0;
        foreach (NativeSpan span in spans)
        {
            if (span.Start < position || span.Start > code.Length || span.Length < 0
                || span.Start + span.Length > code.Length)
            {
                return [(code, TokenKind.Default)];
            }
            if (span.Start > position)
                tokens.Add((code[position..span.Start], TokenKind.Default));
            if (span.Length > 0)
                tokens.Add((code.Substring(span.Start, span.Length), span.Kind));
            position = span.Start + span.Length;
        }
        if (position < code.Length)
            tokens.Add((code[position..], TokenKind.Default));
        return tokens;
    }

    private static List<NativeSpan>? TryGetNativeSpans(string code, string language)
    {
        if (_nativeUnavailable)
            return null;
        try
        {
            byte[] textBytes = Encoding.UTF8.GetBytes(code);
            byte[] langBytes = Encoding.UTF8.GetBytes(language);
            int cap = InitialSpanPacketBytes;
            while (cap <= MaxSpanPacketBytes)
            {
                byte[] outBuf = ArrayPool<byte>.Shared.Rent(cap);
                try
                {
                    int n = ql_highlight_spans(
                        textBytes,
                        (nuint)textBytes.Length,
                        langBytes,
                        (nuint)langBytes.Length,
                        outBuf,
                        (nuint)outBuf.Length);
                    if (n > 0)
                        return ParseSpanPacket(outBuf, n);
                    if (n < 0)
                    {
                        int needed = -n;
                        if (needed <= cap || needed > MaxSpanPacketBytes)
                            return null;
                        cap = needed;
                        continue;
                    }
                    return null;
                }
                finally
                {
                    ArrayPool<byte>.Shared.Return(outBuf);
                }
            }
            return null;
        }
        catch (Exception ex) when (ex is DllNotFoundException or EntryPointNotFoundException)
        {
            _nativeUnavailable = true;
            return null;
        }
        catch
        {
            return null;
        }
    }

    private static List<NativeSpan>? ParseSpanPacket(byte[] buffer, int length)
    {
        if (length < 4)
            return null;
        int count = BitConverter.ToInt32(buffer, 0);
        if (count is < 0 or > MaxNativeHighlightSpans || length != 4 + checked(count * 12))
            return null;

        var spans = new List<NativeSpan>(count);
        for (int i = 0; i < count; i++)
        {
            int offset = 4 + i * 12;
            int start = BitConverter.ToInt32(buffer, offset);
            int length16 = BitConverter.ToInt32(buffer, offset + 4);
            int kind = BitConverter.ToInt32(buffer, offset + 8);
            if (start < 0 || length16 < 0 || kind is < 0 or > (int)TokenKind.Punctuation)
                return null;
            spans.Add(new NativeSpan(start, length16, (TokenKind)kind));
        }
        return spans;
    }
}
