using System.Buffers;
using System.Security.Cryptography;
using System.Runtime.InteropServices;
using System.Security.Cryptography.X509Certificates;
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
    private static extern int ql_extract_archive_entry(
        byte[] archivePathUtf8,
        nuint archivePathLen,
        byte[] entryPathUtf8,
        nuint entryPathLen,
        byte[] outBuf,
        nuint outCap);
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
    private static extern int ql_extract_package_icon(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_extract_office_image(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_gif_frames_sized(
        byte[] pathUtf8,
        nuint pathLen,
        uint targetWidth,
        uint targetHeight,
        byte[] outBuf,
        nuint outCap);

    private delegate int NativePreviewCall(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    private delegate int NativePreviewCallWithCancel(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, IntPtr cancelCb);
    private delegate int NativeRasterCall(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    private const int MaxNativePreviewJsonBytes = 12 * 1024 * 1024;
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
        if (probe.Kind.Equals("certificate", StringComparison.OrdinalIgnoreCase))
            return TryPreviewCertificate(requestId, path, probe);

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
            ? CallPreview(cancelableCall, path)
            : call is not null
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

    public NativeRasterImage? TryExtractPackageIcon(string path)
        => CallRaster(ql_extract_package_icon, path);

    public NativeRasterImage? TryExtractOfficeImage(string path)
        => CallRaster(ql_extract_office_image, path);

    public NativeAnimationFrames? TryDecodeGifFrames(string path, uint targetWidth, uint targetHeight)
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
                    int n = ql_decode_gif_frames_sized(pathBytes, (nuint)pathBytes.Length, targetWidth, targetHeight, outBuf, (nuint)outBuf.Length);
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

    public string? TryExtractArchiveEntry(string archivePath, string entryPath)
    {
        try
        {
            byte[] archiveBytes = Encoding.UTF8.GetBytes(archivePath);
            byte[] entryBytes = Encoding.UTF8.GetBytes(entryPath);
            byte[] outBuf = ArrayPool<byte>.Shared.Rent(32 * 1024);
            try
            {
                int n = ql_extract_archive_entry(
                    archiveBytes,
                    (nuint)archiveBytes.Length,
                    entryBytes,
                    (nuint)entryBytes.Length,
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

    private static string? CallPreview(NativePreviewCallWithCancel call, string path)
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
                    int n = call(pathBytes, (nuint)pathBytes.Length, outBuf, (nuint)outBuf.Length, IntPtr.Zero);
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
        catch { return null; }
    }

    private static NativeRasterImage? CallRaster(NativeRasterCall call, string path)
    {
        try
        {
            byte[] pathBytes = Encoding.UTF8.GetBytes(path);
            return ReadRasterBuffer(cap =>
            {
                byte[] outBuf = ArrayPool<byte>.Shared.Rent(cap);
                try
                {
                    int n = call(pathBytes, (nuint)pathBytes.Length, outBuf, (nuint)outBuf.Length);
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

    private static PreviewReady TryPreviewCertificate(string requestId, string path, FileProbe probe)
    {
        string fileName = Path.GetFileName(path);
        try
        {
            using X509Certificate2 cert = X509CertificateLoader.LoadCertificateFromFile(path);
            string[] usages = cert.Extensions
                .OfType<X509EnhancedKeyUsageExtension>()
                .SelectMany(e => e.EnhancedKeyUsages.Cast<Oid>())
                .Select(oid =>
                {
                    string value = oid.Value ?? "";
                    return string.IsNullOrWhiteSpace(oid.FriendlyName) ? value : $"{oid.FriendlyName} ({value})";
                })
                .Where(s => !string.IsNullOrWhiteSpace(s))
                .ToArray();

            var builder = new StringBuilder();
            builder.AppendLine($"Name: {fileName}");
            builder.AppendLine("Kind: certificate");
            builder.AppendLine($"Subject: {cert.Subject}");
            builder.AppendLine($"Issuer: {cert.Issuer}");
            builder.AppendLine($"Serial number: {cert.SerialNumber}");
            builder.AppendLine($"Thumbprint: {cert.Thumbprint}");
            builder.AppendLine($"Valid from: {cert.NotBefore:G}");
            builder.AppendLine($"Valid until: {cert.NotAfter:G}");
            builder.AppendLine($"Signature algorithm: {cert.SignatureAlgorithm.FriendlyName ?? cert.SignatureAlgorithm.Value}");
            builder.AppendLine($"Public key: {cert.PublicKey.Oid.FriendlyName ?? cert.PublicKey.Oid.Value}");
            builder.AppendLine($"Has private key: {(cert.HasPrivateKey ? "yes" : "no")}");
            if (usages.Length > 0)
                builder.AppendLine($"Enhanced key usage: {string.Join(", ", usages)}");
            builder.AppendLine($"File size: {FormatNumber(probe.Size)} bytes");

            return new PreviewReady(requestId, "certificate", $"{fileName} - {cert.GetNameInfo(X509NameType.SimpleName, false)}", 720, 520)
            {
                TextContent = builder.ToString(),
                TextFormat = "plain",
                TextLanguage = "text",
            };
        }
        catch (Exception ex)
        {
            return new PreviewReady(requestId, "certificate", fileName, 640, 420)
            {
                TextContent = $"Name: {fileName}\nKind: certificate\nSize: {FormatNumber(probe.Size)} bytes\nStatus: failed to parse certificate\nError: {ex.Message}",
                TextFormat = "plain",
                TextLanguage = "text",
            };
        }
    }

    private static string FormatNumber(long value)
        => value.ToString("N0");

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
            double width = kind is "archive" or "folder" or "package" or "table" ? 760 : 720;
            double height = kind is "archive" or "folder" or "package" or "table" ? 560 : 500;

            var ready = new PreviewReady(requestId, kind, title, width, height);
            if (root.TryGetProperty("table", out var table))
            {
                return ready with
                {
                    Table = JsonSerializer.Deserialize<PreviewTable>(table.GetRawText(), ProtocolJson.Options),
                };
            }

            if (root.TryGetProperty("listing", out var listing))
            {
                return ready with
                {
                    Listing = JsonSerializer.Deserialize<PreviewListing>(listing.GetRawText(), ProtocolJson.Options),
                };
            }

            OfficeLayout? officeLayout = null;
            if (root.TryGetProperty("officeLayout", out var layout))
                officeLayout = JsonSerializer.Deserialize<OfficeLayout>(layout.GetRawText(), ProtocolJson.Options);

            PreviewMarkdown? markdown = null;
            if (root.TryGetProperty("markdown", out var markdownElement))
                markdown = JsonSerializer.Deserialize<PreviewMarkdown>(markdownElement.GetRawText(), ProtocolJson.Options);

            if (root.TryGetProperty("text", out var text))
            {
                return ready with
                {
                    TextContent = text.GetString(),
                    TextFormat = root.TryGetProperty("format", out var format) ? format.GetString() : "plain",
                    TextLanguage = root.TryGetProperty("language", out var language) ? language.GetString() : "text",
                    OfficeLayout = officeLayout,
                    Markdown = markdown,
                };
            }

            if (markdown is not null)
                return ready with { Markdown = markdown };

            if (officeLayout is not null)
                return ready with { OfficeLayout = officeLayout };

            return null;
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
