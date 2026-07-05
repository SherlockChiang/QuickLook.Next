namespace QuickLook.Next.App;

internal enum PreviewNavigationSource
{
    ExplorerOpen,
    ExplorerSwitch,
    WindowNavigation,
}

internal readonly record struct PreviewSessionSnapshot(int Generation, CancellationToken Token, string Path, PreviewNavigationSource Source);

internal sealed class PreviewSession
{
    private CancellationTokenSource? _operationCts;

    public int Generation { get; private set; }
    public string? CurrentPath { get; private set; }
    public string? PendingPath { get; private set; }
    public string? CurrentRequestId { get; private set; }
    public string? ExplorerAnchorPath { get; private set; }
    public PreviewNavigationSource Source { get; private set; } = PreviewNavigationSource.ExplorerOpen;

    public CancellationToken Token => _operationCts?.Token ?? CancellationToken.None;

    public PreviewSessionSnapshot Begin(string path, PreviewNavigationSource source)
    {
        CancelOperation();
        _operationCts = new CancellationTokenSource();
        Generation++;
        PendingPath = path;
        Source = source;
        if (source is PreviewNavigationSource.ExplorerOpen or PreviewNavigationSource.ExplorerSwitch)
            ExplorerAnchorPath = path;

        return new PreviewSessionSnapshot(Generation, Token, path, source);
    }

    public PreviewSessionSnapshot BeginClose()
    {
        CancelOperation();
        _operationCts = new CancellationTokenSource();
        Generation++;
        PendingPath = null;
        return new PreviewSessionSnapshot(Generation, Token, CurrentPath ?? "", Source);
    }

    public void CommitPath(string path)
    {
        CurrentPath = path;
        PendingPath = null;
    }

    public void Clear()
    {
        CurrentPath = null;
        PendingPath = null;
        CurrentRequestId = null;
        ExplorerAnchorPath = null;
    }

    public void SetRequestId(string? requestId)
        => CurrentRequestId = requestId;

    public bool IsCurrent(int generation, CancellationToken token)
        => generation == Generation && !token.IsCancellationRequested;

    public bool IsCurrent(PreviewSessionSnapshot snapshot)
        => IsCurrent(snapshot.Generation, snapshot.Token);

    public bool IsCurrentRequest(string requestId)
        => string.Equals(CurrentRequestId, requestId, StringComparison.Ordinal);

    public bool IsCurrentPath(string? path)
        => !string.IsNullOrWhiteSpace(path)
            && !string.IsNullOrWhiteSpace(CurrentPath)
            && string.Equals(CurrentPath, path, StringComparison.OrdinalIgnoreCase);

    public bool ShouldAcceptExplorerSwitch(string path, bool previewVisible)
    {
        if (!previewVisible)
            return false;
        if (IsCurrentPath(path))
            return false;

        return Source != PreviewNavigationSource.WindowNavigation
            || string.IsNullOrWhiteSpace(ExplorerAnchorPath)
            || !string.Equals(path, ExplorerAnchorPath, StringComparison.OrdinalIgnoreCase);
    }

    public void CancelOperation()
    {
        if (_operationCts is null)
            return;

        try { _operationCts.Cancel(); }
        catch { }
        _operationCts.Dispose();
        _operationCts = null;
    }
}
