namespace QuickLook.Next.App;

// The App invokes Queue, Reset, and the queued callback on the UI thread.
internal sealed class PdfViewportUpdateScheduler(Func<Action, bool> tryEnqueue, Action update)
{
    private long _version;
    private bool _queued;

    public void Queue()
    {
        if (_queued)
            return;

        long version = _version;
        _queued = true;
        if (!tryEnqueue(() =>
        {
            if (version != _version)
                return;

            _queued = false;
            update();
        }))
        {
            _queued = false;
        }
    }

    public void Reset()
    {
        _version++;
        _queued = false;
    }
}
