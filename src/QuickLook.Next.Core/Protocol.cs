using System.Text.Json.Serialization;
using QuickLook.Next.Contracts;

namespace QuickLook.Next.Core;

/// <summary>
/// App ⇄ RasterHost control-channel messages (line-delimited JSON over a named pipe).
/// Bulk pixels never travel here — they flow through the shared composition surface referenced by
/// <see cref="PreviewSurface"/>. Validated by Spike 1 (see spikes/spike1-composition/SPIKE1_FINDINGS.md).
///
/// Contract invariant: every <c>RequestId</c> opened with <see cref="PreviewOpen"/> terminates in
/// exactly one of <see cref="PreviewReady"/> | <see cref="PreviewError"/> | (client-side) timeout.
/// </summary>
[JsonPolymorphic(TypeDiscriminatorPropertyName = "type")]
[JsonDerivedType(typeof(Hello), "hello")]
[JsonDerivedType(typeof(HostReady), "host.ready")]
[JsonDerivedType(typeof(PreviewOpen), "preview.open")]
[JsonDerivedType(typeof(PreviewSurface), "preview.surface")]
[JsonDerivedType(typeof(PreviewReady), "preview.ready")]
[JsonDerivedType(typeof(PreviewError), "preview.error")]
[JsonDerivedType(typeof(PreviewResize), "preview.resize")]
[JsonDerivedType(typeof(PreviewPageOpen), "preview.page.open")]
[JsonDerivedType(typeof(PreviewPageClose), "preview.page.close")]
[JsonDerivedType(typeof(PreviewClose), "preview.close")]
public abstract record ControlMessage;

/// <summary>App → Host on connect: lets the host duplicate the shared surface handle into the App.</summary>
public sealed record Hello(int AppProcessId) : ControlMessage;

/// <summary>Host → App once ready. AdapterLuid must match the App's compositor adapter for sharing.</summary>
public sealed record HostReady(long AdapterLuid) : ControlMessage;

/// <summary>App → Host: open a file. Probe comes from the Rust native layer.</summary>
public sealed record PreviewOpen(string RequestId, string Path, FileProbe Probe) : ControlMessage;

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
    public string? TextContent { get; init; }
    public string? TextFormat { get; init; }
    public string? TextLanguage { get; init; }
    public string? MediaPath { get; init; }
    public PreviewListing? Listing { get; init; }
    public OfficeLayout? OfficeLayout { get; init; }
}

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
