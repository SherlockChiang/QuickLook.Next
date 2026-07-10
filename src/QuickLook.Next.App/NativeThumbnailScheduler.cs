namespace QuickLook.Next.App;

internal sealed class NativeThumbnailScheduler
{
    private readonly NativeBridge _native;
    private readonly object _gate = new();
    private readonly Queue<Request> _foreground = new();
    private readonly Queue<Request> _background = new();
    private bool _running;

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
        var request = new Request(path, size, cacheOnly, token);

        lock (_gate)
        {
            if (priority == NativeThumbnailPriority.Foreground)
                _foreground.Enqueue(request);
            else
                _background.Enqueue(request);

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
            if (_foreground.Count > 0)
                return _foreground.Dequeue();

            if (_background.Count > 0)
                return _background.Dequeue();

            _running = false;
            return null;
        }
    }

    private sealed class Request
    {
        private readonly TaskCompletionSource<NativeRasterImage?> _completion = new(TaskCreationOptions.RunContinuationsAsynchronously);
        private readonly CancellationTokenRegistration _registration;

        public Request(string path, int size, bool cacheOnly, CancellationToken token)
        {
            Path = path;
            Size = size;
            CacheOnly = cacheOnly;
            Token = token;
            if (token.CanBeCanceled)
                _registration = token.Register(static state => ((Request)state!).CancelFromToken(), this);
        }

        public string Path { get; }
        public int Size { get; }
        public bool CacheOnly { get; }
        public CancellationToken Token { get; }
        public Task<NativeRasterImage?> Task => _completion.Task;

        public void TrySetResult(NativeRasterImage? image)
        {
            _completion.TrySetResult(image);
            _registration.Dispose();
        }

        public void TrySetCanceled()
        {
            _completion.TrySetCanceled(Token);
            _registration.Dispose();
        }

        private void CancelFromToken()
            => _completion.TrySetCanceled(Token);
    }
}
