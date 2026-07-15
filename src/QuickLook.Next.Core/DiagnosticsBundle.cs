using System.IO.Compression;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace QuickLook.Next.Core;

public readonly record struct DiagnosticsLogState(bool Present, long SizeBytes);

public sealed record DiagnosticsSnapshot
{
    public required string ApplicationVersion { get; init; }
    public required Architecture ProcessArchitecture { get; init; }
    public required bool IsPackaged { get; init; }
    public required Version FrameworkVersion { get; init; }
    public required Version OsVersion { get; init; }
    public required int SettingsSchemaVersion { get; init; }
    public required string LanguageMode { get; init; }
    public required string AnimationMode { get; init; }
    public required bool NativeBridgePresent { get; init; }
    public required bool RasterHostPresent { get; init; }
    public required bool ParserHostPresent { get; init; }
    public required DiagnosticsLogState AppLog { get; init; }
    public required DiagnosticsLogState PreviousAppLog { get; init; }
    public required DiagnosticsLogState RasterHostLog { get; init; }
    public required DiagnosticsLogState PreviousRasterHostLog { get; init; }
}

public static partial class DiagnosticsBundle
{
    public const int SchemaVersion = 1;
    public const int MaxJsonBytes = 32 * 1024;
    public const int MaxReadmeBytes = 4 * 1024;
    public const int MaxBundleBytes = 64 * 1024;
    private static readonly DateTimeOffset MinimumZipTime = new(1980, 1, 1, 0, 0, 0, TimeSpan.Zero);

    public static async Task WriteAsync(
        Stream destination,
        DiagnosticsSnapshot snapshot,
        DateTimeOffset generatedUtc,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(destination);
        ArgumentNullException.ThrowIfNull(snapshot);
        if (!destination.CanWrite)
            throw new ArgumentException("Destination must be writable.", nameof(destination));

        cancellationToken.ThrowIfCancellationRequested();
        DateTimeOffset generated = generatedUtc.ToUniversalTime();
        generated = new DateTimeOffset(generated.Year, generated.Month, generated.Day, generated.Hour, generated.Minute, 0, TimeSpan.Zero);
        if (generated < MinimumZipTime)
            generated = MinimumZipTime;

        byte[] json = JsonSerializer.SerializeToUtf8Bytes(CreateDocument(snapshot, generated), new JsonSerializerOptions
        {
            WriteIndented = true,
            PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        });
        byte[] readme = Encoding.UTF8.GetBytes(ReadmeText);
        if (json.Length > MaxJsonBytes || readme.Length > MaxReadmeBytes)
            throw new InvalidOperationException("Diagnostics content exceeded its size budget.");

        using var buffer = new MemoryStream(capacity: MaxBundleBytes);
        using (var archive = new ZipArchive(buffer, ZipArchiveMode.Create, leaveOpen: true))
        {
            await WriteEntryAsync(archive, "diagnostics.json", json, generated, cancellationToken).ConfigureAwait(false);
            await WriteEntryAsync(archive, "README.txt", readme, generated, cancellationToken).ConfigureAwait(false);
        }
        if (buffer.Length > MaxBundleBytes)
            throw new InvalidOperationException("Diagnostics bundle exceeded its size budget.");
        cancellationToken.ThrowIfCancellationRequested();
        buffer.Position = 0;
        await buffer.CopyToAsync(destination, cancellationToken).ConfigureAwait(false);
    }

    private static object CreateDocument(DiagnosticsSnapshot snapshot, DateTimeOffset generated)
        => new
        {
            schemaVersion = SchemaVersion,
            generatedUtc = generated.ToString("yyyy-MM-dd'T'HH:mm:ss'Z'"),
            application = new
            {
                name = "QuickLook Next",
                version = NormalizeVersion(snapshot.ApplicationVersion),
                processArchitecture = NormalizeArchitecture(snapshot.ProcessArchitecture),
                deployment = snapshot.IsPackaged ? "packaged" : "unpackaged",
                frameworkVersion = NormalizeSystemVersion(snapshot.FrameworkVersion),
            },
            platform = new { os = "Windows", version = NormalizeSystemVersion(snapshot.OsVersion) },
            preferences = new
            {
                settingsSchemaVersion = Math.Clamp(snapshot.SettingsSchemaVersion, 0, 1000),
                languageMode = NormalizeSetting(snapshot.LanguageMode, ["system", "en-US", "zh-CN"]),
                animationMode = NormalizeSetting(snapshot.AnimationMode, ["system", "always", "still"]),
            },
            capabilities = new
            {
                nativeBridgePresent = snapshot.NativeBridgePresent,
                rasterHostPresent = snapshot.RasterHostPresent,
                parserHostPresent = snapshot.ParserHostPresent,
                windowsAppSdkSelfContained = true,
                systemCodecAvailability = "not-probed",
                formatRegistryAvailable = true,
            },
            diagnosticLogs = new
            {
                contentIncluded = false,
                known = new[]
                {
                    Log("app-current", snapshot.AppLog),
                    Log("app-previous", snapshot.PreviousAppLog),
                    Log("raster-host-current", snapshot.RasterHostLog),
                    Log("raster-host-previous", snapshot.PreviousRasterHostLog),
                },
                parserHost = new { contentIncluded = false, inventoryAvailable = false, reason = "ephemeral-isolated-log" },
            },
            privacy = new
            {
                absolutePathsIncluded = false,
                userFileNamesIncluded = false,
                userFileContentsIncluded = false,
                logContentsIncluded = false,
                settingsFileIncluded = false,
                cacheContentsIncluded = false,
                machineOrUserIdentifiersIncluded = false,
                automaticUpload = false,
            },
        };

    private static object Log(string id, DiagnosticsLogState state)
        => new { id, present = state.Present, sizeBytes = state.Present ? Math.Clamp(state.SizeBytes, 0, 16L * 1024 * 1024) : 0 };

    private static string NormalizeVersion(string value)
        => !string.IsNullOrWhiteSpace(value) && value.Length <= 64 && SafeVersion().IsMatch(value) ? value : "unknown";

    private static string NormalizeSystemVersion(Version value)
        => $"{Math.Max(0, value.Major)}.{Math.Max(0, value.Minor)}.{Math.Max(0, value.Build)}";

    private static string NormalizeArchitecture(Architecture architecture)
        => architecture switch
        {
            Architecture.X64 => "x64",
            Architecture.X86 => "x86",
            Architecture.Arm64 => "arm64",
            Architecture.Arm => "arm",
            _ => "unknown",
        };

    private static string NormalizeSetting(string value, string[] allowed)
        => allowed.Contains(value, StringComparer.Ordinal) ? value : "unknown";

    private static async Task WriteEntryAsync(
        ZipArchive archive,
        string name,
        byte[] content,
        DateTimeOffset timestamp,
        CancellationToken cancellationToken)
    {
        ZipArchiveEntry entry = archive.CreateEntry(name, CompressionLevel.Optimal);
        entry.LastWriteTime = timestamp;
        await using Stream stream = entry.Open();
        await stream.WriteAsync(content, cancellationToken).ConfigureAwait(false);
    }

    private const string ReadmeText = """
        QuickLook Next diagnostics bundle

        This bundle contains a small application and system summary for troubleshooting.
        It does not contain user file names, user file contents, absolute paths, log contents,
        settings files, caches, machine or user identifiers, or ParserHost work files.

        No data was uploaded. Review diagnostics.json before attaching this bundle to a report.
        System codec availability was not probed.
        """;

    [GeneratedRegex(@"^[0-9A-Za-z][0-9A-Za-z.-]*$", RegexOptions.CultureInvariant)]
    private static partial Regex SafeVersion();
}
