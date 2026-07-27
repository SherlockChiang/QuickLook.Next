using System.Buffers;
using System.Runtime.InteropServices;
using System.Text;

namespace QuickLook.Next.ShellBroker;

internal static class NativeThumbnail
{
    private const string Dll = "quicklook_next_native";
    private const int MaxPacketBytes = 8 + 512 * 512 * 4;
    private const uint BoundedSizeFlag = 2;
    private static readonly SemaphoreSlim Gate = new(1, 1);
    private static readonly NativeCancelCallback CancelCallback = IsCanceled;
    private static readonly IntPtr CancelCallbackPtr = Marshal.GetFunctionPointerForDelegate(CancelCallback);
    private static CancellationToken _cancellationToken;

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    [return: MarshalAs(UnmanagedType.I1)]
    private delegate bool NativeCancelCallback();

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_get_thumbnail_cancelable_with_flags(
        byte[] pathUtf8, nuint pathLen, int size, uint flags, byte[] outBuf, nuint outCap, IntPtr cancelCb);

    public static byte[]? TryGetPacket(string path, int size, CancellationToken cancellationToken)
    {
        Gate.Wait(cancellationToken);
        byte[] buffer = ArrayPool<byte>.Shared.Rent(MaxPacketBytes);
        try
        {
            cancellationToken.ThrowIfCancellationRequested();
            byte[] pathBytes = Encoding.UTF8.GetBytes(path);
            _cancellationToken = cancellationToken;
            int length = ql_get_thumbnail_cancelable_with_flags(
                pathBytes, (nuint)pathBytes.Length, size, BoundedSizeFlag,
                buffer, (nuint)MaxPacketBytes, CancelCallbackPtr);
            cancellationToken.ThrowIfCancellationRequested();
            if (length <= 8 || length > MaxPacketBytes)
                return null;
            int width = BitConverter.ToInt32(buffer, 0);
            int height = BitConverter.ToInt32(buffer, 4);
            if (width is <= 0 or > 512 || height is <= 0 or > 512
                || length != 8 + width * height * 4)
                return null;
            return buffer[..length];
        }
        finally
        {
            _cancellationToken = CancellationToken.None;
            ArrayPool<byte>.Shared.Return(buffer);
            Gate.Release();
        }
    }

    private static bool IsCanceled() => _cancellationToken.IsCancellationRequested;
}
