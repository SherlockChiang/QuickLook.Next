namespace QuickLook.Next.App;

internal sealed class NativeThumbnailScheduler
{
    private const int MaxQueuedRequests = 512;
    private const int ForegroundBurstLimit = 8;
    private readonly NativeBridge _native;
    private readonly object _gate = new();
    private readonly LinkedList<Request> _foreground = new();
    private readonly LinkedList<Request> _background = new();
    private bool _running;
    private int _foregroundBurst;

    public NativeThumbnailScheduler(NativeBridge native)
    {
        _native = native;
    }

    public Task<NativeRasterImage?> LoadAsync(
        string path,
        int size,
        NativeThumbnailPriority priority,
        bool cacheOnly,
        CancellationToken token)
    {
        token.ThrowIfCancellationRequested();
        var request = new Request(this, path, size, cacheOnly, token);

        lock (_gate)
        {
            if (request.Task.IsCompleted)
                return request.Task;

            if (_foreground.Count + _background.Count >= MaxQueuedRequests)
            {
                if (priority == NativeThumbnailPriority.Background || !DropOldestBackground())
                {
                    request.TrySetResult(null);
                    return request.Task;
                }
            }

            if (priority == NativeThumbnailPriority.Foreground)
                request.Node = _foreground.AddLast(request);
            else
                request.Node = _background.AddLast(request);

            if (!_running)
            {
                _running = true;
                _ = Task.Run(ProcessAsync);
            }
        }

        return request.Task;
    }

    private Task ProcessAsync()
    {
        while (true)
        {
            Request? request = DequeueNext();
            if (request is null)
                return Task.CompletedTask;

            if (request.Token.IsCancellationRequested)
            {
                request.TrySetCanceled();
                continue;
            }

            try
            {
                request.TrySetResult(_native.TryGetThumbnail(request.Path, request.Size, request.CacheOnly, request.Token));
            }
            catch (OperationCanceledException)
            {
                request.TrySetCanceled();
            }
            catch
            {
                request.TrySetResult(null);
            }
        }
    }

    private Request? DequeueNext()
    {
        lock (_gate)
        {
            Request? request;
            if (_foreground.Count > 0
                && (_background.Count == 0 || _foregroundBurst < ForegroundBurstLimit))
            {
                request = RemoveFirst(_foreground);
                _foregroundBurst++;
                return request;
            }

            if (_background.Count > 0)
            {
                request = RemoveFirst(_background);
                _foregroundBurst = 0;
                return request;
            }

            if (_foreground.Count > 0)
            {
                request = RemoveFirst(_foreground);
                _foregroundBurst++;
                return request;
            }

            _running = false;
            return null;
        }
    }

    private static Request RemoveFirst(LinkedList<Request> queue)
    {
        LinkedListNode<Request> node = queue.First!;
        queue.RemoveFirst();
        node.Value.Node = null;
        return node.Value;
    }

    private bool DropOldestBackground()
    {
        if (_background.First is not { } node)
            return false;

        _background.RemoveFirst();
        node.Value.Node = null;
        node.Value.TrySetResult(null);
        return true;
    }

    private void Cancel(Request request)
    {
        lock (_gate)
        {
            if (request.Node is { List: not null } node)
            {
                node.List.Remove(node);
                request.Node = null;
            }
        }
        request.TrySetCanceled();
    }

    private sealed class Request
    {
        private readonly TaskCompletionSource<NativeRasterImage?> _completion = new(TaskCreationOptions.RunContinuationsAsynchronously);
        private readonly CancellationTokenRegistration _registration;

        public Request(
            NativeThumbnailScheduler owner,
            string path,
            int size,
            bool cacheOnly,
            CancellationToken token)
        {
            Path = path;
            Size = size;
            CacheOnly = cacheOnly;
            Token = token;
            if (token.CanBeCanceled)
                _registration = token.Register(
                    static state =>
                    {
                        var (scheduler, request) = ((NativeThumbnailScheduler, Request))state!;
                        scheduler.Cancel(request);
                    },
                    (owner, this));
        }

        public string Path { get; }
        public int Size { get; }
        public bool CacheOnly { get; }
        public CancellationToken Token { get; }
        public LinkedListNode<Request>? Node { get; set; }
        public Task<NativeRasterImage?> Task => _completion.Task;

        public void TrySetResult(NativeRasterImage? image)
        {
            _completion.TrySetResult(image);
            _registration.Unregister();
        }

        public void TrySetCanceled()
        {
            _completion.TrySetCanceled(Token);
            _registration.Unregister();
        }
    }
}
