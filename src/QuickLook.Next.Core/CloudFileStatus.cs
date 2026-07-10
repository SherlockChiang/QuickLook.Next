namespace QuickLook.Next.Core;

public static class CloudFileStatus
{
    public const FileAttributes RecallOnOpen = (FileAttributes)0x00040000;
    public const FileAttributes RecallOnDataAccess = (FileAttributes)0x00400000;

    public static bool MayRequireHydration(FileAttributes attributes)
        => (attributes & (FileAttributes.Offline | RecallOnOpen | RecallOnDataAccess)) != 0;

    public static bool MayRequireHydration(string path)
    {
        try
        {
            return MayRequireHydration(File.GetAttributes(path));
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException or NotSupportedException)
        {
            return false;
        }
    }
}
