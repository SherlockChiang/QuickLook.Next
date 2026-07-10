using System.Buffers;
using System.Runtime.InteropServices;
using System.Text;
using QuickLook.Next.Core;

namespace QuickLook.Next.ParserHost;

internal static class ParserNativePreview
{
    private const string Dll = "quicklook_next_native";
    // Keep native JSON within the control-pipe framing limit before forwarding it to the App.
    private const int MaxPreviewJsonBytes = PipeChannel.MaxControlLineChars;

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    [return: MarshalAs(UnmanagedType.I1)]
    private delegate bool NativeCancelCallback();

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_archive(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_office(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_extract_archive_entry(
        byte[] archivePathUtf8,
        nuint archivePathLen,
        byte[] entryPathUtf8,
        nuint entryPathLen,
        byte[] outBuf,
        nuint outCap);

    public static string? TryPreview(string kind, string path, CancellationToken cancellationToken)
    {
        NativePreviewCall call = kind.Equals("office", StringComparison.OrdinalIgnoreCase)
            ? ql_preview_office
            : ql_preview_archive;
        byte[] pathBytes = Encoding.UTF8.GetBytes(path);
        NativeCancelCallback cancel = () => cancellationToken.IsCancellationRequested;
        try
        {
            int capacity = 64 * 1024;
            while (capacity <= MaxPreviewJsonBytes)
            {
                cancellationToken.ThrowIfCancellationRequested();
                byte[] buffer = ArrayPool<byte>.Shared.Rent(capacity);
                try
                {
                    int length = call(pathBytes, (nuint)pathBytes.Length, buffer, (nuint)capacity, cancel);
                    cancellationToken.ThrowIfCancellationRequested();
                    if (length > 0 && length <= capacity)
                        return Encoding.UTF8.GetString(buffer, 0, length);
                    if (length >= 0)
                        return null;

                    int required = -length;
                    if (required <= capacity || required > MaxPreviewJsonBytes)
                        return null;
                    capacity = required;
                }
                finally
                {
                    ArrayPool<byte>.Shared.Return(buffer);
                }
            }
        }
        finally
        {
            GC.KeepAlive(cancel);
        }

        return null;
    }

    public static string? TryExtractArchiveEntry(string archivePath, string entryPath, CancellationToken cancellationToken)
    {
        const int maxPathBytes = 32 * 1024;
        try
        {
            cancellationToken.ThrowIfCancellationRequested();
            byte[] archiveBytes = Encoding.UTF8.GetBytes(archivePath);
            byte[] entryBytes = Encoding.UTF8.GetBytes(entryPath);
            byte[] buffer = ArrayPool<byte>.Shared.Rent(maxPathBytes);
            try
            {
                int length = ql_extract_archive_entry(
                    archiveBytes, (nuint)archiveBytes.Length,
                    entryBytes, (nuint)entryBytes.Length,
                    buffer, (nuint)maxPathBytes);
                cancellationToken.ThrowIfCancellationRequested();
                return length > 0 && length <= maxPathBytes
                    ? Encoding.UTF8.GetString(buffer, 0, length)
                    : null;
            }
            finally
            {
                ArrayPool<byte>.Shared.Return(buffer);
            }
        }
        catch (OperationCanceledException) { throw; }
        catch { return null; }
    }

    private delegate int NativePreviewCall(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);
}
