using System.Runtime.InteropServices;
using System.Text;

namespace QuickLook.Next.RasterHost;

/// <summary>
/// Shell thumbnail fallback via the native layer (quicklook_next_native, IShellItemImageFactory).
/// Lets "press space" on any file type show the same thumbnail Explorer would.
/// </summary>
internal static class NativeThumbnail
{
    private const string Dll = "quicklook_next_native";
    private static readonly SemaphoreSlim ThumbnailGate = new(1, 1);
    private static readonly NativeCancelCallback ThumbnailCancelCallback = IsThumbnailCanceled;
    private static readonly IntPtr ThumbnailCancelCallbackPtr = Marshal.GetFunctionPointerForDelegate(ThumbnailCancelCallback);
    private static CancellationToken _thumbnailCancellationToken;

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    [return: MarshalAs(UnmanagedType.I1)]
    private delegate bool NativeCancelCallback();

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_get_thumbnail_cancelable(byte[] pathUtf8, nuint pathLen, int size, byte[] outBuf, nuint outCap, IntPtr cancelCb);

    public static async Task<(byte[] Bgra, int Width, int Height)?> TryGetAsync(string path, int size, CancellationToken cancellationToken)
    {
        await ThumbnailGate.WaitAsync(cancellationToken);
        try
        {
            return await Task.Run(() => TryGet(path, size, cancellationToken), CancellationToken.None);
        }
        finally
        {
            ThumbnailGate.Release();
        }
    }

    public static (byte[] Bgra, int Width, int Height)? TryGet(string path, int size)
        => TryGet(path, size, CancellationToken.None);

    public static (byte[] Bgra, int Width, int Height)? TryGet(string path, int size, CancellationToken cancellationToken)
    {
        byte[]? buffer = null;
        try
        {
            cancellationToken.ThrowIfCancellationRequested();
            buffer = System.Buffers.ArrayPool<byte>.Shared.Rent(16 * 1024 * 1024);
            byte[] p = Encoding.UTF8.GetBytes(path);
            _thumbnailCancellationToken = cancellationToken;
            int n = ql_get_thumbnail_cancelable(p, (nuint)p.Length, size, buffer, (nuint)buffer.Length, ThumbnailCancelCallbackPtr);
            _thumbnailCancellationToken = CancellationToken.None;
            if (n <= 8) return null;
            cancellationToken.ThrowIfCancellationRequested();
            int w = BitConverter.ToInt32(buffer, 0);
            int h = BitConverter.ToInt32(buffer, 4);
            if (w <= 0 || h <= 0 || n < 8 + w * h * 4) return null;
            byte[] bgra = new byte[w * h * 4];
            Array.Copy(buffer, 8, bgra, 0, bgra.Length);
            return (bgra, w, h);
        }
        catch (OperationCanceledException) { throw; }
        catch { return null; }
        finally
        {
            _thumbnailCancellationToken = CancellationToken.None;
            if (buffer != null)
                System.Buffers.ArrayPool<byte>.Shared.Return(buffer);
        }
    }

    private static bool IsThumbnailCanceled()
        => _thumbnailCancellationToken.IsCancellationRequested;
}
