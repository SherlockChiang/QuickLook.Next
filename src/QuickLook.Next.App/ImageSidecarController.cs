using System.Collections.ObjectModel;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class ImageSidecarController
{
    private const int MaxImageThumbnailCacheItems = 256;
    private const int MaxImageFilmstripItems = 600;
    private const int MaxInitialFilmstripThumbnailLoads = 160;
    private const int ImmediateFilmstripThumbnailRadius = 20;
    private const int FilmstripThumbnailBatchSize = 12;
    private const int DelayedFilmstripThumbnailStartMs = 350;
    private const int AdjacentImagePrefetchRadius = 2;

    private readonly ListView _filmstripList;
    private readonly FrameworkElement _filmstrip;
    private readonly DispatcherQueue _dispatcherQueue;
    private readonly Func<string, PreviewListing?> _loadFolderListing;
    private readonly Func<string, bool> _isImagePath;
    private readonly Func<string, int, CancellationToken, bool> _isPathCurrent;
    private readonly Func<string, int, CancellationToken, Task<NativeRasterImage?>> _loadThumbnail;
    private readonly Func<NativeRasterImage, ImageSource?> _createBitmapSource;
    private readonly ImageThumbnailCache _thumbnailCache = new(MaxImageThumbnailCacheItems);
    private string[] _siblingPaths = [];

    public ImageSidecarController(
        ListView filmstripList,
        FrameworkElement filmstrip,
        DispatcherQueue dispatcherQueue,
        Func<string, PreviewListing?> loadFolderListing,
        Func<string, bool> isImagePath,
        Func<string, int, CancellationToken, bool> isPathCurrent,
        Func<string, int, CancellationToken, Task<NativeRasterImage?>> loadThumbnail,
        Func<NativeRasterImage, ImageSource?> createBitmapSource)
    {
        _filmstripList = filmstripList;
        _filmstrip = filmstrip;
        _dispatcherQueue = dispatcherQueue;
        _loadFolderListing = loadFolderListing;
        _isImagePath = isImagePath;
        _isPathCurrent = isPathCurrent;
        _loadThumbnail = loadThumbnail;
        _createBitmapSource = createBitmapSource;
        _filmstripList.ItemsSource = Items;
    }

    public ObservableCollection<ImageFilmstripItem> Items { get; } = [];

    public void Clear()
    {
        _siblingPaths = [];
        Items.Clear();
        _filmstripList.SelectedItem = null;
        _filmstrip.Visibility = Visibility.Collapsed;
    }

    public async Task LoadFilmstripAsync(string path, int generation, CancellationToken token)
    {
        try
        {
            using var trace = DiagLog.TraceScope("App", $"image filmstrip load gen={generation}; path={path}", 500);
            string? folder = Path.GetDirectoryName(path);
            if (string.IsNullOrWhiteSpace(folder))
                return;

            string[] siblings = await Task.Run(() =>
            {
                var listing = _loadFolderListing(folder);
                return listing is null
                    ? []
                    : ImageFilmstripPlanner.BuildSiblingPaths(listing, _isImagePath, MaxImageFilmstripItems);
            }, token);
            token.ThrowIfCancellationRequested();
            DiagLog.Write("App", $"image filmstrip listing gen={generation}; siblings={siblings.Length}");

            if (!_isPathCurrent(path, generation, token))
                return;

            _dispatcherQueue.TryEnqueue(() =>
            {
                if (!_isPathCurrent(path, generation, token))
                    return;

                _siblingPaths = siblings;
                Items.Clear();
                foreach (string sibling in siblings)
                {
                    Items.Add(new ImageFilmstripItem
                    {
                        Path = sibling,
                        Name = Path.GetFileName(sibling),
                    });
                }
                SelectCurrent(path);
                _filmstrip.Visibility = siblings.Length > 1 ? Visibility.Visible : Visibility.Collapsed;
            });
            _ = PrefetchAdjacentImagesAsync(path, siblings, generation, token);

            var thumbnailBatch = new List<(string Path, ImageSource Source)>(FilmstripThumbnailBatchSize);
            int thumbnailAttempts = 0;
            bool delayedFarThumbnails = false;
            foreach ((string sibling, int distance) in ImageFilmstripPlanner.PrioritizeWithDistance(siblings, path).Take(MaxInitialFilmstripThumbnailLoads))
            {
                token.ThrowIfCancellationRequested();
                if (!_isPathCurrent(path, generation, token))
                    return;

                if (distance > ImmediateFilmstripThumbnailRadius && !delayedFarThumbnails)
                {
                    FlushThumbnailBatch(path, generation, token, thumbnailBatch);
                    delayedFarThumbnails = true;
                    await Task.Delay(DelayedFilmstripThumbnailStartMs, token);
                    if (!_isPathCurrent(path, generation, token))
                        return;
                }

                thumbnailAttempts++;
                if (_thumbnailCache.TryGet(sibling, out ImageSource? cachedSource) && cachedSource is not null)
                {
                    thumbnailBatch.Add((sibling, cachedSource));
                    FlushThumbnailBatchIfNeeded(path, generation, token, thumbnailBatch);
                    continue;
                }
                if (CloudFileStatus.MayRequireHydration(sibling))
                {
                    DiagLog.Write("App", $"image filmstrip skipped cloud placeholder: {sibling}");
                    continue;
                }

                NativeRasterImage? raster = await _loadThumbnail(sibling, 96, token);
                if (!_isPathCurrent(path, generation, token))
                    return;

                if (raster is null)
                    continue;
                ImageSource? source = _createBitmapSource(raster);
                if (source is null)
                    continue;
                _thumbnailCache.Add(sibling, source);

                thumbnailBatch.Add((sibling, source));
                FlushThumbnailBatchIfNeeded(path, generation, token, thumbnailBatch);
            }
            FlushThumbnailBatch(path, generation, token, thumbnailBatch);
            DiagLog.Write("App", $"image filmstrip thumbnails done gen={generation}; siblings={siblings.Length}; attempted={thumbnailAttempts}");
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "image filmstrip load failed: " + ex.Message);
        }
    }

    public string? GetRelativePath(string currentPath, int delta)
    {
        if (_siblingPaths.Length == 0 || string.IsNullOrWhiteSpace(currentPath))
            return null;

        int index = Array.FindIndex(_siblingPaths, p => string.Equals(p, currentPath, StringComparison.OrdinalIgnoreCase));
        if (index < 0)
            return null;

        int next = (index + delta + _siblingPaths.Length) % _siblingPaths.Length;
        return _siblingPaths[next];
    }

    public string? NextPathAfterDelete(string currentPath)
    {
        if (_siblingPaths.Length == 0)
            return null;

        int index = Array.FindIndex(_siblingPaths, p => string.Equals(p, currentPath, StringComparison.OrdinalIgnoreCase));
        if (index < 0)
            return null;

        for (int i = index + 1; i < _siblingPaths.Length; i++)
        {
            if (File.Exists(_siblingPaths[i]))
                return _siblingPaths[i];
        }

        for (int i = index - 1; i >= 0; i--)
        {
            if (File.Exists(_siblingPaths[i]))
                return _siblingPaths[i];
        }

        return null;
    }

    public void RemovePath(string path)
    {
        _thumbnailCache.Remove(path);
        _siblingPaths = _siblingPaths
            .Where(p => !string.Equals(p, path, StringComparison.OrdinalIgnoreCase))
            .ToArray();

        ImageFilmstripItem? item = Items.FirstOrDefault(i =>
            string.Equals(i.Path, path, StringComparison.OrdinalIgnoreCase));
        if (item is not null)
            Items.Remove(item);

        _filmstrip.Visibility = _siblingPaths.Length > 1 ? Visibility.Visible : Visibility.Collapsed;
    }

    private void FlushThumbnailBatchIfNeeded(
        string currentPath,
        int generation,
        CancellationToken token,
        List<(string Path, ImageSource Source)> batch)
    {
        if (batch.Count >= FilmstripThumbnailBatchSize)
            FlushThumbnailBatch(currentPath, generation, token, batch);
    }

    private void FlushThumbnailBatch(
        string currentPath,
        int generation,
        CancellationToken token,
        List<(string Path, ImageSource Source)> batch)
    {
        if (batch.Count == 0)
            return;

        (string Path, ImageSource Source)[] updates = batch.ToArray();
        batch.Clear();
        _dispatcherQueue.TryEnqueue(() =>
        {
            if (!_isPathCurrent(currentPath, generation, token))
                return;
            foreach ((string path, ImageSource source) in updates)
            {
                ImageFilmstripItem? item = Items.FirstOrDefault(i =>
                    string.Equals(i.Path, path, StringComparison.OrdinalIgnoreCase));
                if (item is not null)
                    item.Thumbnail = source;
            }
        });
    }

    private async Task PrefetchAdjacentImagesAsync(string currentPath, string[] siblings, int generation, CancellationToken token)
    {
        try
        {
            string[] targets = ImageFilmstripPlanner.AdjacentPaths(siblings, currentPath, AdjacentImagePrefetchRadius)
                .Where(path => !_thumbnailCache.Contains(path))
                .Where(path => !CloudFileStatus.MayRequireHydration(path))
                .ToArray();
            if (targets.Length == 0)
                return;

            DiagLog.Write("App", $"image adjacent prefetch gen={generation}; count={targets.Length}");
            foreach (string target in targets)
            {
                token.ThrowIfCancellationRequested();
                if (!_isPathCurrent(currentPath, generation, token))
                    return;

                NativeRasterImage? raster = await Task.Run(async () =>
                {
                    if (token.IsCancellationRequested || !_isPathCurrent(currentPath, generation, token))
                        return null;

                    return await _loadThumbnail(target, 128, token);
                }, token);
                if (!_isPathCurrent(currentPath, generation, token))
                    return;

                if (raster is null)
                    continue;

                _dispatcherQueue.TryEnqueue(() =>
                {
                    if (!_isPathCurrent(currentPath, generation, token))
                        return;
                    ImageSource? source = _createBitmapSource(raster);
                    if (source is null)
                        return;
                    _thumbnailCache.Add(target, source);
                    ImageFilmstripItem? item = Items.FirstOrDefault(i =>
                        string.Equals(i.Path, target, StringComparison.OrdinalIgnoreCase));
                    if (item is not null && item.Thumbnail is null)
                        item.Thumbnail = source;
                });
            }
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "image adjacent prefetch failed: " + ex.Message);
        }
    }

    public void SelectCurrent(string path)
    {
        ImageFilmstripItem? current = Items.FirstOrDefault(i =>
            string.Equals(i.Path, path, StringComparison.OrdinalIgnoreCase));
        if (current is null)
            return;
        _filmstripList.SelectedItem = current;
        _filmstripList.ScrollIntoView(current);
    }
}
