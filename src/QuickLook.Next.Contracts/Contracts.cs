namespace QuickLook.Next.Contracts;

/// <summary>
/// Cheap, host-produced facts about a file. Produced by the Rust native layer (type/magic/metadata)
/// and shipped to preview components; providers consume it for routing/CanHandle and never redo shell/IO probing.
/// <see cref="MagicPrefix"/> is a small immutable-by-convention prefix snapshot; hot-path probing should not
/// mutate or retain the backing array beyond the current preview request.
/// </summary>
public sealed record FileProbe(string Path, string Extension, byte[] MagicPrefix)
{
    /// <summary>Coarse type from the native layer: folder | image | pdf | text | archive | binary | unknown.</summary>
    public string Kind { get; init; } = "unknown";
    public long Size { get; init; }
    public long ModifiedUnix { get; init; }
}

/// <summary>
/// Legacy .NET plugin result. The default preview hot path uses Rust/native JSON, Rust/native raster
/// buffers, and RasterHost shared surfaces instead of this large managed payload contract.
/// </summary>
public sealed record PreviewResult(string Kind, string Title)
{
    /// <summary>Provider's preferred initial size in DIPs, if any.</summary>
    public double PreferredWidth { get; init; }
    public double PreferredHeight { get; init; }

    /// <summary>
    /// Legacy plugin-only decoded pixels: premultiplied BGRA, row-major, stride =
    /// <see cref="PixelWidth"/> * 4. Null = info-only. New raster preview paths should use the
    /// native raster ABI plus shared surfaces to avoid LOH-sized managed arrays.
    /// </summary>
    [Obsolete("Legacy .NET plugin-only payload. Use Rust/native raster ABI plus shared surfaces for hot paths.")]
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

    /// <summary>Structured delimited table preview produced by the Rust native parser.</summary>
    public PreviewTable? Table { get; init; }

    /// <summary>Approximate Office document layout produced by the Rust native parser.</summary>
    public OfficeLayout? OfficeLayout { get; init; }
}

public sealed record OfficeLayout(string LayoutKind)
{
    public double Width { get; init; }
    public double Height { get; init; }
    public OfficePage[] Pages { get; init; } = [];
}

public sealed record OfficePage(string Title)
{
    public int Index { get; init; }
    public double Width { get; init; }
    public double Height { get; init; }
    public OfficeCell[] Cells { get; init; } = [];
    public OfficeLayoutItem[] Items { get; init; } = [];
}

public sealed record OfficeCell(int Row, int Column, string Text)
{
    public double X { get; init; }
    public double Y { get; init; }
    public double Width { get; init; }
    public double Height { get; init; }
}

public sealed record OfficeLayoutItem(string Kind)
{
    public double X { get; init; }
    public double Y { get; init; }
    public double Width { get; init; }
    public double Height { get; init; }
    public string? Text { get; init; }
    public string? ImageName { get; init; }
    public string? MimeType { get; init; }
    public string? ImageBase64 { get; init; }
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

public sealed record PreviewTable(string Format)
{
    public string Delimiter { get; init; } = ",";
    public string[] Headers { get; init; } = [];
    public PreviewTableRow[] Rows { get; init; } = [];
    public int TotalRows { get; init; }
    public int TotalColumns { get; init; }
    public bool IsPartial { get; init; }
}

public sealed record PreviewTableRow(string[] Cells);

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
/// Legacy .NET preview provider contract. It is kept so old provider source remains readable, but default
/// releases do not discover or load these providers; new preview logic should be implemented in Rust/native
/// preview code and rendered by the WinUI shell/RasterHost.
/// </summary>
public interface IPreviewProvider
{
    bool CanHandle(FileProbe probe);
    Task<PreviewResult> OpenAsync(string path, FileProbe probe, IPreviewContext context);
    ValueTask CloseAsync() => ValueTask.CompletedTask;
}
