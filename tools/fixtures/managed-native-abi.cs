// Inert compiler input: imports/callback signatures are inspected, never invoked.
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace QuickLook.Next.App;

internal static class AbiFixture
{
    private const string Dll = "quicklook_next_native";

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void IntentCallback(IntPtr text);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    [return: MarshalAs(UnmanagedType.I1)]
    private delegate bool CancelCallback();
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate IntPtr OutputCallback(UIntPtr length);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
    private static extern int ql_probe_file([In] byte[] pathUtf8, UIntPtr pathLen, [In, Out] byte[] output, UIntPtr capacity);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
    private static extern int ql_probe_file_handle(SafeFileHandle source, ulong length, [In] byte[] name, UIntPtr nameLen,
        [In, Out] byte[] output, UIntPtr capacity, out UIntPtr outRequired);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
    private static extern int ql_get_thumbnail_cancelable_with_flags([In] byte[] path, UIntPtr pathLen, int size, uint flags,
        [In, Out] byte[] output, UIntPtr capacity, IntPtr cancel);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
    private static extern void ql_set_callback(IntentCallback? callback);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
    private static extern int ql_decode_gif_frames_handle_direct(IntPtr source, ulong length, [In] byte[] name, UIntPtr nameLen,
        uint width, uint height, out UIntPtr outRequired, OutputCallback? outputCallback, CancelCallback? cancelCallback);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
    private static extern int ql_extract_archive_entry_to_output_handle(IntPtr source, ulong length,
        [In] byte[] name, UIntPtr nameLen, [In] byte[] entry, UIntPtr entryLen,
        IntPtr outputHandle, ulong outputCapacity, out ulong written, CancelCallback? cancelCallback);
}
