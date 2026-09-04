using System.Text.Json;
using System.Text.RegularExpressions;

namespace QuickLook.Next.Core;

public readonly record struct PreviewFailureDiagnostic(
    int SchemaVersion,
    string ErrorCode,
    string Phase,
    long CorrelationId,
    string Version,
    string Format,
    string SizeBucket);

public static partial class PreviewFailureDiagnostics
{
    public const int SchemaVersion = 1;
    public const int MaxClipboardChars = 1024;
    private const long OneMebibyte = 1024 * 1024;
    private static readonly JsonSerializerOptions JsonOptions = new() { WriteIndented = true };

    public static PreviewFailureDiagnostic Create(
        string? errorCode,
        string? phase,
        long correlationId,
        string? version,
        string? reportedFormat,
        string? extension,
        string? kind,
        long sizeBytes)
        => new(
            SchemaVersion,
            NormalizeErrorCode(errorCode),
            NormalizePhase(phase),
            Math.Max(0, correlationId),
            NormalizeVersion(version),
            NormalizeFormat(reportedFormat, extension, kind),
            GetSizeBucket(sizeBytes));

    public static string Serialize(PreviewFailureDiagnostic diagnostic)
    {
        string payload = JsonSerializer.Serialize(
            new
            {
                schemaVersion = diagnostic.SchemaVersion,
                errorCode = diagnostic.ErrorCode,
                phase = diagnostic.Phase,
                correlationId = diagnostic.CorrelationId,
                version = diagnostic.Version,
                format = diagnostic.Format,
                sizeBucket = diagnostic.SizeBucket,
            },
            JsonOptions);
        if (payload.Length > MaxClipboardChars)
            throw new InvalidOperationException("Preview diagnostics exceeded the clipboard size budget.");
        return payload;
    }

    public static string GetSizeBucket(long sizeBytes)
        => sizeBytes switch
        {
            < 0 => "unknown",
            0 => "empty",
            < OneMebibyte => "lt-1-mib",
            < 16 * OneMebibyte => "1-16-mib",
            < 64 * OneMebibyte => "16-64-mib",
            < 256 * OneMebibyte => "64-256-mib",
            _ => "gte-256-mib",
        };

    private static string NormalizeErrorCode(string? value)
    {
        string normalized = value?.Trim().ToLowerInvariant() ?? "";
        return SafeErrorCode().IsMatch(normalized) ? normalized : PreviewErrorCodes.Unknown;
    }

    private static string NormalizePhase(string? value)
    {
        string normalized = value?.Trim().ToLowerInvariant() ?? "";
        return SafePhase().IsMatch(normalized) ? normalized : "unknown";
    }

    private static string NormalizeVersion(string? value)
    {
        string normalized = value?.Trim() ?? "";
        return SafeVersion().IsMatch(normalized) ? normalized : "unknown";
    }

    private static string NormalizeFormat(params string?[] candidates)
    {
        foreach (string? candidate in candidates)
        {
            string normalized = candidate?.Trim().TrimStart('.').ToLowerInvariant() ?? "";
            if (SafeFormat().IsMatch(normalized))
                return normalized;
        }
        return "unknown";
    }

    [GeneratedRegex("^[a-z0-9][a-z0-9._-]{0,63}$", RegexOptions.CultureInvariant)]
    private static partial Regex SafeErrorCode();

    [GeneratedRegex("^[a-z0-9][a-z0-9-]{0,47}$", RegexOptions.CultureInvariant)]
    private static partial Regex SafePhase();

    [GeneratedRegex("^[0-9A-Za-z][0-9A-Za-z.-]{0,63}$", RegexOptions.CultureInvariant)]
    private static partial Regex SafeVersion();

    [GeneratedRegex("^[a-z0-9][a-z0-9.+-]{0,31}$", RegexOptions.CultureInvariant)]
    private static partial Regex SafeFormat();
}
