namespace QuickLook.Next.Core;

public static partial class NativeAbi
{
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
        | HandleMail
        | HandlePdfText;
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
    public const long MaxPdfTextInputBytes = 128L * 1024 * 1024;

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
