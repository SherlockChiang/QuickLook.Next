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
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    [return: MarshalAs(UnmanagedType.I1)]
    private delegate bool NativeCancelCallback();

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
    private static extern int ql_preview_archive(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, IntPtr cancelCb);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_ebook(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_office(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, IntPtr cancelCb);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_executable(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_torrent(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_folder(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, IntPtr cancelCb);
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
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_image_metadata(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_get_thumbnail(byte[] pathUtf8, nuint pathLen, int size, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_get_thumbnail_cancelable(byte[] pathUtf8, nuint pathLen, int size, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_gif_frames_sized(
        byte[] pathUtf8,
        nuint pathLen,
        uint targetWidth,
        uint targetHeight,
        byte[] outBuf,
        nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_webp_frames_sized(
        byte[] pathUtf8,
        nuint pathLen,
        uint targetWidth,
        uint targetHeight,
        byte[] outBuf,
        nuint outCap);

    private delegate int NativePreviewCall(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    private delegate int NativePreviewCallWithCancel(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, IntPtr cancelCb);
    private delegate int NativeAnimationCall(byte[] pathUtf8, nuint pathLen, uint targetWidth, uint targetHeight, byte[] outBuf, nuint outCap);
    private const int MaxNativePreviewJsonBytes = 12 * 1024 * 1024;
    private const int MaxNativeProbeJsonBytes = 512 * 1024;
    private const int MaxNativeRasterBytes = 16 * 1024 * 1024;
    private const int MaxNativeAnimationBytes = 80 * 1024 * 1024;

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
                if (n < -2)
                {
                    int required = checked(-n);
                    if (required > MaxNativeProbeJsonBytes)
                        return null;
                    ArrayPool<byte>.Shared.Return(outBuf);
                    outBuf = ArrayPool<byte>.Shared.Rent(required);
                    n = ql_probe_file(pathBytes, (nuint)pathBytes.Length, outBuf, (nuint)outBuf.Length);
                }
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

    public PreviewReady? TryPreview(string requestId, string path, FileProbe probe, CancellationToken cancellationToken = default)
    {
        if (probe.Kind.Equals("certificate", StringComparison.OrdinalIgnoreCase))
            return CertificatePreview.Create(requestId, path, probe.Size);

        NativePreviewCall? call = probe.Kind.ToLowerInvariant() switch
        {
            "text" => ql_preview_text,
            "ebook" => ql_preview_ebook,
            "executable" => ql_preview_executable,
            "torrent" => ql_preview_torrent,
            _ => null,
        };
        NativePreviewCallWithCancel? cancelableCall = probe.Kind.ToLowerInvariant() switch
        {
            "archive" => ql_preview_archive,
            "package" => ql_preview_archive,
            "office" => ql_preview_office,
            "folder" => ql_preview_folder,
            _ => null,
        };

        string? json = cancelableCall is not null
            ? CallPreview(cancelableCall, path, cancellationToken)
            : call is not null
            ? CallPreview(call, path)
            : ShouldUseNativeInfo(probe) ? CallInfoPreview(path, probe) : null;
        return string.IsNullOrWhiteSpace(json)
            ? null
            : PreviewReadyJson.TryParse(requestId, json, out PreviewReady? ready, out _) ? ready : null;
    }

    public PreviewListing? TryPreviewFolderListing(string path)
    {
        string? json = CallPreview(ql_preview_folder, path, CancellationToken.None);
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

    public ImageMetadata? TryPreviewImageMetadata(string path)
    {
        string? json = CallPreview(ql_preview_image_metadata, path);
        if (string.IsNullOrWhiteSpace(json))
            return null;

        try
        {
            return JsonSerializer.Deserialize<ImageMetadata>(json, ProtocolJson.Options);
        }
        catch
        {
            return null;
        }
    }

    public NativeAnimationFrames? TryDecodeGifFrames(string path, uint targetWidth, uint targetHeight)
        => TryDecodeAnimationFrames(ql_decode_gif_frames_sized, path, targetWidth, targetHeight);

    public NativeAnimationFrames? TryDecodeWebPFrames(string path, uint targetWidth, uint targetHeight)
        => TryDecodeAnimationFrames(ql_decode_webp_frames_sized, path, targetWidth, targetHeight);

    private static NativeAnimationFrames? TryDecodeAnimationFrames(NativeAnimationCall call, string path, uint targetWidth, uint targetHeight)
    {
        try
        {
            byte[] pathBytes = Encoding.UTF8.GetBytes(path);
            int cap = 8 * 1024 * 1024;
            while (cap <= MaxNativeAnimationBytes)
            {
                byte[] outBuf = ArrayPool<byte>.Shared.Rent(cap);
                try
                {
                    int n = call(pathBytes, (nuint)pathBytes.Length, targetWidth, targetHeight, outBuf, (nuint)outBuf.Length);
                    if (n < 0)
                    {
                        int needed = -n;
                        if (needed <= cap || needed > MaxNativeAnimationBytes)
                            return null;
                        cap = needed;
                        continue;
                    }
                    if (n <= 12)
                        return null;

                    int frameCount = checked((int)BitConverter.ToUInt32(outBuf, 0));
                    int width = checked((int)BitConverter.ToUInt32(outBuf, 4));
                    int height = checked((int)BitConverter.ToUInt32(outBuf, 8));
                    int frameBytes = checked(width * height * 4);
                    if (frameCount <= 0 || width <= 0 || height <= 0 || 12 + frameCount * (4 + frameBytes) != n)
                        return null;

                    var frames = new List<NativeAnimationFrame>(frameCount);
                    int offset = 12;
                    for (int i = 0; i < frameCount; i++)
                    {
                        int delayMs = checked((int)BitConverter.ToUInt32(outBuf, offset));
                        offset += 4;
                        var bgra = new byte[frameBytes];
                        Buffer.BlockCopy(outBuf, offset, bgra, 0, frameBytes);
                        offset += frameBytes;
                        frames.Add(new NativeAnimationFrame(delayMs, bgra));
                    }
                    return new NativeAnimationFrames(width, height, frames);
                }
                finally { ArrayPool<byte>.Shared.Return(outBuf); }
            }
        }
        catch { }

        return null;
    }

    public NativeRasterImage? TryGetThumbnail(string path, int size)
        => TryGetThumbnail(path, size, CancellationToken.None);

    public NativeRasterImage? TryGetThumbnail(string path, int size, CancellationToken token)
    {
        NativeCancelCallback? cancelCb = null;
        try
        {
            byte[] pathBytes = Encoding.UTF8.GetBytes(path);
            cancelCb = token.CanBeCanceled
                ? () => token.IsCancellationRequested
                : null;
            return ReadRasterBuffer(cap =>
            {
                byte[] outBuf = ArrayPool<byte>.Shared.Rent(cap);
                try
                {
                    int n = cancelCb is null
                        ? ql_get_thumbnail(pathBytes, (nuint)pathBytes.Length, size, outBuf, (nuint)outBuf.Length)
                        : ql_get_thumbnail_cancelable(pathBytes, (nuint)pathBytes.Length, size, outBuf, (nuint)outBuf.Length, cancelCb);
                    return (n, outBuf);
                }
                catch
                {
                    ArrayPool<byte>.Shared.Return(outBuf);
                    throw;
                }
            });
        }
        catch
        {
            return null;
        }
        finally
        {
            GC.KeepAlive(cancelCb);
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

    private static string? CallPreview(NativePreviewCallWithCancel call, string path, CancellationToken cancellationToken)
    {
        NativeCancelCallback? cancelCb = cancellationToken.CanBeCanceled
            ? () => cancellationToken.IsCancellationRequested
            : null;
        try
        {
            byte[] pathBytes = Encoding.UTF8.GetBytes(path);
            int cap = 256 * 1024;
            while (cap <= MaxNativePreviewJsonBytes)
            {
                byte[] outBuf = ArrayPool<byte>.Shared.Rent(cap);
                try
                {
                    cancellationToken.ThrowIfCancellationRequested();
                    IntPtr cancelCbPtr = cancelCb is null
                        ? IntPtr.Zero
                        : Marshal.GetFunctionPointerForDelegate(cancelCb);
                    int n = call(pathBytes, (nuint)pathBytes.Length, outBuf, (nuint)outBuf.Length, cancelCbPtr);
                    cancellationToken.ThrowIfCancellationRequested();
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
            return null;
        }
        catch (OperationCanceledException) { throw; }
        catch { return null; }
        finally { GC.KeepAlive(cancelCb); }
    }

    private static NativeRasterImage? ReadRasterBuffer(Func<int, (int Length, byte[] Buffer)> read)
    {
        int cap = 2 * 1024 * 1024;
        while (cap <= MaxNativeRasterBytes)
        {
            var (n, outBuf) = read(cap);
            try
            {
                if (n > 8)
                {
                    int width = BitConverter.ToInt32(outBuf, 0);
                    int height = BitConverter.ToInt32(outBuf, 4);
                    int bytes = checked(width * height * 4);
                    if (width <= 0 || height <= 0 || n < 8 + bytes)
                        return null;
                    byte[] bgra = new byte[bytes];
                    Array.Copy(outBuf, 8, bgra, 0, bytes);
                    return new NativeRasterImage(bgra, width, height);
                }
                if (n < 0)
                {
                    int needed = -n;
                    if (needed <= cap || needed > MaxNativeRasterBytes)
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

    private static bool ShouldUseNativeInfo(FileProbe probe)
        => probe.Kind is "binary" or "unknown" or "disk-image" or "font" or "database" or "mail" or "chm" or "dump" or "elf" or "video" or "audio" or "media";

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

}

internal sealed record NativeRasterImage(byte[] Bgra, int Width, int Height);
internal sealed record NativeAnimationFrame(int DelayMilliseconds, byte[] Bgra);
internal sealed record NativeAnimationFrames(int Width, int Height, IReadOnlyList<NativeAnimationFrame> Frames);
