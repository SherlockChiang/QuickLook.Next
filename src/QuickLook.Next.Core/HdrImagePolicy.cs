using QuickLook.Next.Contracts;

namespace QuickLook.Next.Core;

/// <summary>
/// Stable surface identifiers shared by the App and RasterHost. Keep these
/// values wire-compatible: the control channel carries the string form.
/// </summary>
public static class PreviewSurfaceFormats
{
    /// <summary>Premultiplied 8-bit BGRA in the SDR composition path.</summary>
    public const string SdrBgra8 = "B8G8R8A8_UNORM";

    /// <summary>Linear scRGB stored as 16-bit floating-point RGBA.</summary>
    public const string HdrScRgbFp16 = "R16G16B16A16_FLOAT_SCRGB";

    public static bool IsKnown(string? format)
        => string.Equals(format, SdrBgra8, StringComparison.Ordinal)
            || string.Equals(format, HdrScRgbFp16, StringComparison.Ordinal);

    public static bool IsHdr(string? format)
        => string.Equals(format, HdrScRgbFp16, StringComparison.Ordinal);
}

/// <summary>Display color capability observed by the WinUI shell.</summary>
public enum DisplayColorCapability
{
    Unknown = 0,
    StandardDynamicRange = 1,
    HighDynamicRange = 2,
}

/// <summary>Color transfer and gamut facts required before treating an image as HDR.</summary>
public enum ImageColorProfileKind
{
    Unknown = 0,
    StandardDynamicRange = 1,
    PqBt2020 = 2,
    HlgBt2020 = 3,
}

/// <summary>
/// Source/display decision for a preview surface. This is deliberately pure so
/// it can be tested without a GPU or HDR monitor and reused by future decoders.
/// </summary>
public readonly record struct HdrImageSourceInfo(
    ImageColorProfileKind Profile,
    int BitDepth,
    bool MetadataComplete = false)
{
    public bool IsHdr
        => MetadataComplete
            && BitDepth >= 10
            && Profile is ImageColorProfileKind.PqBt2020 or ImageColorProfileKind.HlgBt2020;
}

public readonly record struct PreviewSurfacePlan(string Format, bool IsHdr)
{
    public static PreviewSurfacePlan Sdr
        => new(PreviewSurfaceFormats.SdrBgra8, IsHdr: false);

    public static PreviewSurfacePlan Hdr
        => new(PreviewSurfaceFormats.HdrScRgbFp16, IsHdr: true);
}

public static class HdrImagePolicy
{
    /// <summary>
    /// Converts decoder metadata into the strict source facts required by HDR
    /// selection. Unknown or incomplete metadata deliberately becomes SDR.
    /// </summary>
    public static HdrImageSourceInfo FromMetadata(ImageMetadata? metadata)
    {
        if (metadata is null)
            return default;

        ImageColorProfileKind profile = ParseProfile(
            metadata.HdrTransferFunction,
            metadata.HdrColorPrimaries);
        return new HdrImageSourceInfo(
            profile,
            metadata.BitDepth.GetValueOrDefault(),
            metadata.HdrMetadataComplete == true);
    }

    /// <summary>
    /// Select HDR only when both the display and source are explicitly known to
    /// support it. Unknown values fail closed to the existing SDR path.
    /// </summary>
    public static PreviewSurfacePlan Select(
        DisplayColorCapability display,
        HdrImageSourceInfo source,
        bool fp16SurfaceSupported)
        => display == DisplayColorCapability.HighDynamicRange
            && source.IsHdr
            // HLG metadata is preserved for diagnostics, but its OOTF and
            // mastering-luminance mapping are not implemented yet.
            && source.Profile == ImageColorProfileKind.PqBt2020
            && fp16SurfaceSupported
                ? PreviewSurfacePlan.Hdr
                : PreviewSurfacePlan.Sdr;

    public static PreviewSurfacePlan Reconcile(
        DisplayColorCapability display,
        HdrImageSourceInfo source,
        string? actualFormat)
    {
        if (!PreviewSurfaceFormats.IsKnown(actualFormat))
            return PreviewSurfacePlan.Sdr;

        bool hdr = PreviewSurfaceFormats.IsHdr(actualFormat);
        return hdr && Select(display, source, fp16SurfaceSupported: true).IsHdr
            ? PreviewSurfacePlan.Hdr
            : PreviewSurfacePlan.Sdr;
    }

    private static ImageColorProfileKind ParseProfile(string? transfer, string? primaries)
    {
        if (!string.Equals(primaries, "BT.2020", StringComparison.OrdinalIgnoreCase)
            && !string.Equals(primaries, "BT2020", StringComparison.OrdinalIgnoreCase))
            return ImageColorProfileKind.Unknown;

        if (string.Equals(transfer, "PQ", StringComparison.OrdinalIgnoreCase)
            || string.Equals(transfer, "SMPTE ST 2084", StringComparison.OrdinalIgnoreCase)
            || string.Equals(transfer, "ST2084", StringComparison.OrdinalIgnoreCase))
            return ImageColorProfileKind.PqBt2020;
        if (string.Equals(transfer, "HLG", StringComparison.OrdinalIgnoreCase)
            || string.Equals(transfer, "ARIB STD-B67", StringComparison.OrdinalIgnoreCase))
            return ImageColorProfileKind.HlgBt2020;
        return ImageColorProfileKind.Unknown;
    }
}
