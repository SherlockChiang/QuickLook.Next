namespace QuickLook.Next.Core;

public static class NativeAbi
{
    public const uint Version = 3;
    public const ulong HandleText = 1UL << 0;
    public const ulong HandleExecutable = 1UL << 1;
    public const ulong HandleTorrent = 1UL << 2;
    public const ulong HandleSqliteSnapshot = 1UL << 3;
    public const ulong HandleArchive = 1UL << 4;
    public const ulong HandleOffice = 1UL << 5;
    public const ulong HandleEbook = 1UL << 6;
    public const ulong HandleArchiveEntry = 1UL << 7;
    public const ulong HandleStaticImage = 1UL << 8;
    public const ulong HandleSvg = 1UL << 9;
    public const ulong HandleGif = 1UL << 10;
    public const ulong HandlePackage = 1UL << 11;
    public const ulong HandlePackageIcon = 1UL << 12;
    public const ulong HandleProbe = 1UL << 13;
    public const ulong HandleRasterImage = 1UL << 14;
    public const ulong HandleAnimation = 1UL << 15;
    public const ulong HandleOfficeLayoutImage = 1UL << 16;
    public const ulong HandleImageWaveform = 1UL << 17;
    public const ulong HandleArchiveEntryOutput = 1UL << 18;
    public const ulong HandleImageMetadata = 1UL << 19;
    public const ulong DirectGifAnimationOutput = 1UL << 20;
    public const ulong HandleMail = 1UL << 21;
    public const ulong ParserHandleInputs =
        HandleText
        | HandleExecutable
        | HandleTorrent
        | HandleSqliteSnapshot
        | HandleArchive
        | HandleOffice
        | HandleEbook
        | HandleArchiveEntry
        | HandleArchiveEntryOutput
        | HandlePackage
        | HandlePackageIcon
        | HandleOfficeLayoutImage
        | HandleMail;
    public const ulong RasterHandleInputs = HandleStaticImage | HandleSvg | HandleGif | HandleRasterImage;
    public const int MaxLogicalNameUtf8Bytes = 4 * 255;
    public const int MaxOfficeImageRefUtf8Bytes = 2048;
    public const long MaxOfficeImageSourceBytes = 768L * 1024;
    public const int MaxOfficeImageDimension = 1024;
    public const long MaxOfficeImagePacketBytes =
        8L + (long)MaxOfficeImageDimension * MaxOfficeImageDimension * 4;
    public const long MaxParserHandleInputBytes = 256L * 1024 * 1024;
    // Archive readers seek across compressed payloads and retain only bounded headers/listing
    // metadata, so they can safely inspect large local archives without mapping the whole file.
    public const long MaxArchiveHandleInputBytes = 16L * 1024 * 1024 * 1024 * 1024;
    public const long MaxSqliteWalBytes = 64L * 1024 * 1024;
    public const long MaxSqliteShmBytes = 4L * 1024 * 1024;
    public const long MaxArchiveEntryOutputBytes = 64L * 1024 * 1024;

    // Stable status values for HANDLE entry points. Legacy path entry points retain their
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
