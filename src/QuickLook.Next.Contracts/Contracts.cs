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

    /// <summary>Structured Markdown AST produced by the Rust native parser and rendered by WinUI.</summary>
    public PreviewMarkdown? Markdown { get; init; }

    /// <summary>
    /// Approximate Office document layout produced by the Rust native parser. This is a preview model,
    /// not a full Office rendering engine; PPT/XLSX favor usable layout reconstruction over perfect parity.
    /// </summary>
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
    public string? BackgroundColor { get; init; }
    public int FreezeRows { get; init; }
    public int FreezeColumns { get; init; }
    public OfficeCell[] Cells { get; init; } = [];
    public OfficeLayoutItem[] Items { get; init; } = [];
}

public sealed record OfficeCell(int Row, int Column, string Text)
{
    public double X { get; init; }
    public double Y { get; init; }
    public double Width { get; init; }
    public double Height { get; init; }
    public int RowSpan { get; init; } = 1;
    public int ColumnSpan { get; init; } = 1;
    public string? NumberFormat { get; init; }
}

public sealed record OfficeLayoutItem(string Kind)
{
    public double X { get; init; }
    public double Y { get; init; }
    public double Width { get; init; }
    public double Height { get; init; }
    public string? Text { get; init; }
    public string? Shape { get; init; }
    public string? FillColor { get; init; }
    public string? StrokeColor { get; init; }
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

public sealed record ImageMetadata
{
    public string? Make { get; init; }
    public string? Model { get; init; }
    public string? DateTime { get; init; }
    public uint? Width { get; init; }
    public uint? Height { get; init; }
    public ushort? Orientation { get; init; }
    public string? LensMake { get; init; }
    public string? LensModel { get; init; }
    public string? Software { get; init; }
    public double? FNumber { get; init; }
    public double? MaxAperture { get; init; }
    public double? ExposureTime { get; init; }
    public uint? Iso { get; init; }
    public double? FocalLength { get; init; }
    public uint? FocalLengthIn35mmFilm { get; init; }
    public double? ExposureBias { get; init; }
    public ushort? ExposureProgram { get; init; }
    public ushort? ExposureMode { get; init; }
    public ushort? MeteringMode { get; init; }
    public ushort? Flash { get; init; }
    public ushort? WhiteBalance { get; init; }
    public ushort? LightSource { get; init; }
    public double? DigitalZoomRatio { get; init; }
    public double? SubjectDistance { get; init; }
    public ushort? Contrast { get; init; }
    public ushort? Saturation { get; init; }
    public ushort? Sharpness { get; init; }
    public ushort? GainControl { get; init; }
    public ushort? ColorSpace { get; init; }
    public string? ExifVersion { get; init; }
    public string? CameraSerial { get; init; }
    public string? LensSerial { get; init; }
    public double? Latitude { get; init; }
    public double? Longitude { get; init; }
    public double? Altitude { get; init; }
    public double? Direction { get; init; }
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

public sealed record PreviewMarkdown
{
    public PreviewMarkdownBlock[] Blocks { get; init; } = [];
    public bool IsPartial { get; init; }
}

public sealed record PreviewMarkdownBlock(string Kind)
{
    public int Level { get; init; }
    public string Text { get; init; } = "";
    public string Language { get; init; } = "";
    public PreviewMarkdownInline[] Inlines { get; init; } = [];
    public PreviewMarkdownBlock[] Children { get; init; } = [];
    public string[] TableHeaders { get; init; } = [];
    public string[][] TableRows { get; init; } = [];
}

public sealed record PreviewMarkdownInline(string Kind)
{
    public string Text { get; init; } = "";
    public string Url { get; init; } = "";
    public PreviewMarkdownInline[] Children { get; init; } = [];
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
