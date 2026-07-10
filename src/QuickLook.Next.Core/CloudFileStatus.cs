using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace QuickLook.Next.Core;

public enum CloudFileAvailability
{
    Local,
    RequiresHydration,
    Unknown,
}

public static class CloudFileStatus
{
    public const FileAttributes RecallOnOpen = (FileAttributes)0x00040000;
    public const FileAttributes RecallOnDataAccess = (FileAttributes)0x00400000;

    public static bool MayRequireHydration(FileAttributes attributes)
        => (attributes & (FileAttributes.Offline | RecallOnOpen | RecallOnDataAccess)) != 0;

    public static bool IsCloudReparseTag(uint reparseTag)
        => (reparseTag & 0xFFFF0FFF) == 0x9000001A;

    public static bool MayRequireHydration(string path)
        => GetAvailability(path) != CloudFileAvailability.Local;

    public static CloudFileAvailability GetAvailability(string path)
    {
        try
        {
            FileAttributes attributes = File.GetAttributes(path);
            if (MayRequireHydration(attributes))
                return CloudFileAvailability.RequiresHydration;
            if ((attributes & FileAttributes.ReparsePoint) == 0)
                return CloudFileAvailability.Local;
            if (!TryGetReparseTag(path, out uint reparseTag))
                return CloudFileAvailability.Unknown;
            return IsCloudReparseTag(reparseTag)
                ? CloudFileAvailability.RequiresHydration
                : CloudFileAvailability.Local;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException or NotSupportedException)
        {
            return CloudFileAvailability.Unknown;
        }
    }

    private static bool TryGetReparseTag(string path, out uint reparseTag)
    {
        reparseTag = 0;
        using SafeFileHandle handle = CreateFile(
            path,
            0,
            FileShare.ReadWrite | FileShare.Delete,
            nint.Zero,
            FileMode.Open,
            FileFlagBackupSemantics | FileFlagOpenReparsePoint,
            nint.Zero);
        if (handle.IsInvalid)
            return false;
        if (!GetFileInformationByHandleEx(
                handle,
                FileAttributeTagInfoClass,
                out FileAttributeTagInfo info,
                (uint)Marshal.SizeOf<FileAttributeTagInfo>()))
            return false;
        reparseTag = info.ReparseTag;
        return true;
    }

    private const int FileAttributeTagInfoClass = 9;
    private const uint FileFlagOpenReparsePoint = 0x00200000;
    private const uint FileFlagBackupSemantics = 0x02000000;

    [StructLayout(LayoutKind.Sequential)]
    private struct FileAttributeTagInfo
    {
        public FileAttributes FileAttributes;
        public uint ReparseTag;
    }

    [DllImport("kernel32.dll", EntryPoint = "CreateFileW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern SafeFileHandle CreateFile(
        string fileName,
        uint desiredAccess,
        FileShare shareMode,
        nint securityAttributes,
        FileMode creationDisposition,
        uint flagsAndAttributes,
        nint templateFile);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileInformationByHandleEx(
        SafeFileHandle file,
        int fileInformationClass,
        out FileAttributeTagInfo fileInformation,
        uint bufferSize);
}
