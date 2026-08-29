namespace QuickLook.Next.App;

/// <summary>
/// Suppresses the space-toggle close for a just-revealed preview so the same input gesture — a
/// space press that raced the async reveal, or Explorer re-emitting the same selection — cannot
/// immediately undo it. The latch is per path, is cleared explicitly on close, and only settles
/// within a short wall-clock bound so a genuine later toggle still closes.
/// </summary>
internal sealed class DuplicateOpenCloseGuard(int settleWindowMs)
{
    private string? _revealedPath;
    private long _revealedTick;

    public void NoteReveal(string? path, long tick)
    {
        _revealedPath = path;
        _revealedTick = tick;
    }

    public void Clear()
    {
        _revealedPath = null;
        _revealedTick = 0;
    }

    public bool ShouldIgnoreToggleClose(string? path, long tick)
    {
        if (string.IsNullOrWhiteSpace(path)
            || !string.Equals(_revealedPath, path, StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        long elapsed = tick - _revealedTick;
        return elapsed >= 0 && elapsed < settleWindowMs;
    }
}
