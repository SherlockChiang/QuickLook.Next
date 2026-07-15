namespace QuickLook.Next.Core;

public static class PreviewErrorCodes
{
    public const string ImageCodecRequired = "image.codec.required";
    public const string ImageDecodeFailed = "image.decode.failed";
}

public static class ImageCodecPolicy
{
    public static string? NormalizeFormat(string? extension)
        => extension?.ToLowerInvariant() switch
        {
            ".avif" => "avif",
            ".heic" or ".heif" => "heic",
            ".jxl" => "jxl",
            ".webp" => "webp",
            ".jpg" or ".jpeg" or ".jpe" => "jpeg",
            ".png" => "png",
            ".tif" or ".tiff" => "tiff",
            ".gif" => "gif",
            ".bmp" or ".dib" => "bmp",
            ".ico" => "ico",
            ".svg" => "svg",
            _ => null,
        };

    public static bool RequiresSystemCodec(string? extension)
        => NormalizeFormat(extension) is "avif" or "heic" or "jxl";
}
