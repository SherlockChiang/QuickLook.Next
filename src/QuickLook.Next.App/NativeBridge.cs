using System.Buffers;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

/// <summary>
/// In-process FFI to quicklook_next_native (the Rust cdylib). The native layer installs the keyboard
/// hook and reads the Explorer selection, then calls back with high-level intent lines, which we decode
/// into <see cref="NativeIntent"/>. (Validated in Spike 3.)
/// </summary>
internal sealed class NativeBridge : IDisposable
{
    private const string Dll = "quicklook_next_native";

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void NativeCallback(IntPtr utf16);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    [return: MarshalAs(UnmanagedType.I1)]
    private delegate bool NativeCancelCallback();

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern uint ql_abi_version();
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern ulong ql_capabilities();
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern void ql_set_callback(NativeCallback? cb);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_start();
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_stop();
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern void ql_set_preview_visible(int visible);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern void ql_get_selection();
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_probe_file(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_probe_file_handle(
        SafeFileHandle sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        byte[] outBuf,
        nuint outCap,
        out nuint outRequired);
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
    private static extern int ql_get_thumbnail_cancelable_with_flags(byte[] pathUtf8, nuint pathLen, int size, uint flags, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);
    private delegate int NativePreviewCall(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    private delegate int NativePreviewCallWithCancel(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, IntPtr cancelCb);
    private const int MaxNativePreviewJsonBytes = 12 * 1024 * 1024;
    private const int MaxNativeProbeJsonBytes = 512 * 1024;
    private const int MaxNativeRasterBytes = 16 * 1024 * 1024;

    private NativeCallback? _callback; // keep alive: native stores the function pointer
    private Action<NativeIntent>? _onIntent;
    private ulong _capabilities;
    public NativeHookStatus? HookStatus { get; private set; }
    public NativeHookStatus? LastHookFailure { get; private set; }

    public void Start(Action<NativeIntent> onIntent)
    {
        NativeAbi.EnsureCompatible(ql_abi_version());
        _capabilities = ql_capabilities();
        _onIntent = onIntent;
        LastHookFailure = null;
        _callback = OnNative;
        ql_set_callback(_callback);
        int status = ql_start();
        if (status <= 0 || HookStatus?.State == NativeHookState.Failed)
        {
            LastHookFailure = HookStatus?.State == NativeHookState.Failed
                ? HookStatus
                : new NativeHookStatus(NativeHookState.Failed, "START", status);
            Stop();
            throw new InvalidOperationException($"Native keyboard hook failed to start (status {status}).");
        }
        if (HookStatus?.State is not (NativeHookState.Ready or NativeHookState.Degraded))
        {
            Stop();
            throw new InvalidOperationException("Native keyboard hook did not report readiness.");
        }
    }

    public void Stop()
    {
        _onIntent = null;
        try { ql_set_preview_visible(0); } catch { }
        try { ql_stop(); } catch { }
        try { ql_set_callback(null); } catch { }
        _callback = null;
        HookStatus = new NativeHookStatus(NativeHookState.Stopped);
    }

    public void Dispose() => Stop();

    public void SetPreviewVisible(bool visible)
    {
        try { ql_set_preview_visible(visible ? 1 : 0); }
        catch { /* ignore: stale native builds simply behave like the old hook */ }
    }

    private void OnNative(IntPtr utf16)
    {
        string? line = Marshal.PtrToStringUni(utf16);
        if (line is null) return;
        NativeHookStatus? hookStatus = NativeHookStatus.TryParse(line);
        if (hookStatus is not null)
        {
            HookStatus = hookStatus;
            if (hookStatus.State == NativeHookState.Failed)
                LastHookFailure = hookStatus;
            return;
        }
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

    public bool SupportsHandleProbe => (_capabilities & NativeAbi.HandleProbe) != 0;

    public FileProbe? ProbeFileHandle(SafeFileHandle sourceHandle, long expectedLength, string logicalPath)
    {
        if (!SupportsHandleProbe)
            return null;
        if (expectedLength < 0)
            throw new InvalidDataException("Pinned preview length is invalid.");

        byte[] logicalName = Encoding.UTF8.GetBytes(Path.GetFileName(logicalPath));
        if (logicalName.Length is 0 or > NativeAbi.MaxLogicalNameUtf8Bytes)
            throw new InvalidDataException("Pinned preview logical name is invalid.");

        byte[] outBuf = ArrayPool<byte>.Shared.Rent(2048);
        try
        {
            int status = ql_probe_file_handle(
                sourceHandle,
                checked((ulong)expectedLength),
                logicalName,
                (nuint)logicalName.Length,
                outBuf,
                (nuint)outBuf.Length,
                out nuint required);
            if (status == NativeAbi.StatusBufferTooSmall)
            {
                if (required is 0 || required > MaxNativeProbeJsonBytes)
                    throw new InvalidDataException("Native HANDLE probe output exceeds its bound.");
                ArrayPool<byte>.Shared.Return(outBuf);
                outBuf = ArrayPool<byte>.Shared.Rent(checked((int)required));
                status = ql_probe_file_handle(
                    sourceHandle,
                    checked((ulong)expectedLength),
                    logicalName,
                    (nuint)logicalName.Length,
                    outBuf,
                    (nuint)outBuf.Length,
                    out required);
            }
            if (status != NativeAbi.StatusOk || required is 0 || required > (nuint)outBuf.Length)
                throw new InvalidDataException($"Native HANDLE probe failed with status {status}.");

            return ParseProbe(outBuf, checked((int)required), logicalPath);
        }
        finally { ArrayPool<byte>.Shared.Return(outBuf); }
    }

    private static FileProbe ParseProbe(byte[] utf8Json, int length, string path)
    {
        using var doc = JsonDocument.Parse(new ReadOnlyMemory<byte>(utf8Json, 0, length));
        var root = doc.RootElement;
        string magicHex = root.GetProperty("magicHex").GetString() ?? "";
        return new FileProbe(path, root.GetProperty("extension").GetString() ?? "", magicHex.Length > 0 ? Convert.FromHexString(magicHex) : [])
        {
            Kind = root.GetProperty("kind").GetString() ?? "unknown",
            Size = root.GetProperty("size").GetInt64(),
            ModifiedUnix = root.GetProperty("modifiedUnix").GetInt64(),
        };
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

    public NativeRasterImage? TryGetThumbnail(string path, int size)
        => TryGetThumbnail(path, size, CancellationToken.None);

    public NativeRasterImage? TryGetThumbnail(string path, int size, CancellationToken token)
        => TryGetThumbnail(path, size, cacheOnly: false, token);

    public NativeRasterImage? TryGetThumbnail(string path, int size, bool cacheOnly, CancellationToken token)
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
                    int n = cacheOnly
                        ? ql_get_thumbnail_cancelable_with_flags(pathBytes, (nuint)pathBytes.Length, size, 1, outBuf, (nuint)outBuf.Length, cancelCb)
                        : cancelCb is null
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
            while (cap <= MaxNativePreviewJsonBytes)
            {
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

}

internal sealed record NativeRasterImage(byte[] Bgra, int Width, int Height);
internal sealed record NativeAnimationFrame(int DelayMilliseconds, byte[] Bgra);
internal sealed record NativeAnimationFrames(int Width, int Height, IReadOnlyList<NativeAnimationFrame> Frames);
