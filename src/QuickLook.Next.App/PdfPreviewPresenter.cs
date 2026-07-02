using System.Numerics;
using Microsoft.UI.Composition;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Xaml.Media;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class PdfPreviewPresenter
{
    private const double PageTargetWidth = 860;
    private const int OffscreenSurfaceCachePages = 5;

    private static readonly SolidColorBrush PageBackground = new(Microsoft.UI.Colors.White);

    private readonly ScrollViewer _scrollViewer;
    private readonly StackPanel _pagesPanel;
    private readonly DispatcherQueue _dispatcherQueue;
    private readonly Func<Compositor?> _compositorProvider;
    private readonly Func<RasterHostSupervisor?> _supervisorProvider;

    private readonly Dictionary<int, Border> _pageHosts = new();
    private readonly HashSet<int> _requestedPages = new();
    private readonly Dictionary<int, long> _pageLastTouched = new();

    private string? _requestId;
    private double _scale = 1.0;
    private long _touchTick;

    public PdfPreviewPresenter(
        ScrollViewer scrollViewer,
        StackPanel pagesPanel,
        DispatcherQueue dispatcherQueue,
        Func<Compositor?> compositorProvider,
        Func<RasterHostSupervisor?> supervisorProvider)
    {
        _scrollViewer = scrollViewer;
        _pagesPanel = pagesPanel;
        _dispatcherQueue = dispatcherQueue;
        _compositorProvider = compositorProvider;
        _supervisorProvider = supervisorProvider;
        _scrollViewer.ViewChanged += (_, _) => RequestVisiblePages();
    }

    public PdfPreviewResult Render(string requestId, PreviewReady ready, (double Width, double Height) maxContent)
    {
        Clear();
        _requestId = requestId;

        double pageWidth = Math.Max(1, ready.PageWidth > 0 ? ready.PageWidth : ready.PreferredWidth);
        double pageHeight = Math.Max(1, ready.PageHeight > 0 ? ready.PageHeight : ready.PreferredHeight);
        double targetPageWidth = Math.Min(PageTargetWidth, Math.Max(320, maxContent.Width - 64));
        double targetPageHeight = Math.Max(320, maxContent.Height - 96);
        _scale = Math.Clamp(
            Math.Min(targetPageWidth / pageWidth, targetPageHeight / pageHeight),
            0.25,
            1.6);
        double displayWidth = Math.Round(pageWidth * _scale);
        double displayHeight = Math.Round(pageHeight * _scale);

        int pageCount = Math.Max(1, ready.PageCount);
        for (int i = 0; i < pageCount; i++)
        {
            var pageHost = new Border
            {
                Width = displayWidth,
                Height = displayHeight,
                Background = PageBackground,
            };
            _pageHosts[i] = pageHost;
            _pagesPanel.Children.Add(pageHost);
        }

        Task.Delay(100).ContinueWith(_ => _dispatcherQueue.TryEnqueue(RequestVisiblePages));
        return new PdfPreviewResult(
            $"pdf: {ready.Title}",
            Math.Min(maxContent.Width, displayWidth + 64),
            Math.Min(maxContent.Height, displayHeight + 96));
    }

    public bool AttachSurface(PreviewSurface surface, out string? error)
    {
        error = null;
        Compositor? compositor = _compositorProvider();
        if (compositor is null || !_pageHosts.TryGetValue(surface.PageIndex, out var pageHost))
            return true;

        var (compSurface, hr) = CompositionInterop.CreateSurfaceForHandle(compositor, (nint)surface.SharedHandle);
        if (hr < 0 || compSurface is null)
        {
            error = $"pdf page failed 0x{hr:X8}";
            return false;
        }

        pageHost.Width = surface.Width;
        pageHost.Height = surface.Height;
        TouchPage(surface.PageIndex);
        DisposePageVisual(pageHost);

        var brush = compositor.CreateSurfaceBrush(compSurface);
        brush.Stretch = CompositionStretch.Fill;
        var sprite = compositor.CreateSpriteVisual();
        sprite.RelativeSizeAdjustment = Vector2.One;
        sprite.Brush = brush;
        ElementCompositionPreview.SetElementChildVisual(pageHost, sprite);
        _dispatcherQueue.TryEnqueue(() => pageHost.InvalidateArrange());
        return true;
    }

    public void RequestVisiblePages()
    {
        RasterHostSupervisor? supervisor = _supervisorProvider();
        if (supervisor is null || _requestId is null || _scrollViewer.Visibility != Visibility.Visible)
            return;

        if (_pageHosts.Count == 0)
            return;

        int pageCount = _pageHosts.Count;
        double pageHeight = _pageHosts[0].ActualHeight;
        if (pageHeight <= 0)
            return;

        double viewportHeight = Math.Max(1, _scrollViewer.ViewportHeight);
        double scrollOffset = _scrollViewer.VerticalOffset;
        int firstVisible = (int)Math.Floor(scrollOffset / pageHeight);
        int lastVisible = (int)Math.Ceiling((scrollOffset + viewportHeight) / pageHeight);

        int renderFirst = Math.Max(0, firstVisible - 1);
        int renderLast = Math.Min(pageCount - 1, lastVisible + 2);
        for (int index = renderFirst; index <= renderLast; index++)
        {
            if (!_requestedPages.Contains(index))
            {
                _requestedPages.Add(index);
                TouchPage(index);
                _ = supervisor.RenderPageAsync(_requestId, index, _scale);
            }
            else
            {
                TouchPage(index);
            }
        }

        TrimSurfaceCache(renderFirst, renderLast);
    }

    public void Clear()
    {
        foreach (Border host in _pageHosts.Values)
            DisposePageVisual(host);

        _pagesPanel.Children.Clear();
        _pageHosts.Clear();
        _requestedPages.Clear();
        _pageLastTouched.Clear();
        _touchTick = 0;
        _requestId = null;
    }

    private void TouchPage(int pageIndex)
    {
        _pageLastTouched[pageIndex] = ++_touchTick;
    }

    private void TrimSurfaceCache(int protectedFirst, int protectedLast)
    {
        if (_requestId is null)
            return;

        int protectedCount = Math.Max(0, protectedLast - protectedFirst + 1);
        int maxSurfaces = protectedCount + OffscreenSurfaceCachePages;
        if (_requestedPages.Count <= maxSurfaces)
            return;

        int excess = _requestedPages.Count - maxSurfaces;
        while (excess-- > 0)
        {
            int oldestIndex = -1;
            long oldestTick = long.MaxValue;
            foreach (int index in _requestedPages)
            {
                if (index >= protectedFirst && index <= protectedLast)
                    continue;

                long tick = _pageLastTouched.TryGetValue(index, out long value) ? value : 0;
                if (tick < oldestTick)
                {
                    oldestTick = tick;
                    oldestIndex = index;
                }
            }

            if (oldestIndex < 0)
                break;

            ReleasePageSurface(oldestIndex);
        }
    }

    private void ReleasePageSurface(int pageIndex)
    {
        _requestedPages.Remove(pageIndex);
        _pageLastTouched.Remove(pageIndex);
        if (_pageHosts.TryGetValue(pageIndex, out var host))
        {
            DisposePageVisual(host);
            ElementCompositionPreview.SetElementChildVisual(host, null);
        }

        if (_requestId is not null)
            _ = _supervisorProvider()?.ClosePageAsync(_requestId, pageIndex);
    }

    private static void DisposePageVisual(Border host)
    {
        var oldChild = ElementCompositionPreview.GetElementChildVisual(host);
        if (oldChild is not SpriteVisual oldSprite)
            return;

        try { (oldSprite.Brush as IDisposable)?.Dispose(); } catch { }
        try { oldSprite.Dispose(); } catch { }
    }
}

internal readonly record struct PdfPreviewResult(string Status, double Width, double Height);
