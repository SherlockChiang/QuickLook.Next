using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Windows.Graphics.Imaging;

namespace QuickLook.Next.RasterHost;

/// <summary>
/// Windows-codec metadata supplement bound to the retained source HANDLE.
/// The logical name is only a display hint; the decoder always reads the independently reopened
/// file object and never resolves or opens a path.
/// </summary>
internal static class SystemImageMetadataReader
{
    private const long MaxInputImageBytes = 512L * 1024 * 1024;
    private const int SystemMetadataTimeoutExitCode = 33;
    private static readonly TimeSpan DrainGrace = TimeSpan.FromMilliseconds(250);
    private static readonly SemaphoreSlim MetadataGate = new(1, 1);

    public static async Task<ImageMetadata?> TryReadHandleAsync(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        TimeSpan timeout,
        CancellationToken cancellationToken)
    {
        if (sourceLength is < 0 or > MaxInputImageBytes
            || sourceHandle.IsInvalid
            || sourceHandle.IsClosed
            || timeout <= TimeSpan.Zero)
        {
            return null;
        }

        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeoutCts.CancelAfter(timeout);
        bool enteredGate = false;
        Task<ImageMetadata?>? worker = null;
        try
        {
            await MetadataGate.WaitAsync(timeoutCts.Token);
            enteredGate = true;
            worker = Task.Run(
                () => ReadHandleAsync(
                    sourceHandle,
                    sourceLength,
                    logicalName,
                    timeoutCts.Token),
                CancellationToken.None);
            return await worker.WaitAsync(timeoutCts.Token);
        }
        catch (OperationCanceledException) when (
            !cancellationToken.IsCancellationRequested
            && timeoutCts.IsCancellationRequested)
        {
            if (worker is not null)
                await DrainWorkerOrExitAsync(worker);
            DiagLog.Write(
                "RasterHost",
                $"system HANDLE image metadata timed out; timeout={timeout.TotalMilliseconds:0}ms");
            return null;
        }
        catch (OperationCanceledException)
        {
            if (worker is not null)
                await DrainWorkerOrExitAsync(worker);
            throw;
        }
        catch (Exception ex)
        {
            DiagLog.Write("RasterHost", $"system HANDLE image metadata failed: {ex.Message}");
            return null;
        }
        finally
        {
            if (enteredGate)
                MetadataGate.Release();
        }
    }

    public static ImageMetadata? Merge(ImageMetadata? native, ImageMetadata? system)
    {
        if (native is null)
            return system;
        if (system is null)
            return native;

        return native with
        {
            Format = FirstText(native.Format, system.Format),
            Width = Positive(native.Width) ?? Positive(system.Width),
            Height = Positive(native.Height) ?? Positive(system.Height),
            HorizontalResolution = PositiveFinite(native.HorizontalResolution)
                ?? PositiveFinite(system.HorizontalResolution),
            VerticalResolution = PositiveFinite(native.VerticalResolution)
                ?? PositiveFinite(system.VerticalResolution),
            PhotometricInterpretation = FirstText(
                native.PhotometricInterpretation,
                system.PhotometricInterpretation),
            BitDepth = native.BitDepth is > 0 ? native.BitDepth : system.BitDepth,
            ColorType = FirstText(native.ColorType, system.ColorType),
            HasAlpha = native.HasAlpha ?? system.HasAlpha,
            Animated = native.Animated ?? system.Animated,
            FrameCount = Positive(native.FrameCount) ?? Positive(system.FrameCount),
        };
    }

    private static async Task<ImageMetadata?> ReadHandleAsync(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        using SafeFileHandle metadataHandle =
            WindowsHandleTransfer.ReopenReadOnlyFile(sourceHandle, sourceLength);
        using var fileStream =
            new FileStream(metadataHandle, FileAccess.Read, 64 * 1024, isAsync: false);
        if (fileStream.Length != sourceLength)
            return null;

        using var stream = fileStream.AsRandomAccessStream();
        BitmapDecoder decoder = await BitmapDecoder
            .CreateAsync(stream)
            .AsTask(cancellationToken);
        cancellationToken.ThrowIfCancellationRequested();

        uint width = decoder.OrientedPixelWidth > 0
            ? decoder.OrientedPixelWidth
            : decoder.PixelWidth;
        uint height = decoder.OrientedPixelHeight > 0
            ? decoder.OrientedPixelHeight
            : decoder.PixelHeight;
        if (width == 0 || height == 0)
            return null;

        BitmapPixelFormat pixelFormat = decoder.BitmapPixelFormat;
        BitmapAlphaMode alphaMode = decoder.BitmapAlphaMode;
        uint frameCount = decoder.FrameCount;
        return new ImageMetadata
        {
            Format = ResolveFormat(decoder.DecoderInformation.FileExtensions, logicalName),
            Width = width,
            Height = height,
            HorizontalResolution = PositiveFinite(decoder.DpiX),
            VerticalResolution = PositiveFinite(decoder.DpiY),
            PhotometricInterpretation = PhotometricName(pixelFormat),
            BitDepth = ChannelBitDepth(pixelFormat),
            ColorType = ColorTypeName(pixelFormat),
            HasAlpha = alphaMode switch
            {
                BitmapAlphaMode.Premultiplied or BitmapAlphaMode.Straight => true,
                BitmapAlphaMode.Ignore => false,
                _ => null,
            },
            Animated = frameCount > 0 ? frameCount > 1 : null,
            FrameCount = frameCount > 0 ? frameCount : null,
        };
    }

    private static async Task DrainWorkerOrExitAsync(Task worker)
    {
        if (await DrainsWithinGraceAsync(worker, DrainGrace))
            return;

        DiagLog.Write(
            "RasterHost",
            "system HANDLE image metadata did not drain after cancellation; exiting host.");
        Environment.Exit(SystemMetadataTimeoutExitCode);
    }

    internal static async Task<bool> DrainsWithinGraceAsync(
        Task worker,
        TimeSpan grace)
    {
        ArgumentNullException.ThrowIfNull(worker);
        if (grace <= TimeSpan.Zero)
            throw new ArgumentOutOfRangeException(nameof(grace));

        try
        {
            await worker.WaitAsync(grace);
            return true;
        }
        catch (TimeoutException)
        {
            return false;
        }
        catch
        {
            // A completed faulted/canceled worker has still drained.
            return true;
        }
    }

    private static string ResolveFormat(
        IReadOnlyList<string> codecExtensions,
        string logicalName)
    {
        string logicalExtension = Path.GetExtension(Path.GetFileName(logicalName));
        string extension = codecExtensions.FirstOrDefault(
                value => string.Equals(value, logicalExtension, StringComparison.OrdinalIgnoreCase))
            ?? codecExtensions
            .FirstOrDefault(static value => !string.IsNullOrWhiteSpace(value))
            ?? logicalExtension;
        return extension.Trim().TrimStart('.').ToUpperInvariant() switch
        {
            "JPG" or "JPE" or "JFIF" => "JPEG",
            "TIF" => "TIFF",
            "DIB" => "BMP",
            "" => "Image",
            string value => value,
        };
    }

    private static byte? ChannelBitDepth(BitmapPixelFormat format) => format switch
    {
        BitmapPixelFormat.Gray8 or BitmapPixelFormat.Rgba8 or BitmapPixelFormat.Bgra8 => 8,
        BitmapPixelFormat.Gray16 or BitmapPixelFormat.Rgba16 => 16,
        _ => null,
    };

    private static string? ColorTypeName(BitmapPixelFormat format) => format switch
    {
        BitmapPixelFormat.Gray8 or BitmapPixelFormat.Gray16 => "grayscale",
        BitmapPixelFormat.Rgba8 or BitmapPixelFormat.Rgba16 => "RGBA",
        BitmapPixelFormat.Bgra8 => "BGRA",
        BitmapPixelFormat.Nv12 or BitmapPixelFormat.P010 or BitmapPixelFormat.Yuy2 => "YCbCr",
        _ => null,
    };

    private static string? PhotometricName(BitmapPixelFormat format) => format switch
    {
        BitmapPixelFormat.Gray8 or BitmapPixelFormat.Gray16 => "black is zero",
        BitmapPixelFormat.Rgba8 or BitmapPixelFormat.Rgba16 or BitmapPixelFormat.Bgra8 => "RGB",
        BitmapPixelFormat.Nv12 or BitmapPixelFormat.P010 or BitmapPixelFormat.Yuy2 => "YCbCr",
        _ => null,
    };

    private static string? FirstText(string? primary, string? fallback)
        => !string.IsNullOrWhiteSpace(primary) ? primary : fallback;

    private static uint? Positive(uint? value) => value is > 0 ? value : null;

    private static double? PositiveFinite(double? value)
        => value is > 0 && double.IsFinite(value.Value) ? value : null;
}
