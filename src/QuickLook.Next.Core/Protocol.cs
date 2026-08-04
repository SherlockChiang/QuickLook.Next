using System.Text.Json.Serialization;
using QuickLook.Next.Contracts;

namespace QuickLook.Next.Core;

/// <summary>
/// App ⇄ preview-host control-channel messages (line-delimited JSON over a named pipe).
/// Bulk pixels never travel here — they flow through the shared composition surface referenced by
/// <see cref="PreviewSurface"/>. Validated by Spike 1 (see spikes/spike1-composition/SPIKE1_FINDINGS.md).
///
/// Contract invariant: the App accepts at most one terminal Host message per <c>RequestId</c>.
/// It may instead stop waiting because of client cancellation, timeout, disconnect, or service
/// failure; late and duplicate Host terminals are rejected.
/// </summary>
[JsonPolymorphic(TypeDiscriminatorPropertyName = "type")]
[JsonDerivedType(typeof(Hello), "hello")]
[JsonDerivedType(typeof(HostReady), "host.ready")]
[JsonDerivedType(typeof(ParserReady), "parser.ready")]
[JsonDerivedType(typeof(ShellBrokerReady), "shell.ready")]
[JsonDerivedType(typeof(PreviewOpen), "preview.open")]
[JsonDerivedType(typeof(PreviewOpenHandle), "preview.open.handle")]
[JsonDerivedType(typeof(PreviewOpenSqliteHandles), "preview.open.sqlite-handles")]
[JsonDerivedType(typeof(PreviewSurface), "preview.surface")]
[JsonDerivedType(typeof(PreviewImageWaveform), "preview.image.waveform")]
[JsonDerivedType(typeof(PreviewImageMetadataOpen), "preview.image.metadata.open")]
[JsonDerivedType(typeof(PreviewImageMetadataReady), "preview.image.metadata.ready")]
[JsonDerivedType(typeof(PreviewImageMetadataClose), "preview.image.metadata.close")]
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
[JsonDerivedType(typeof(OfficeImageOpen), "office.image.open")]
[JsonDerivedType(typeof(OfficeImageReady), "office.image.ready")]
[JsonDerivedType(typeof(OfficeImageClose), "office.image.close")]
[JsonDerivedType(typeof(PreviewAnimationFramesOpen), "preview.animation.open")]
[JsonDerivedType(typeof(PreviewAnimationFramesReady), "preview.animation.ready")]
[JsonDerivedType(typeof(PreviewAnimationFramesClose), "preview.animation.close")]
[JsonDerivedType(typeof(ShellThumbnailOpen), "shell.thumbnail.open")]
[JsonDerivedType(typeof(ShellThumbnailReady), "shell.thumbnail.ready")]
[JsonDerivedType(typeof(ShellThumbnailClose), "shell.thumbnail.close")]
public abstract record ControlMessage;

/// <summary>
/// App → Host on connect: authenticates the launch. AppProcessId is identity metadata checked against
/// the named-pipe server PID; the host never opens the App process or duplicates handles into it.
/// </summary>
public sealed record Hello(int AppProcessId, string SessionToken) : ControlMessage;

/// <summary>Host → App once ready. AdapterLuid must match the App's compositor adapter for sharing.</summary>
public sealed record HostReady(long AdapterLuid) : ControlMessage;

/// <summary>ParserHost → App after the authenticated handshake completes.</summary>
public sealed record ParserReady : ControlMessage;

/// <summary>ShellBroker → App after the authenticated handshake completes.</summary>
public sealed record ShellBrokerReady : ControlMessage;

/// <summary>App → Host: open a path. Used for cloud fail-closed metadata and compatibility paths.</summary>
public sealed record PreviewOpen(string RequestId, string Path, FileProbe Probe) : ControlMessage
{
    public uint TargetWidth { get; init; }
    public uint TargetHeight { get; init; }
    public bool PrepareAnimation { get; init; }
}

/// <summary>App → preview host: open the exact read-only file object duplicated into the host.</summary>
public sealed record PreviewOpenHandle(
    string RequestId, long SourceHandle, long SourceLength, string LogicalPath, FileProbe Probe) : ControlMessage
{
    public uint TargetWidth { get; init; }
    public uint TargetHeight { get; init; }
    public bool PrepareAnimation { get; init; }
}

/// <summary>
/// App → ParserHost: open one exact SQLite input plus optional WAL/SHM companions already
/// duplicated into the host. An absent companion is represented only by the tuple (0, 0).
/// </summary>
public sealed record PreviewOpenSqliteHandles(
    string RequestId,
    long MainHandle,
    long MainLength,
    long WalHandle,
    long WalLength,
    long ShmHandle,
    long ShmLength,
    string LogicalPath,
    FileProbe Probe) : ControlMessage;

/// <summary>
/// RasterHost → App: a host-local composition handle value. The App pulls a duplicate through its
/// existing RasterHost process handle, then acknowledges so the host can close its transfer handle.
/// </summary>
public sealed record PreviewSurface(
    string RequestId, long SharedHandle, uint Width, uint Height, double Dpi, string Format,
    int PageIndex = -1, long PageGeneration = 0) : ControlMessage
{
    public string TransferId { get; init; } = "";
    public ImageWaveform? Waveform { get; init; }
}

/// <summary>Bounded RGB density scope derived from decoded pixels; channels are planar and row-major.</summary>
public sealed record ImageWaveform(int Width, int Height, byte[] RgbDensity);

/// <summary>RasterHost → App: optional image analysis computed after the first surface is published.</summary>
public sealed record PreviewImageWaveform(string RequestId, ImageWaveform Waveform) : ControlMessage;

/// <summary>
/// App → RasterHost: read optional metadata from an already-open exact image HANDLE. The child
/// request carries no path and fails closed when the retained parent is unavailable.
/// </summary>
public sealed record PreviewImageMetadataOpen(
    string RequestId,
    string PreviewRequestId) : ControlMessage;

/// <summary>RasterHost → App: bounded metadata from the exact retained parent image object.</summary>
public sealed record PreviewImageMetadataReady(
    string RequestId,
    string PreviewRequestId,
    ImageMetadata Metadata) : ControlMessage;

/// <summary>App → RasterHost: cancel or release an image-metadata child request.</summary>
public sealed record PreviewImageMetadataClose(string RequestId) : ControlMessage;

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

/// <summary>
/// App → ParserHost: extract one archive listing entry directly into an App-owned bounded output
/// file HANDLE. OutputHandle is transferred to ParserHost, which must adopt it before validation.
/// </summary>
public sealed record ArchiveEntryExtract(
    string RequestId,
    string ArchivePath,
    string EntryPath,
    long OutputHandle,
    long OutputCapacity) : ControlMessage
{
    /// <summary>
    /// For a local HANDLE preview, identifies the parent source retained by ParserHost. When present,
    /// ParserHost must fail closed if that exact parent is unavailable and must not fall back to ArchivePath.
    /// </summary>
    public string? ParentPreviewRequestId { get; init; }
}

/// <summary>
/// ParserHost → App: terminal successful archive entry extraction. The bytes already reside in the
/// App-owned output object supplied by <see cref="ArchiveEntryExtract"/>.
/// </summary>
public sealed record ArchiveEntryExtracted(
    string RequestId, long FileLength, string LogicalName) : ControlMessage;

/// <summary>App → ParserHost: cancel an archive entry extraction.</summary>
public sealed record ArchiveEntryExtractClose(string RequestId) : ControlMessage;

/// <summary>App → ParserHost: extract a package icon or Office embedded image into a bounded temp raster.</summary>
public sealed record HeroRasterExtract(string RequestId, string Path, string Kind) : ControlMessage
{
    public string? ParentPreviewRequestId { get; init; }
}

/// <summary>
/// ParserHost → App: a bounded BGRA raster is ready in a host-owned anonymous section.
/// The App duplicates a read-only section handle and acknowledges consumption with close.
/// </summary>
public sealed record HeroRasterExtracted(string RequestId, long SectionHandle, long PacketLength, int Width, int Height) : ControlMessage;

/// <summary>App → ParserHost: release a hero-raster section after the App has consumed it.</summary>
public sealed record HeroRasterExtractClose(string RequestId) : ControlMessage;

/// <summary>
/// App → ParserHost: decode one image referenced by an already-open Office preview. The parent
/// request binds ImageRef to the exact retained package HANDLE and prevents path fallback.
/// </summary>
public sealed record OfficeImageOpen(
    string RequestId,
    string ParentPreviewRequestId,
    string ImageRef,
    uint TargetWidth,
    uint TargetHeight) : ControlMessage;

/// <summary>
/// ParserHost → App: a bounded BGRA raster is ready in a host-owned anonymous section.
/// The App duplicates a read-only section handle and acknowledges consumption with close.
/// </summary>
public sealed record OfficeImageReady(
    string RequestId,
    long SectionHandle,
    long PacketLength,
    int Width,
    int Height) : ControlMessage;

/// <summary>App → ParserHost: release or cancel an Office-image section request.</summary>
public sealed record OfficeImageClose(string RequestId) : ControlMessage;

/// <summary>App → RasterHost: decode animation frames for the currently open parent preview.</summary>
public sealed record PreviewAnimationFramesOpen(
    string RequestId, string PreviewRequestId, uint TargetWidth, uint TargetHeight) : ControlMessage;

/// <summary>
/// RasterHost → App: a bounded animation frame packet is ready in a host-owned anonymous section.
/// The App duplicates a read-only section handle and acknowledges consumption with close.
/// </summary>
public sealed record PreviewAnimationFramesReady(
    string RequestId, string PreviewRequestId, long SectionHandle, int FrameCount, int Width, int Height, long PacketLength) : ControlMessage;

/// <summary>App → RasterHost: release an animation section after consumption.</summary>
public sealed record PreviewAnimationFramesClose(string RequestId) : ControlMessage;

/// <summary>App → ShellBroker: request one bounded thumbnail for an explicit compatibility path.</summary>
public sealed record ShellThumbnailOpen(string RequestId, string Path, int Size) : ControlMessage;

/// <summary>ShellBroker → App: a bounded BGRA packet in broker-owned temporary storage.</summary>
public sealed record ShellThumbnailReady(
    string RequestId, long FileHandle, long PacketLength, int Width, int Height) : ControlMessage;

/// <summary>App → ShellBroker: release a thumbnail packet or cancel its extraction.</summary>
public sealed record ShellThumbnailClose(string RequestId) : ControlMessage;
