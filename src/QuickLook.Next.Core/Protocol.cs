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
[JsonDerivedType(typeof(PreviewSurface), "preview.surface")]
[JsonDerivedType(typeof(PreviewReady), "preview.ready")]
[JsonDerivedType(typeof(PreviewError), "preview.error")]
[JsonDerivedType(typeof(PreviewResize), "preview.resize")]
[JsonDerivedType(typeof(PreviewPageOpen), "preview.page.open")]
[JsonDerivedType(typeof(PreviewPageClose), "preview.page.close")]
[JsonDerivedType(typeof(PreviewClose), "preview.close")]
[JsonDerivedType(typeof(ArchiveEntryExtract), "archive.entry.extract")]
[JsonDerivedType(typeof(ArchiveEntryExtracted), "archive.entry.extracted")]
[JsonDerivedType(typeof(ArchiveEntryExtractClose), "archive.entry.extract.close")]
[JsonDerivedType(typeof(HeroRasterExtract), "hero.raster.extract")]
[JsonDerivedType(typeof(HeroRasterExtracted), "hero.raster.extracted")]
[JsonDerivedType(typeof(HeroRasterExtractClose), "hero.raster.extract.close")]
public abstract record ControlMessage;

/// <summary>App → Host on connect: authenticates the launch and lets the host duplicate surface handles into the App.</summary>
public sealed record Hello(int AppProcessId, string SessionToken) : ControlMessage;

/// <summary>Host → App once ready. AdapterLuid must match the App's compositor adapter for sharing.</summary>
public sealed record HostReady(long AdapterLuid) : ControlMessage;

/// <summary>ParserHost → App after the authenticated handshake completes.</summary>
public sealed record ParserReady : ControlMessage;

/// <summary>App → Host: open a file. Probe comes from the Rust native layer.</summary>
public sealed record PreviewOpen(string RequestId, string Path, FileProbe Probe) : ControlMessage
{
    public uint TargetWidth { get; init; }
    public uint TargetHeight { get; init; }
}

/// <summary>Host → App: the shared composition surface handle (already duplicated into the App process).</summary>
public sealed record PreviewSurface(
    string RequestId, long SharedHandle, uint Width, uint Height, double Dpi, string Format, int PageIndex = -1) : ControlMessage;

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
public sealed record PreviewError(string RequestId, string Message) : ControlMessage;

/// <summary>App → Host: the preview region resized; host reallocates and emits a fresh PreviewSurface.</summary>
public sealed record PreviewResize(string RequestId, uint Width, uint Height, double Dpi) : ControlMessage;

/// <summary>App → Host: render one page from an already-open document preview.</summary>
public sealed record PreviewPageOpen(string RequestId, int PageIndex, double Scale) : ControlMessage;

/// <summary>App → Host: a page scrolled out of the keep-alive window; release its GPU surface.</summary>
public sealed record PreviewPageClose(string RequestId, int PageIndex) : ControlMessage;

/// <summary>App → Host: tear down a preview.</summary>
public sealed record PreviewClose(string RequestId) : ControlMessage;

/// <summary>App → ParserHost: extract one archive listing entry into the native bounded temp cache.</summary>
public sealed record ArchiveEntryExtract(string RequestId, string ArchivePath, string EntryPath) : ControlMessage;

/// <summary>ParserHost → App: terminal successful archive entry extraction.</summary>
public sealed record ArchiveEntryExtracted(string RequestId, string TempPath) : ControlMessage;

/// <summary>App → ParserHost: cancel an archive entry extraction.</summary>
public sealed record ArchiveEntryExtractClose(string RequestId) : ControlMessage;

/// <summary>App → ParserHost: extract a package icon or Office embedded image into a bounded temp raster.</summary>
public sealed record HeroRasterExtract(string RequestId, string Path, string Kind) : ControlMessage;

/// <summary>ParserHost → App: a bounded BGRA raster is ready at TempPath; pixels never use the control pipe.</summary>
public sealed record HeroRasterExtracted(string RequestId, string TempPath, int Width, int Height) : ControlMessage;

/// <summary>App → ParserHost: release a hero-raster temp handoff after the App has consumed it.</summary>
public sealed record HeroRasterExtractClose(string RequestId) : ControlMessage;
