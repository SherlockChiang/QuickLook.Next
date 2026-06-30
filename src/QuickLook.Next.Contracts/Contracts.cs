namespace QuickLook.Next.Contracts;

/// <summary>
/// Cheap, host-produced facts about a file. Produced by the Rust native layer (type/magic/metadata)
/// and shipped to preview components; providers consume it for routing/CanHandle and never redo shell/IO probing.
/// </summary>
public sealed record FileProbe(string Path, string Extension, byte[] MagicPrefix)
{
    /// <summary>Coarse type from the native layer: folder | image | pdf | text | archive | binary | unknown.</summary>
    public string Kind { get; init; } = "unknown";
    public long Size { get; init; }
    public long ModifiedUnix { get; init; }
}

/// <summary>
/// Result of opening a file for preview. A raster provider (image/PDF/…) decodes the content to
/// premultiplied BGRA and the host uploads it into the shared composition surface; an info provider
/// leaves <see cref="Bgra"/> null and only supplies metadata.
/// </summary>
public sealed record PreviewResult(string Kind, string Title)
{
    /// <summary>Provider's preferred initial size in DIPs, if any.</summary>
    public double PreferredWidth { get; init; }
    public double PreferredHeight { get; init; }

    /// <summary>Decoded pixels: premultiplied BGRA, row-major, stride = <see cref="PixelWidth"/> * 4. Null = info-only.</summary>
    public byte[]? Bgra { get; init; }
    public int PixelWidth { get; init; }
    public int PixelHeight { get; init; }

    /// <summary>Text preview payload, capped by the provider. Null means this is not a text preview.</summary>
    public string? Text { get; init; }
    public string? TextFormat { get; init; }
    public string? TextLanguage { get; init; }

    /// <summary>Full path to a media file the host should pass back for playback (video/audio). Null = not a media file.</summary>
    public string? MediaPath { get; init; }

    /// <summary>Structured file/folder listing for archive and directory previews.</summary>
    public PreviewListing? Listing { get; init; }
}

public sealed record PreviewListing(string RootName, string RootPath, string ListingKind)
{
    public string Summary { get; init; } = "";
    public bool IsPartial { get; init; }
    public PreviewListingItem[] Items { get; init; } = [];
}

public sealed record PreviewListingItem(string Name, string Path, string ParentPath, bool IsFolder)
{
    public long Size { get; init; }
    public long PackedSize { get; init; }
    public long ModifiedUnix { get; init; }
    public string Type { get; init; } = "";
    public string? NativePath { get; init; }
}

/// <summary>
/// Host-provided surface a provider uses while opening: report status/errors, and (in the full host)
/// obtain a composition container to mount its visual into. Kept minimal in the scaffold.
/// </summary>
public interface IPreviewContext
{
    void ReportStatus(string status);
    CancellationToken Cancellation { get; }
}

/// <summary>
/// Implemented by every viewer plugin. Loaded on demand into a collectible AssemblyLoadContext inside
/// Legacy plugin host. Manifest-level routing (id/priority/extensions) selects candidates without loading;
/// <see cref="CanHandle"/> is the authoritative check that may inspect the magic prefix.
/// </summary>
public interface IPreviewProvider
{
    bool CanHandle(FileProbe probe);
    Task<PreviewResult> OpenAsync(string path, FileProbe probe, IPreviewContext context);
}
