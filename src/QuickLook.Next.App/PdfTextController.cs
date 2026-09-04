using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal enum PdfTextLoadStatus
{
    Unavailable,
    Idle,
    Loading,
    Ready,
    Partial,
    NoText,
    Encrypted,
    Failed,
}

internal readonly record struct PdfTextSearchState(
    int Current,
    int Count,
    int PageIndex,
    PdfTextLoadStatus LoadStatus,
    bool Truncated,
    int FailedPages);

internal sealed class PdfTextController
{
    public const int MaxQueryChars = 256;
    public const int MaxMatches = TextSearchIndex.MaxMatches;

    private readonly Func<SafeFileHandle, long, string, CancellationToken, Task<PdfTextDocument?>> _extract;
    private readonly List<PdfTextMatch> _matches = [];
    private SafeFileHandle? _source;
    private long _sourceLength;
    private string? _logicalPath;
    private PdfTextDocument? _document;
    private CancellationTokenSource? _loadCts;
    private Task<PdfTextSearchState>? _loadTask;
    private string _query = "";
    private int _currentMatch = -1;
    private int _sourceVersion;
    private PdfTextLoadStatus _loadStatus = PdfTextLoadStatus.Unavailable;

    public PdfTextController(
        Func<SafeFileHandle, long, string, CancellationToken, Task<PdfTextDocument?>> extract)
    {
        _extract = extract;
    }

    public PdfTextSearchState State => new(
        _currentMatch >= 0 ? _currentMatch + 1 : 0,
        _matches.Count,
        _currentMatch >= 0 ? _matches[_currentMatch].PageIndex : -1,
        _loadStatus,
        _document?.Truncated == true,
        _document?.FailedPages ?? 0);

    public void SetSource(SafeFileHandle source, long sourceLength, string logicalPath)
    {
        ArgumentNullException.ThrowIfNull(source);
        if (source.IsInvalid || source.IsClosed)
            throw new ArgumentException("A valid retained PDF handle is required.", nameof(source));
        if (sourceLength is < 0 or > NativeAbi.MaxPdfTextInputBytes)
            throw new ArgumentOutOfRangeException(nameof(sourceLength));
        if (!Path.GetExtension(logicalPath).Equals(".pdf", StringComparison.OrdinalIgnoreCase))
            throw new ArgumentException("A PDF logical name is required.", nameof(logicalPath));

        Clear();
        _source = source;
        _sourceLength = sourceLength;
        _logicalPath = logicalPath;
        _loadStatus = PdfTextLoadStatus.Idle;
    }

    public Task<PdfTextSearchState> EnsureLoadedAsync(CancellationToken cancellationToken)
    {
        if (_document is not null || _loadStatus is PdfTextLoadStatus.Encrypted or PdfTextLoadStatus.NoText)
            return Task.FromResult(State);
        if (_source is null || _logicalPath is null)
            return Task.FromResult(State);
        if (_loadTask is not null)
            return _loadTask.WaitAsync(cancellationToken);

        int version = _sourceVersion;
        var loadCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        _loadCts = loadCts;
        Task<PdfTextSearchState> loadTask = LoadCoreAsync(
            version,
            _source,
            _sourceLength,
            _logicalPath,
            loadCts);
        _loadTask = loadTask;
        _ = TrackLoadCompletionAsync(loadTask, loadCts);
        return loadTask.WaitAsync(cancellationToken);
    }

    public PdfTextSearchState SetSearchQuery(string query)
    {
        _query = query.Trim();
        if (_query.Length > MaxQueryChars)
            _query = _query[..MaxQueryChars];
        RebuildMatches();
        return State;
    }

    public PdfTextSearchState MoveSearch(int delta)
    {
        if (_matches.Count == 0)
            return State;
        _currentMatch = (_currentMatch + delta + _matches.Count) % _matches.Count;
        return State;
    }

    public PdfTextSearchState ClearSearch()
    {
        _query = "";
        _matches.Clear();
        _currentMatch = -1;
        return State;
    }

    public string? GetPageText(int pageIndex)
    {
        string? text = _document?.Pages.FirstOrDefault(page => page.PageIndex == pageIndex)?.Text;
        return string.IsNullOrWhiteSpace(text) ? null : text;
    }

    public void Clear()
    {
        _sourceVersion++;
        try { _loadCts?.Cancel(); } catch { }
        _loadCts = null;
        _loadTask = null;
        try { _source?.Dispose(); } catch { }
        _source = null;
        _sourceLength = 0;
        _logicalPath = null;
        _document = null;
        _loadStatus = PdfTextLoadStatus.Unavailable;
        ClearSearch();
    }

    internal PdfTextSearchState ApplyDocument(PdfTextDocument document)
    {
        ArgumentNullException.ThrowIfNull(document);
        _document = document;
        _loadStatus = document.Status switch
        {
            "ready" => PdfTextLoadStatus.Ready,
            "partial" => PdfTextLoadStatus.Partial,
            "noText" => PdfTextLoadStatus.NoText,
            "encrypted" => PdfTextLoadStatus.Encrypted,
            _ => PdfTextLoadStatus.Failed,
        };
        RebuildMatches();
        return State;
    }

    private async Task<PdfTextSearchState> LoadCoreAsync(
        int version,
        SafeFileHandle source,
        long sourceLength,
        string logicalPath,
        CancellationTokenSource loadCts)
    {
        _loadStatus = PdfTextLoadStatus.Loading;
        try
        {
            using SafeFileHandle lease = WindowsHandleTransfer.ReopenReadOnlyFile(source, sourceLength);
            PdfTextDocument? document = await _extract(
                lease,
                sourceLength,
                logicalPath,
                loadCts.Token);
            if (version != _sourceVersion)
                return State;
            if (document is null)
            {
                _loadStatus = PdfTextLoadStatus.Failed;
                return State;
            }

            PdfTextSearchState state = ApplyDocument(document);
            try { _source?.Dispose(); } catch { }
            _source = null;
            return state;
        }
        catch (OperationCanceledException)
        {
            if (version == _sourceVersion)
                _loadStatus = PdfTextLoadStatus.Idle;
            return State;
        }
        catch
        {
            if (version == _sourceVersion)
                _loadStatus = PdfTextLoadStatus.Failed;
            return State;
        }
    }

    private async Task TrackLoadCompletionAsync(
        Task<PdfTextSearchState> loadTask,
        CancellationTokenSource loadCts)
    {
        try
        {
            await loadTask;
        }
        finally
        {
            if (ReferenceEquals(_loadTask, loadTask))
                _loadTask = null;
            if (ReferenceEquals(_loadCts, loadCts))
                _loadCts = null;
            loadCts.Dispose();
        }
    }

    private void RebuildMatches()
    {
        _matches.Clear();
        _currentMatch = -1;
        if (_document is null || _query.Length == 0)
            return;

        foreach (PdfTextPage page in _document.Pages)
        {
            foreach (int start in TextSearchIndex.FindMatches(page.Text, _query))
            {
                _matches.Add(new PdfTextMatch(page.PageIndex));
                if (_matches.Count >= MaxMatches)
                    break;
            }
            if (_matches.Count >= MaxMatches)
                break;
        }
        if (_matches.Count > 0)
            _currentMatch = 0;
    }

    private readonly record struct PdfTextMatch(int PageIndex);
}
