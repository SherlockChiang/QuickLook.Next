namespace QuickLook.Next.App;

internal enum PreviewNavigationSource
{
    ExplorerOpen,
    ExplorerSwitch,
    WindowNavigation,
}

internal readonly record struct PreviewSessionSnapshot(int Generation, CancellationToken Token, string Path, PreviewNavigationSource Source);
internal readonly record struct PreviewErrorContext(PreviewSessionSnapshot Snapshot, bool CanRetry)
{
    public string Path => Snapshot.Path;
}

internal sealed class PreviewSession
{
    private CancellationTokenSource? _operationCts;
    private PreviewErrorContext? _errorContext;

    public int Generation { get; private set; }
    public string? CurrentPath { get; private set; }
    public string? PendingPath { get; private set; }
    public string? CurrentRequestId { get; private set; }
    public string? ExplorerAnchorPath { get; private set; }
    public PreviewNavigationSource Source { get; private set; } = PreviewNavigationSource.ExplorerOpen;
    public string? ActivePath => PendingPath ?? CurrentPath;
    public PreviewErrorContext? ErrorContext => _errorContext;
    public string? ErrorActionPath
        => _errorContext is PreviewErrorContext context && IsCurrentError(context)
            ? context.Path
            : null;

    public CancellationToken Token => _operationCts?.Token ?? CancellationToken.None;

    public PreviewSessionSnapshot Begin(string path, PreviewNavigationSource source)
    {
        CancelOperation();
        _operationCts = new CancellationTokenSource();
        Generation++;
        PendingPath = path;
        _errorContext = null;
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
        _errorContext = null;
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
        _errorContext = null;
    }

    public void SetRequestId(string? requestId)
        => CurrentRequestId = requestId;

    public bool IsCurrent(int generation, CancellationToken token)
        => generation == Generation && !token.IsCancellationRequested;

    public bool IsCurrent(PreviewSessionSnapshot snapshot)
        => IsCurrent(snapshot.Generation, snapshot.Token);

    public bool TryGetActiveSnapshot(string path, out PreviewSessionSnapshot snapshot)
    {
        snapshot = default;
        if (Token.IsCancellationRequested
            || string.IsNullOrWhiteSpace(path)
            || string.IsNullOrWhiteSpace(ActivePath)
            || !string.Equals(ActivePath, path, StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        snapshot = new PreviewSessionSnapshot(Generation, Token, path, Source);
        return true;
    }

    public bool TryBindError(
        PreviewSessionSnapshot snapshot,
        bool canRetry,
        out PreviewErrorContext context)
    {
        context = default;
        if (!IsCurrent(snapshot)
            || string.IsNullOrWhiteSpace(ActivePath)
            || !string.Equals(ActivePath, snapshot.Path, StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        context = new PreviewErrorContext(snapshot, canRetry);
        _errorContext = context;
        return true;
    }

    public bool IsCurrentError(PreviewErrorContext context)
        => _errorContext is PreviewErrorContext current
            && current == context
            && IsCurrent(context.Snapshot)
            && string.Equals(ActivePath, context.Path, StringComparison.OrdinalIgnoreCase);

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

    /// <summary>
    /// Explorer-originated previews stay non-activating so the shell retains keyboard focus for
    /// arrow-key selection follow-up. Content may request focus only for in-preview navigation.
    /// </summary>
    public bool ShouldActivatePreview(bool contentNeedsFocus)
        => contentNeedsFocus && Source == PreviewNavigationSource.WindowNavigation;

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
