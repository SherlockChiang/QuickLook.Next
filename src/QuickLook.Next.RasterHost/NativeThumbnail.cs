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

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_get_thumbnail(byte[] pathUtf8, nuint pathLen, int size, byte[] outBuf, nuint outCap);

    // Reusable buffer (opens are processed serially by the host loop). 16 MB covers ~2048×2048 —
    // important because video thumbnails come back at the frame's aspect ratio (e.g. 1280×720+).
    private static readonly byte[] Buffer = new byte[16 * 1024 * 1024];

    public static (byte[] Bgra, int Width, int Height)? TryGet(string path, int size)
    {
        try
        {
            byte[] p = Encoding.UTF8.GetBytes(path);
            int n = ql_get_thumbnail(p, (nuint)p.Length, size, Buffer, (nuint)Buffer.Length);
            if (n <= 8) return null;
            int w = BitConverter.ToInt32(Buffer, 0);
            int h = BitConverter.ToInt32(Buffer, 4);
            if (w <= 0 || h <= 0 || n < 8 + w * h * 4) return null;
            byte[] bgra = new byte[w * h * 4];
            Array.Copy(Buffer, 8, bgra, 0, bgra.Length);
            return (bgra, w, h);
        }
        catch { return null; }
    }
}
