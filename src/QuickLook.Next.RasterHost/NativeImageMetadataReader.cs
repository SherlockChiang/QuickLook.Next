using System.Buffers;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.RasterHost;

internal readonly record struct NativeImageMetadataResult(
    int Status,
    ImageMetadata? Metadata,
    bool IsSupported = true);

internal static class NativeImageMetadataReader
{
    private const string Dll = "quicklook_next_native";
    private const int InitialJsonBytes = 16 * 1024;
    internal const int MaxMetadataJsonBytes = 1024 * 1024;
    private const long MaxInputImageBytes = 256L * 1024 * 1024;
    private static readonly SemaphoreSlim MetadataGate = new(1, 1);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    [return: MarshalAs(UnmanagedType.I1)]
    private delegate bool NativeCancelCallback();

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_image_metadata_handle(
        nint sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        byte[] outBuf,
        nuint outCap,
        out nuint outRequired,
        NativeCancelCallback cancelCb);

    public static async Task<NativeImageMetadataResult> TryReadHandleAsync(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        TimeSpan timeout,
        CancellationToken cancellationToken)
    {
        if (!NativeImageDecoder.SupportsHandleImageMetadata)
        {
            return new NativeImageMetadataResult(
                NativeAbi.StatusInvalidArgument,
                null,
                IsSupported: false);
        }

        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeoutCts.CancelAfter(timeout);
        try
        {
            await MetadataGate.WaitAsync(timeoutCts.Token);
            try
            {
                NativeImageMetadataResult result = await Task.Run(
                    () => TryReadHandle(
                        sourceHandle,
                        sourceLength,
                        logicalName,
                        timeoutCts.Token),
                    CancellationToken.None);
                cancellationToken.ThrowIfCancellationRequested();
                return result;
            }
            finally
            {
                MetadataGate.Release();
            }
        }
        catch (OperationCanceledException) when (
            !cancellationToken.IsCancellationRequested
            && timeoutCts.IsCancellationRequested)
        {
            return new NativeImageMetadataResult(NativeAbi.StatusCancelled, null);
        }
    }

    private static NativeImageMetadataResult TryReadHandle(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        CancellationToken cancellationToken)
    {
        if (sourceLength is < 0 or > MaxInputImageBytes
            || sourceHandle.IsInvalid
            || sourceHandle.IsClosed)
        {
            return new NativeImageMetadataResult(NativeAbi.StatusInvalidArgument, null);
        }

        string fileName = Path.GetFileName(logicalName);
        byte[] logicalNameBytes = Encoding.UTF8.GetBytes(fileName);
        if (logicalNameBytes.Length is 0 or > NativeAbi.MaxLogicalNameUtf8Bytes)
            return new NativeImageMetadataResult(NativeAbi.StatusInvalidArgument, null);

        NativeCancelCallback cancel = () => cancellationToken.IsCancellationRequested;
        bool addRef = false;
        try
        {
            sourceHandle.DangerousAddRef(ref addRef);
            int capacity = InitialJsonBytes;
            while (capacity <= MaxMetadataJsonBytes)
            {
                cancellationToken.ThrowIfCancellationRequested();
                byte[] buffer = ArrayPool<byte>.Shared.Rent(capacity);
                try
                {
                    int status = ql_preview_image_metadata_handle(
                        sourceHandle.DangerousGetHandle(),
                        checked((ulong)sourceLength),
                        logicalNameBytes,
                        (nuint)logicalNameBytes.Length,
                        buffer,
                        (nuint)capacity,
                        out nuint required,
                        cancel);
                    cancellationToken.ThrowIfCancellationRequested();

                    if (status == NativeAbi.StatusOk)
                    {
                        if (required is 0
                            || required > (nuint)capacity
                            || required > (nuint)MaxMetadataJsonBytes)
                        {
                            return new NativeImageMetadataResult(NativeAbi.StatusInternal, null);
                        }

                        try
                        {
                            ImageMetadata? metadata = JsonSerializer.Deserialize<ImageMetadata>(
                                buffer.AsSpan(0, checked((int)required)),
                                ProtocolJson.Options);
                            return metadata is null
                                ? new NativeImageMetadataResult(NativeAbi.StatusMalformed, null)
                                : new NativeImageMetadataResult(NativeAbi.StatusOk, metadata);
                        }
                        catch (JsonException)
                        {
                            return new NativeImageMetadataResult(NativeAbi.StatusMalformed, null);
                        }
                    }

                    if (status != NativeAbi.StatusBufferTooSmall)
                    {
                        return required == 0
                            ? new NativeImageMetadataResult(status, null)
                            : new NativeImageMetadataResult(NativeAbi.StatusInternal, null);
                    }

                    if (required <= (nuint)capacity)
                        return new NativeImageMetadataResult(NativeAbi.StatusInternal, null);
                    if (required > (nuint)MaxMetadataJsonBytes)
                        return new NativeImageMetadataResult(NativeAbi.StatusBufferTooSmall, null);
                    capacity = checked((int)required);
                }
                finally
                {
                    ArrayPool<byte>.Shared.Return(buffer);
                }
            }
        }
        catch (ObjectDisposedException)
        {
            return new NativeImageMetadataResult(NativeAbi.StatusInvalidHandle, null);
        }
        catch (OverflowException)
        {
            return new NativeImageMetadataResult(NativeAbi.StatusLengthMismatch, null);
        }
        finally
        {
            if (addRef)
                sourceHandle.DangerousRelease();
            GC.KeepAlive(cancel);
        }

        return new NativeImageMetadataResult(NativeAbi.StatusBufferTooSmall, null);
    }

    public static string DescribeStatus(int status) => status switch
    {
        NativeAbi.StatusInvalidArgument => "Invalid image metadata request.",
        NativeAbi.StatusBufferTooSmall => "Image metadata exceeded the host limit.",
        NativeAbi.StatusCancelled => "Image metadata request timed out.",
        NativeAbi.StatusMalformed => "Image metadata is malformed or unsupported.",
        NativeAbi.StatusIo => "Image metadata could not be read.",
        NativeAbi.StatusInvalidHandle => "Image metadata source is no longer available.",
        NativeAbi.StatusLengthMismatch => "Image metadata source changed after it was opened.",
        NativeAbi.StatusLimitExceeded => "Image metadata source exceeded the host limit.",
        _ => "Image metadata extraction failed.",
    };
}
