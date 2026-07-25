namespace QuickLook.Next.Core;

public static class NativeAbi
{
    public const uint Version = 2;
    public const ulong HandleText = 1UL << 0;
    public const ulong HandleExecutable = 1UL << 1;
    public const ulong HandleTorrent = 1UL << 2;
    public const ulong HandleSqliteSnapshot = 1UL << 3;
    public const ulong HandleArchive = 1UL << 4;
    public const ulong HandleOffice = 1UL << 5;
    public const ulong HandleEbook = 1UL << 6;
    public const ulong HandleArchiveEntry = 1UL << 7;
    public const ulong HandleStaticImage = 1UL << 8;
    public const ulong ParserHandleInputs =
        HandleText
        | HandleExecutable
        | HandleTorrent
        | HandleSqliteSnapshot
        | HandleArchive
        | HandleOffice
        | HandleEbook
        | HandleArchiveEntry;
    public const ulong RasterHandleInputs = HandleStaticImage;
    public const int MaxLogicalNameUtf8Bytes = 4 * 255;
    public const long MaxParserHandleInputBytes = 256L * 1024 * 1024;
    public const long MaxSqliteWalBytes = 64L * 1024 * 1024;
    public const long MaxSqliteShmBytes = 4L * 1024 * 1024;

    // Stable status values for ABI 2 HANDLE entry points. Legacy path entry points retain their
    // existing per-function return conventions until they are migrated.
    public const int StatusOk = 0;
    public const int StatusInvalidArgument = -1;
    public const int StatusBufferTooSmall = -2;
    public const int StatusCancelled = -3;
    public const int StatusMalformed = -4;
    public const int StatusIo = -5;
    public const int StatusInvalidHandle = -6;
    public const int StatusLengthMismatch = -7;
    public const int StatusInternal = -8;
    public const int StatusLimitExceeded = -9;

    public static void EnsureCompatible(uint actual)
    {
        if (actual != Version)
            throw new InvalidOperationException($"Native ABI mismatch: expected {Version}, received {actual}.");
    }

    public static void EnsureCapabilities(ulong actual, ulong required)
    {
        if ((actual & required) != required)
            throw new InvalidOperationException($"Native capabilities missing: required 0x{required:X}, received 0x{actual:X}.");
    }
}
