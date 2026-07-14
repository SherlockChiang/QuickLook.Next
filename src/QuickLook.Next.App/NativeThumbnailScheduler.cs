namespace QuickLook.Next.App;

internal sealed class NativeThumbnailScheduler
{
    private const int MaxQueuedRequests = 512;
    private const int ForegroundBurstLimit = 8;
    private readonly NativeBridge _native;
    private readonly object _gate = new();
    private readonly LinkedList<Work> _foreground = new();
    private readonly LinkedList<Work> _background = new();
    private readonly Dictionary<WorkKey, Work> _pending = new();
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
        var key = new WorkKey(path, size, cacheOnly);

        lock (_gate)
        {
            if (_pending.TryGetValue(key, out Work? existing))
            {
                Subscriber subscriber = existing.AddSubscriber(this, token);
                if (priority == NativeThumbnailPriority.Foreground)
                    Promote(existing);
                return subscriber.Task;
            }

            if (_foreground.Count + _background.Count >= MaxQueuedRequests)
            {
                if (priority == NativeThumbnailPriority.Background || !DropOldestBackground())
                    return Task.FromResult<NativeRasterImage?>(null);
            }

            var work = new Work(key);
            Subscriber result = work.AddSubscriber(this, token);
            if (result.Task.IsCompleted)
                return result.Task;
            _pending.Add(key, work);
            work.Node = priority == NativeThumbnailPriority.Foreground
                ? _foreground.AddLast(work)
                : _background.AddLast(work);

            if (!_running)
            {
                _running = true;
                _ = Task.Run(ProcessAsync);
            }

            return result.Task;
        }
    }

    private Task ProcessAsync()
    {
        while (true)
        {
            Work? work = DequeueNext();
            if (work is null)
                return Task.CompletedTask;

            NativeRasterImage? result = null;
            bool canceled = false;
            try
            {
                result = _native.TryGetThumbnail(
                    work.Key.Path,
                    work.Key.Size,
                    work.Key.CacheOnly,
                    work.Cancellation.Token);
            }
            catch (OperationCanceledException)
            {
                canceled = true;
            }
            catch
            {
            }

            Complete(work, result, canceled);
        }
    }

    private Work? DequeueNext()
    {
        lock (_gate)
        {
            Work? work;
            if (_foreground.Count > 0
                && (_background.Count == 0 || _foregroundBurst < ForegroundBurstLimit))
            {
                work = RemoveFirst(_foreground);
                _foregroundBurst++;
            }
            else if (_background.Count > 0)
            {
                work = RemoveFirst(_background);
                _foregroundBurst = 0;
            }
            else if (_foreground.Count > 0)
            {
                work = RemoveFirst(_foreground);
                _foregroundBurst++;
            }
            else
            {
                _running = false;
                return null;
            }

            work.Started = true;
            return work;
        }
    }

    private static Work RemoveFirst(LinkedList<Work> queue)
    {
        LinkedListNode<Work> node = queue.First!;
        queue.RemoveFirst();
        node.Value.Node = null;
        return node.Value;
    }

    private void Promote(Work work)
    {
        if (work.Started || work.Node?.List != _background)
            return;

        _background.Remove(work.Node);
        work.Node = _foreground.AddLast(work);
    }

    private bool DropOldestBackground()
    {
        if (_background.First is not { } node)
            return false;

        _background.RemoveFirst();
        Work work = node.Value;
        work.Node = null;
        RemovePending(work);
        work.CompleteSubscribers(null, canceled: false);
        work.Dispose();
        return true;
    }

    private void Cancel(Work work, Subscriber subscriber)
    {
        lock (_gate)
        {
            if (!work.RemoveSubscriber(subscriber))
                return;

            subscriber.TrySetCanceled();
            if (work.HasSubscribers)
                return;

            RemovePending(work);
            if (work.Node is { List: not null } node)
            {
                node.List.Remove(node);
                work.Node = null;
            }
            work.Cancellation.Cancel();
            if (!work.Started)
                work.Dispose();
        }
    }

    private void Complete(Work work, NativeRasterImage? result, bool canceled)
    {
        lock (_gate)
        {
            RemovePending(work);
            work.CompleteSubscribers(result, canceled);
            work.Dispose();
        }
    }

    private void RemovePending(Work work)
    {
        if (_pending.TryGetValue(work.Key, out Work? pending) && ReferenceEquals(pending, work))
            _pending.Remove(work.Key);
    }

    private readonly record struct WorkKey(string Path, int Size, bool CacheOnly)
    {
        public bool Equals(WorkKey other)
            => Size == other.Size
                && CacheOnly == other.CacheOnly
                && StringComparer.OrdinalIgnoreCase.Equals(Path, other.Path);

        public override int GetHashCode()
            => HashCode.Combine(StringComparer.OrdinalIgnoreCase.GetHashCode(Path), Size, CacheOnly);
    }

    private sealed class Work : IDisposable
    {
        private readonly List<Subscriber> _subscribers = [];

        public Work(WorkKey key) => Key = key;

        public WorkKey Key { get; }
        public CancellationTokenSource Cancellation { get; } = new();
        public LinkedListNode<Work>? Node { get; set; }
        public bool Started { get; set; }
        public bool HasSubscribers => _subscribers.Count > 0;

        public Subscriber AddSubscriber(NativeThumbnailScheduler owner, CancellationToken token)
        {
            var subscriber = new Subscriber(token);
            _subscribers.Add(subscriber);
            subscriber.Register(owner, this);
            return subscriber;
        }

        public bool RemoveSubscriber(Subscriber subscriber) => _subscribers.Remove(subscriber);

        public void CompleteSubscribers(NativeRasterImage? result, bool canceled)
        {
            Subscriber[] subscribers = _subscribers.ToArray();
            _subscribers.Clear();
            foreach (Subscriber subscriber in subscribers)
            {
                if (canceled)
                    subscriber.TrySetCanceled();
                else
                    subscriber.TrySetResult(result);
            }
        }

        public void Dispose() => Cancellation.Dispose();
    }

    private sealed class Subscriber
    {
        private readonly TaskCompletionSource<NativeRasterImage?> _completion =
            new(TaskCreationOptions.RunContinuationsAsynchronously);
        private readonly CancellationToken _token;
        private CancellationTokenRegistration _registration;

        public Subscriber(CancellationToken token) => _token = token;

        public Task<NativeRasterImage?> Task => _completion.Task;

        public void Register(NativeThumbnailScheduler owner, Work work)
        {
            if (_token.CanBeCanceled)
                _registration = _token.Register(
                    static state =>
                    {
                        var (scheduler, pending, subscriber) =
                            ((NativeThumbnailScheduler, Work, Subscriber))state!;
                        scheduler.Cancel(pending, subscriber);
                    },
                    (owner, work, this));
        }

        public void TrySetResult(NativeRasterImage? image)
        {
            _completion.TrySetResult(image);
            _registration.Unregister();
        }

        public void TrySetCanceled()
        {
            _completion.TrySetCanceled(_token);
            _registration.Unregister();
        }
    }
}
