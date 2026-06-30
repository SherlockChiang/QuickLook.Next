using System.Buffers;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

/// <summary>
/// In-process FFI to quicklook_next_native (the Rust cdylib). The native layer installs the keyboard
/// hook and reads the Explorer selection, then calls back with high-level intent lines, which we decode
/// into <see cref="NativeIntent"/>. (Validated in Spike 3.)
/// </summary>
internal sealed class NativeBridge
{
    private const string Dll = "quicklook_next_native";

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void NativeCallback(IntPtr utf16);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern void ql_set_callback(NativeCallback cb);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_start();
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern void ql_set_preview_visible(int visible);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern void ql_get_selection();
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_probe_file(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_text(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_archive(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_executable(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_torrent(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_folder(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_info(
        byte[] pathUtf8,
        nuint pathLen,
        byte[] kindUtf8,
        nuint kindLen,
        long size,
        long modifiedUnix,
        byte[] outBuf,
        nuint outCap);

    private delegate int NativePreviewCall(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    private const int MaxNativePreviewJsonBytes = 4 * 1024 * 1024;

    private NativeCallback? _callback; // keep alive: native stores the function pointer
    private Action<NativeIntent>? _onIntent;

    public void Start(Action<NativeIntent> onIntent)
    {
        _onIntent = onIntent;
        _callback = OnNative;
        ql_set_callback(_callback);
        ql_start();
    }

    public void SetPreviewVisible(bool visible)
    {
        try { ql_set_preview_visible(visible ? 1 : 0); }
        catch { /* ignore: stale native builds simply behave like the old hook */ }
    }

    private void OnNative(IntPtr utf16)
    {
        string? line = Marshal.PtrToStringUni(utf16);
        if (line is null) return;
        var intent = NativeIntent.TryParse(line);
        if (intent is not null) _onIntent?.Invoke(intent);
    }

    /// <summary>Native single-source-of-truth file probe (type/magic/metadata, cached). Null on failure.</summary>
    public FileProbe? ProbeFile(string path)
    {
        try
        {
            byte[] pathBytes = Encoding.UTF8.GetBytes(path);
            byte[] outBuf = ArrayPool<byte>.Shared.Rent(2048);
            try
            {
                int n = ql_probe_file(pathBytes, (nuint)pathBytes.Length, outBuf, (nuint)outBuf.Length);
                if (n <= 0) return null;

                using var doc = JsonDocument.Parse(new ReadOnlyMemory<byte>(outBuf, 0, n));
                var r = doc.RootElement;
                string magicHex = r.GetProperty("magicHex").GetString() ?? "";
                return new FileProbe(
                    r.GetProperty("path").GetString() ?? path,
                    r.GetProperty("extension").GetString() ?? "",
                    magicHex.Length > 0 ? Convert.FromHexString(magicHex) : [])
                {
                    Kind = r.GetProperty("kind").GetString() ?? "unknown",
                    Size = r.GetProperty("size").GetInt64(),
                    ModifiedUnix = r.GetProperty("modifiedUnix").GetInt64(),
                };
            }
            finally { ArrayPool<byte>.Shared.Return(outBuf); }
        }
        catch { return null; }
    }

    public PreviewReady? TryPreview(string requestId, string path, FileProbe probe)
    {
        NativePreviewCall? call = probe.Kind.ToLowerInvariant() switch
        {
            "text" => ql_preview_text,
            "archive" => ql_preview_archive,
            "package" => ql_preview_archive,
            "executable" => ql_preview_executable,
            "torrent" => ql_preview_torrent,
            "folder" => ql_preview_folder,
            _ => null,
        };

        string? json = call is not null
            ? CallPreview(call, path)
            : ShouldUseNativeInfo(probe) ? CallInfoPreview(path, probe) : null;
        return string.IsNullOrWhiteSpace(json) ? null : ParsePreviewReady(requestId, json);
    }

    public PreviewListing? TryPreviewFolderListing(string path)
    {
        string? json = CallPreview(ql_preview_folder, path);
        if (string.IsNullOrWhiteSpace(json))
            return null;

        try
        {
            using var doc = JsonDocument.Parse(json);
            return doc.RootElement.TryGetProperty("listing", out var listing)
                ? JsonSerializer.Deserialize<PreviewListing>(listing.GetRawText(), ProtocolJson.Options)
                : null;
        }
        catch
        {
            return null;
        }
    }

    private static string? CallPreview(NativePreviewCall call, string path)
    {
        try
        {
            byte[] pathBytes = Encoding.UTF8.GetBytes(path);
            int cap = 256 * 1024;
            while (cap <= MaxNativePreviewJsonBytes)
            {
                byte[] outBuf = ArrayPool<byte>.Shared.Rent(cap);
                try
                {
                    int n = call(pathBytes, (nuint)pathBytes.Length, outBuf, (nuint)outBuf.Length);
                    if (n > 0)
                        return Encoding.UTF8.GetString(outBuf, 0, n);
                    if (n < 0)
                    {
                        int needed = -n;
                        if (needed <= cap || needed > MaxNativePreviewJsonBytes)
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
        }
        catch
        {
            return null;
        }

        return null;
    }

    private static bool ShouldUseNativeInfo(FileProbe probe)
        => probe.Kind is "binary" or "unknown" or "office" or "disk-image";

    private static string? CallInfoPreview(string path, FileProbe probe)
    {
        try
        {
            byte[] pathBytes = Encoding.UTF8.GetBytes(path);
            byte[] kindBytes = Encoding.UTF8.GetBytes(probe.Kind);
            int cap = 64 * 1024;
            byte[] outBuf = ArrayPool<byte>.Shared.Rent(cap);
            try
            {
                int n = ql_preview_info(
                    pathBytes,
                    (nuint)pathBytes.Length,
                    kindBytes,
                    (nuint)kindBytes.Length,
                    probe.Size,
                    probe.ModifiedUnix,
                    outBuf,
                    (nuint)outBuf.Length);
                return n > 0 ? Encoding.UTF8.GetString(outBuf, 0, n) : null;
            }
            finally
            {
                ArrayPool<byte>.Shared.Return(outBuf);
            }
        }
        catch
        {
            return null;
        }
    }

    private static PreviewReady? ParsePreviewReady(string requestId, string json)
    {
        try
        {
            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            string kind = root.GetProperty("kind").GetString() ?? "unknown";
            string title = root.GetProperty("title").GetString() ?? kind;
            double width = kind is "archive" or "folder" or "package" ? 760 : 720;
            double height = kind is "archive" or "folder" or "package" ? 560 : 500;

            var ready = new PreviewReady(requestId, kind, title, width, height);
            if (root.TryGetProperty("listing", out var listing))
            {
                return ready with
                {
                    Listing = JsonSerializer.Deserialize<PreviewListing>(listing.GetRawText(), ProtocolJson.Options),
                };
            }

            if (root.TryGetProperty("text", out var text))
            {
                return ready with
                {
                    TextContent = text.GetString(),
                    TextFormat = root.TryGetProperty("format", out var format) ? format.GetString() : "plain",
                    TextLanguage = root.TryGetProperty("language", out var language) ? language.GetString() : "text",
                };
            }

            return null;
        }
        catch
        {
            return null;
        }
    }
}
