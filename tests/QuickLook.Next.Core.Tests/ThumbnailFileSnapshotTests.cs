using QuickLook.Next.App;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class ThumbnailFileSnapshotTests
{
    [Fact]
    public void Cache_uses_supplied_versions_without_touching_the_file_system()
    {
        var cache = new ImageThumbnailCache<object>(2);
        var file = new ThumbnailFileSnapshot("Z:/unavailable/image.png", 1, 10, true);
        var source = new object();
        cache.Add(file, 96, source);
        Assert.True(cache.TryGet(file with { Path = file.Path.ToUpperInvariant() }, 96, out var cached));
        Assert.Same(source, cached);
        Assert.False(cache.Contains(file with { ModifiedTicks = 2 }, 96));
        Assert.False(cache.Contains(file with { Length = 11 }, 96));
        cache.Add(file with { ModifiedTicks = 2 }, 96, new object());
        Assert.False(cache.Contains(file, 96));
        cache.Remove(file.Path);
        Assert.False(cache.Contains(file with { ModifiedTicks = 2 }, 96));
    }

    [Fact]
    public async Task Metadata_reads_run_on_a_worker_and_stop_between_files_on_cancellation()
    {
        using var started = new ManualResetEventSlim();
        using var release = new ManualResetEventSlim();
        using var cancellation = new CancellationTokenSource();
        int caller = Environment.CurrentManagedThreadId;
        int worker = caller;
        int reads = 0;
        Task<IReadOnlyDictionary<string, ThumbnailFileSnapshot>> pending = ThumbnailFileSnapshots.ReadAsync(
            ["first", "second"], cancellation.Token, path =>
            {
                worker = Environment.CurrentManagedThreadId;
                Interlocked.Increment(ref reads);
                started.Set();
                if (!release.Wait(TimeSpan.FromSeconds(10))) throw new TimeoutException();
                return new(path, 1, 1, true);
            });
        try
        {
            Assert.True(started.Wait(TimeSpan.FromSeconds(10)));
            Assert.NotEqual(caller, worker);
            Assert.False(pending.IsCompleted);
            cancellation.Cancel();
        }
        finally { release.Set(); }
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => pending);
        Assert.Equal(1, reads);
    }

    [Fact]
    public async Task Metadata_batches_are_bounded_and_case_insensitive()
    {
        var paths = Enumerable.Range(0, 100).Select(index => $"file{index}");
        var result = await ThumbnailFileSnapshots.ReadAsync(paths, CancellationToken.None, path => new(path, 1, 1, true));
        Assert.Equal(ThumbnailFileSnapshots.MaxBatchSize, result.Count);
        Assert.True(result.ContainsKey("FILE0"));
    }
}
