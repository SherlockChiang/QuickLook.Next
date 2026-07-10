using System.Runtime.InteropServices;
using Microsoft.UI.Composition;
using WinRT;

namespace QuickLook.Next.App;

/// <summary>
/// Consumer side of the cross-process composition boundary (validated in Spike 1). Turns a shared
/// DirectComposition surface handle (produced by RasterHost) into a Microsoft.UI.Composition surface
/// this app's compositor can paint. CreateCompositionSurfaceForHandle lives on ICompositorSwapChainInterop
/// (vtable slot 4); we call it via a function pointer and marshal the result with C#/WinRT.
/// </summary>
internal static class CompositionInterop
{
    private static readonly Guid IID_ICompositorSwapChainInterop = new("FC084699-67D8-40E1-ADE7-08901D84FFDA");

    public static unsafe (ICompositionSurface? Surface, int Hr) CreateSurfaceForHandle(Compositor compositor, nint sharedHandle)
    {
        nint compositorAbi = MarshalInspectable<Compositor>.FromManaged(compositor);
        try
        {
            try
            {
                Guid iid = IID_ICompositorSwapChainInterop;
                int qhr = Marshal.QueryInterface(compositorAbi, in iid, out nint interop);
                if (qhr < 0) return (null, qhr);
                try
                {
                    void** vtbl = *(void***)interop;
                    var createForHandle = (delegate* unmanaged[Stdcall]<nint, nint, nint*, int>)vtbl[4];
                    nint surfaceAbi;
                    int hr = createForHandle(interop, sharedHandle, &surfaceAbi);
                    if (hr < 0) return (null, hr);
                    try { return (MarshalInterface<ICompositionSurface>.FromAbi(surfaceAbi), 0); }
                    finally { Marshal.Release(surfaceAbi); }
                }
                finally { Marshal.Release(interop); }
            }
            finally { CloseSharedHandle(sharedHandle); }
        }
        finally { MarshalInspectable<Compositor>.DisposeAbi(compositorAbi); }
    }

    public static void CloseSharedHandle(nint sharedHandle)
    {
        if (sharedHandle != 0)
            CloseHandle(sharedHandle);
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CloseHandle(nint hObject);
}
