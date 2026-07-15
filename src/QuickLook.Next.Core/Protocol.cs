using System.Text.Json.Serialization;
using QuickLook.Next.Contracts;

namespace QuickLook.Next.Core;

/// <summary>
/// App ⇄ preview-host control-channel messages (line-delimited JSON over a named pipe).
/// Bulk pixels never travel here — they flow through the shared composition surface referenced by
/// <see cref="PreviewSurface"/>. Validated by Spike 1 (see spikes/spike1-composition/SPIKE1_FINDINGS.md).
///
/// Contract invariant: every <c>RequestId</c> opened with <see cref="PreviewOpen"/> terminates in
/// exactly one of <see cref="PreviewReady"/> | <see cref="PreviewError"/> | (client-side) timeout.
/// </summary>
[JsonPolymorphic(TypeDiscriminatorPropertyName = "type")]
[JsonDerivedType(typeof(Hello), "hello")]
[JsonDerivedType(typeof(HostReady), "host.ready")]
[JsonDerivedType(typeof(ParserReady), "parser.ready")]
[JsonDerivedType(typeof(PreviewOpen), "preview.open")]
[JsonDerivedType(typeof(PreviewOpenHandle), "preview.open.handle")]
[JsonDerivedType(typeof(PreviewSurface), "preview.surface")]
[JsonDerivedType(typeof(PreviewSurfaceRelease), "preview.surface.release")]
[JsonDerivedType(typeof(PreviewReady), "preview.ready")]
[JsonDerivedType(typeof(PreviewError), "preview.error")]
[JsonDerivedType(typeof(PreviewResize), "preview.resize")]
[JsonDerivedType(typeof(PreviewPageOpen), "preview.page.open")]
[JsonDerivedType(typeof(PreviewPageClose), "preview.page.close")]
[JsonDerivedType(typeof(PreviewPageError), "preview.page.error")]
[JsonDerivedType(typeof(PreviewClose), "preview.close")]
[JsonDerivedType(typeof(ArchiveEntryExtract), "archive.entry.extract")]
[JsonDerivedType(typeof(ArchiveEntryExtracted), "archive.entry.extracted")]
[JsonDerivedType(typeof(ArchiveEntryExtractClose), "archive.entry.extract.close")]
[JsonDerivedType(typeof(HeroRasterExtract), "hero.raster.extract")]
[JsonDerivedType(typeof(HeroRasterExtracted), "hero.raster.extracted")]
[JsonDerivedType(typeof(HeroRasterExtractClose), "hero.raster.extract.close")]
[JsonDerivedType(typeof(PreviewAnimationFramesOpen), "preview.animation.open")]
[JsonDerivedType(typeof(PreviewAnimationFramesReady), "preview.animation.ready")]
[JsonDerivedType(typeof(PreviewAnimationFramesClose), "preview.animation.close")]
public abstract record ControlMessage;

/// <summary>App → Host on connect: authenticates the launch and lets the host duplicate surface handles into the App.</summary>
public sealed record Hello(int AppProcessId, string SessionToken) : ControlMessage;

/// <summary>Host → App once ready. AdapterLuid must match the App's compositor adapter for sharing.</summary>
public sealed record HostReady(long AdapterLuid) : ControlMessage;

/// <summary>ParserHost → App after the authenticated handshake completes.</summary>
public sealed record ParserReady : ControlMessage;

/// <summary>App → Host: open a path. Used for cloud fail-closed metadata and compatibility paths.</summary>
public sealed record PreviewOpen(string RequestId, string Path, FileProbe Probe) : ControlMessage
{
    public uint TargetWidth { get; init; }
    public uint TargetHeight { get; init; }
}

/// <summary>App → ParserHost: open the exact read-only file object duplicated into the host.</summary>
public sealed record PreviewOpenHandle(
    string RequestId, long SourceHandle, long SourceLength, string LogicalPath, FileProbe Probe) : ControlMessage;

/// <summary>RasterHost → App: a host-local composition handle that the App must copy and release.</summary>
public sealed record PreviewSurface(
    string RequestId, long SharedHandle, uint Width, uint Height, double Dpi, string Format,
    int PageIndex = -1, long PageGeneration = 0) : ControlMessage
{
    public string TransferId { get; init; } = "";
}

/// <summary>App → RasterHost: the host-local surface handle was copied or rejected and can be closed.</summary>
public sealed record PreviewSurfaceRelease(string TransferId) : ControlMessage;

/// <summary>Host → App: terminal success for a RequestId.</summary>
public sealed record PreviewReady(
    string RequestId, string Kind, string Title, double PreferredWidth, double PreferredHeight) : ControlMessage
{
    public int PageCount { get; init; }
    public double PageWidth { get; init; }
    public double PageHeight { get; init; }
    public PdfPageGeometry[]? PdfPageGeometries { get; init; }
    public string? TextContent { get; init; }
    public string? TextFormat { get; init; }
    public string? TextLanguage { get; init; }
    public string? MediaPath { get; init; }
    public PreviewListing? Listing { get; init; }
    public PreviewTable? Table { get; init; }
    public PreviewMarkdown? Markdown { get; init; }
    public OfficeLayout? OfficeLayout { get; init; }
}

/// <summary>Logical dimensions of one PDF page, in the PDF renderer's native units.</summary>
public readonly record struct PdfPageGeometry(double Width, double Height);

/// <summary>Host → App: terminal failure for a RequestId.</summary>
public sealed record PreviewError(string RequestId, string Message) : ControlMessage
{
    public string? Code { get; init; }
    public string? Format { get; init; }
}

/// <summary>App → Host: the preview region resized; host reallocates and emits a fresh PreviewSurface.</summary>
public sealed record PreviewResize(string RequestId, uint Width, uint Height, double Dpi) : ControlMessage;

/// <summary>App → Host: render one page from an already-open document preview.</summary>
public sealed record PreviewPageOpen(string RequestId, int PageIndex, long PageGeneration, double Scale) : ControlMessage;

/// <summary>App → Host: a page scrolled out of the keep-alive window; release its GPU surface.</summary>
public sealed record PreviewPageClose(string RequestId, int PageIndex, long PageGeneration) : ControlMessage;

/// <summary>Host → App: one requested page failed before publishing a surface.</summary>
public sealed record PreviewPageError(
    string RequestId, int PageIndex, long PageGeneration, bool TimedOut, string Message) : ControlMessage;

/// <summary>App → Host: tear down a preview.</summary>
public sealed record PreviewClose(string RequestId) : ControlMessage;

/// <summary>App → ParserHost: extract one archive listing entry into the native bounded temp cache.</summary>
public sealed record ArchiveEntryExtract(string RequestId, string ArchivePath, string EntryPath) : ControlMessage;

/// <summary>ParserHost → App: terminal successful archive entry extraction.</summary>
public sealed record ArchiveEntryExtracted(
    string RequestId, long FileHandle, long FileLength, string LogicalName) : ControlMessage;

/// <summary>App → ParserHost: cancel an archive entry extraction.</summary>
public sealed record ArchiveEntryExtractClose(string RequestId) : ControlMessage;

/// <summary>App → ParserHost: extract a package icon or Office embedded image into a bounded temp raster.</summary>
public sealed record HeroRasterExtract(string RequestId, string Path, string Kind) : ControlMessage
{
    public string? ParentPreviewRequestId { get; init; }
}

/// <summary>ParserHost → App: a bounded BGRA raster is ready at TempPath; pixels never use the control pipe.</summary>
public sealed record HeroRasterExtracted(string RequestId, long FileHandle, long PacketLength, int Width, int Height) : ControlMessage;

/// <summary>App → ParserHost: release a hero-raster temp handoff after the App has consumed it.</summary>
public sealed record HeroRasterExtractClose(string RequestId) : ControlMessage;

/// <summary>App → RasterHost: decode animation frames for the currently open parent preview.</summary>
public sealed record PreviewAnimationFramesOpen(
    string RequestId, string PreviewRequestId, uint TargetWidth, uint TargetHeight) : ControlMessage;

/// <summary>RasterHost → App: a bounded animation frame packet is ready in host-owned temporary storage.</summary>
public sealed record PreviewAnimationFramesReady(
    string RequestId, string PreviewRequestId, long FileHandle, int FrameCount, int Width, int Height, long PacketLength) : ControlMessage;

/// <summary>App → RasterHost: release an animation frame packet after consumption.</summary>
public sealed record PreviewAnimationFramesClose(string RequestId) : ControlMessage;
