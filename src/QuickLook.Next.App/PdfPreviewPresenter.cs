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
    private enum PdfPageState
    {
        Requested,
        Rendering,
        Rendered,
        Released,
    }

    private const double PageTargetWidth = 860;
    private const double PageSpacing = 16;
    private const double PagePanelTopPadding = 16;
    private const int OffscreenSurfaceCachePages = 5;
    private const int MaxActivePageHosts = 64;
    private const int PageHostOverscan = 4;

    private static readonly SolidColorBrush PageBackground = new(Microsoft.UI.Colors.White);

    private readonly ScrollViewer _scrollViewer;
    private readonly StackPanel _pagesPanel;
    private readonly Border _pagerBar;
    private readonly Button _previousButton;
    private readonly Button _nextButton;
    private readonly TextBlock _pageIndicator;
    private readonly DispatcherQueue _dispatcherQueue;
    private readonly Func<Compositor?> _compositorProvider;
    private readonly Func<RasterHostSupervisor?> _supervisorProvider;

    private readonly Dictionary<int, Border> _pageHosts = new();
    private readonly HashSet<int> _requestedPages = new();
    private readonly Dictionary<int, PdfPageState> _pageStates = new();
    private readonly Dictionary<int, long> _pageLastTouched = new();

    private string? _requestId;
    private double _scale = 1.0;
    private long _touchTick;
    private int _currentPageIndex;
    private int _pageCount;
    private double[] _pageDisplayWidths = [];
    private double[] _pageDisplayHeights = [];
    private double[] _pageTopOffsets = [];
    private double _lastScrollOffset;
    private int _renderVersion;

    public PdfPreviewPresenter(
        ScrollViewer scrollViewer,
        StackPanel pagesPanel,
        Border pagerBar,
        Button previousButton,
        Button nextButton,
        TextBlock pageIndicator,
        DispatcherQueue dispatcherQueue,
        Func<Compositor?> compositorProvider,
        Func<RasterHostSupervisor?> supervisorProvider)
    {
        _scrollViewer = scrollViewer;
        _pagesPanel = pagesPanel;
        _pagerBar = pagerBar;
        _previousButton = previousButton;
        _nextButton = nextButton;
        _pageIndicator = pageIndicator;
        _dispatcherQueue = dispatcherQueue;
        _compositorProvider = compositorProvider;
        _supervisorProvider = supervisorProvider;
        _pagesPanel.Spacing = 0;
        _scrollViewer.ViewChanged += (_, _) => RequestVisiblePages();
        _scrollViewer.SizeChanged += (_, _) => RequestVisiblePages();
    }

    public PdfPreviewResult Render(string requestId, PreviewReady ready, (double Width, double Height) maxContent)
    {
        Clear();
        _requestId = requestId;
        _currentPageIndex = 0;
        int version = _renderVersion;

        _pageCount = Math.Max(1, ready.PageCount);
        double pageWidth = Math.Max(1, ready.PageWidth > 0 ? ready.PageWidth : ready.PreferredWidth);
        double pageHeight = Math.Max(1, ready.PageHeight > 0 ? ready.PageHeight : ready.PreferredHeight);
        PdfPageGeometry[]? geometries = ready.PdfPageGeometries;
        bool hasPageGeometries = geometries is { Length: > 0 }
            && geometries.Length == _pageCount
            && geometries.All(geometry => double.IsFinite(geometry.Width)
                && double.IsFinite(geometry.Height)
                && geometry.Width > 0
                && geometry.Height > 0);
        if (hasPageGeometries)
        {
            pageWidth = geometries![0].Width;
            pageHeight = geometries[0].Height;
        }
        double targetPageWidth = Math.Min(PageTargetWidth, Math.Max(320, maxContent.Width - 64));
        double targetPageHeight = Math.Max(320, maxContent.Height - 96);
        _scale = Math.Clamp(
            Math.Min(targetPageWidth / pageWidth, targetPageHeight / pageHeight),
            0.25,
            1.6);
        _pageDisplayWidths = new double[_pageCount];
        _pageDisplayHeights = new double[_pageCount];
        _pageTopOffsets = new double[_pageCount];
        for (int pageIndex = 0; pageIndex < _pageCount; pageIndex++)
        {
            PdfPageGeometry geometry = hasPageGeometries ? geometries![pageIndex] : new(pageWidth, pageHeight);
            _pageDisplayWidths[pageIndex] = Math.Round(geometry.Width * _scale);
            _pageDisplayHeights[pageIndex] = Math.Round(geometry.Height * _scale);
            if (pageIndex > 0)
                _pageTopOffsets[pageIndex] = _pageTopOffsets[pageIndex - 1] + _pageDisplayHeights[pageIndex - 1] + PageSpacing;
        }
        UpdatePageHosts(0, Math.Min(_pageCount - 1, MaxActivePageHosts - 1));

        _scrollViewer.ChangeView(null, 0, null, disableAnimation: true);
        _pagerBar.Visibility = _pageCount > 1 ? Visibility.Visible : Visibility.Collapsed;
        UpdatePager();
        Task.Delay(100).ContinueWith(_ => _dispatcherQueue.TryEnqueue(() =>
        {
            if (version == _renderVersion)
                RequestVisiblePages();
        }));
        return new PdfPreviewResult(
            $"pdf: {ready.Title}",
            Math.Min(maxContent.Width, _pageDisplayWidths[0] + 64),
            Math.Min(maxContent.Height, _pageDisplayHeights[0] + 96));
    }

    public bool AttachSurface(PreviewSurface surface, out string? error)
    {
        error = null;
        Compositor? compositor = _compositorProvider();
        if (compositor is null || !_pageHosts.TryGetValue(surface.PageIndex, out var pageHost))
        {
            CompositionInterop.CloseSharedHandle((nint)surface.SharedHandle);
            return true;
        }
        if (_pageStates.TryGetValue(surface.PageIndex, out PdfPageState state) && state == PdfPageState.Released)
        {
            CompositionInterop.CloseSharedHandle((nint)surface.SharedHandle);
            return true;
        }

        var (compSurface, hr) = CompositionInterop.CreateSurfaceForHandle(compositor, (nint)surface.SharedHandle);
        if (hr < 0 || compSurface is null)
        {
            error = $"pdf page failed 0x{hr:X8}";
            return false;
        }

        TouchPage(surface.PageIndex);
        SetPageState(surface.PageIndex, PdfPageState.Rendered);
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

        if (_pageCount == 0)
            return;

        int pageCount = _pageCount;
        if (_pageDisplayHeights.Length != pageCount || _pageTopOffsets.Length != pageCount)
            return;

        double viewportHeight = Math.Max(1, _scrollViewer.ViewportHeight);
        double scrollOffset = _scrollViewer.VerticalOffset;
        bool scrollingDown = scrollOffset >= _lastScrollOffset;
        _lastScrollOffset = scrollOffset;
        int firstVisible = FindPageAtOffset(scrollOffset - PagePanelTopPadding);
        int lastVisible = FindPageAtOffset(scrollOffset + viewportHeight - PagePanelTopPadding);
        int centered = FindPageAtOffset(scrollOffset + viewportHeight * 0.45 - PagePanelTopPadding);
        SetCurrentPage(centered);

        int renderFirst = Math.Max(0, firstVisible - 1);
        int renderLast = Math.Min(pageCount - 1, lastVisible + 2);
        UpdatePageHosts(renderFirst, renderLast);
        foreach (int index in PrioritizePageRequests(renderFirst, renderLast, _currentPageIndex, scrollingDown))
        {
            if (!_requestedPages.Contains(index))
            {
                _requestedPages.Add(index);
                TouchPage(index);
                SetPageState(index, PdfPageState.Requested);
                _ = RenderPageAsync(supervisor, _requestId, index, _scale);
            }
            else
            {
                TouchPage(index);
            }
        }

        CancelFarPageRequests(renderFirst, renderLast);
        TrimSurfaceCache(renderFirst, renderLast);
    }

    public void GoToPreviousPage()
        => ScrollToPage(_currentPageIndex - 1);

    public void GoToNextPage()
        => ScrollToPage(_currentPageIndex + 1);

    public void Clear()
    {
        string? requestId = _requestId;
        RasterHostSupervisor? supervisor = requestId is null ? null : _supervisorProvider();
        foreach (int pageIndex in _requestedPages.ToArray())
            _ = supervisor?.ClosePageAsync(requestId!, pageIndex);

        foreach (var host in _pageHosts.Values)
        {
            ElementCompositionPreview.SetElementChildVisual(host, null);
            DisposePageVisual(host);
        }
        _pagesPanel.Children.Clear();
        _pageHosts.Clear();
        _requestedPages.Clear();
        _pageStates.Clear();
        _pageLastTouched.Clear();
        _touchTick = 0;
        _currentPageIndex = 0;
        _pageCount = 0;
        _pageDisplayWidths = [];
        _pageDisplayHeights = [];
        _pageTopOffsets = [];
        _lastScrollOffset = 0;
        _requestId = null;
        _renderVersion++;
        _pagerBar.Visibility = Visibility.Collapsed;
        UpdatePager();
    }

    private void ScrollToPage(int pageIndex)
    {
        if (_pageCount == 0)
            return;

        pageIndex = Math.Clamp(pageIndex, 0, _pageCount - 1);
        double targetOffset = PagePanelTopPadding + _pageTopOffsets[pageIndex];
        SetCurrentPage(pageIndex);
        _scrollViewer.ChangeView(null, targetOffset, null, disableAnimation: false);
        RequestVisiblePages();
    }

    private void SetCurrentPage(int pageIndex)
    {
        if (_pageCount == 0)
        {
            _currentPageIndex = 0;
            UpdatePager();
            return;
        }

        pageIndex = Math.Clamp(pageIndex, 0, _pageCount - 1);
        if (_currentPageIndex == pageIndex)
        {
            UpdatePager();
            return;
        }

        _currentPageIndex = pageIndex;
        UpdatePager();
    }

    private void UpdatePager()
    {
        int pageCount = _pageCount;
        int displayPage = pageCount == 0 ? 0 : _currentPageIndex + 1;
        _pageIndicator.Text = pageCount == 0
            ? UiStrings.PdfPageIndicatorEmpty
            : UiStrings.Format(UiStrings.PdfPageIndicatorFormat, displayPage, pageCount);
        _previousButton.IsEnabled = pageCount > 1 && _currentPageIndex > 0;
        _nextButton.IsEnabled = pageCount > 1 && _currentPageIndex < pageCount - 1;
    }

    private void TouchPage(int pageIndex)
    {
        _pageLastTouched[pageIndex] = ++_touchTick;
    }

    private void UpdatePageHosts(int renderFirst, int renderLast)
    {
        if (_pageCount == 0)
            return;

        int first = Math.Max(0, renderFirst - PageHostOverscan);
        int last = Math.Min(_pageCount - 1, renderLast + PageHostOverscan);
        if (last - first + 1 > MaxActivePageHosts)
        {
            first = Math.Clamp(_currentPageIndex - MaxActivePageHosts / 2, 0, _pageCount - MaxActivePageHosts);
            last = first + MaxActivePageHosts - 1;
        }

        if (_pageHosts.Count == last - first + 1
            && _pageHosts.ContainsKey(first)
            && _pageHosts.ContainsKey(last))
        {
            return;
        }

        foreach (int pageIndex in _pageHosts.Keys.ToArray())
        {
            if (pageIndex < first || pageIndex > last)
            {
                ReleasePageSurface(pageIndex);
                _pageHosts.Remove(pageIndex);
            }
        }

        for (int pageIndex = first; pageIndex <= last; pageIndex++)
        {
            if (_pageHosts.ContainsKey(pageIndex))
                continue;

            _pageHosts[pageIndex] = new Border
            {
                Width = _pageDisplayWidths[pageIndex],
                Height = _pageDisplayHeights[pageIndex],
                Margin = new Thickness(0, 0, 0, pageIndex < _pageCount - 1 ? PageSpacing : 0),
                Background = PageBackground,
            };
        }

        _pagesPanel.Children.Clear();
        double topSpacerHeight = _pageTopOffsets[first];
        if (topSpacerHeight > 0)
            _pagesPanel.Children.Add(CreateSpacer(topSpacerHeight));

        for (int pageIndex = first; pageIndex <= last; pageIndex++)
            _pagesPanel.Children.Add(_pageHosts[pageIndex]);

        double hostedEnd = _pageTopOffsets[last] + _pageDisplayHeights[last]
            + (last < _pageCount - 1 ? PageSpacing : 0);
        double bottomSpacerHeight = TotalContentHeight - hostedEnd;
        if (bottomSpacerHeight > 0)
            _pagesPanel.Children.Add(CreateSpacer(bottomSpacerHeight));
    }

    private static Border CreateSpacer(double height)
        => new() { Height = height };

    private double TotalContentHeight => _pageCount == 0
        ? 0
        : _pageTopOffsets[^1] + _pageDisplayHeights[^1];

    private int FindPageAtOffset(double offset)
    {
        int index = Array.BinarySearch(_pageTopOffsets, Math.Max(0, offset));
        if (index < 0)
            index = ~index - 1;
        return Math.Clamp(index, 0, _pageCount - 1);
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

    private void CancelFarPageRequests(int protectedFirst, int protectedLast)
    {
        foreach (int index in _requestedPages.ToArray())
        {
            if (index >= protectedFirst && index <= protectedLast)
                continue;

            if (_pageStates.TryGetValue(index, out PdfPageState state)
                && state is PdfPageState.Requested or PdfPageState.Rendering)
            {
                ReleasePageSurface(index);
            }
        }
    }

    private void ReleasePageSurface(int pageIndex)
    {
        _requestedPages.Remove(pageIndex);
        _pageLastTouched.Remove(pageIndex);
        if (_pageHosts.TryGetValue(pageIndex, out var host))
        {
            ElementCompositionPreview.SetElementChildVisual(host, null);
            DisposePageVisual(host);
        }

        if (_requestId is not null)
            _ = _supervisorProvider()?.ClosePageAsync(_requestId, pageIndex);
        SetPageState(pageIndex, PdfPageState.Released);
    }

    private async Task RenderPageAsync(RasterHostSupervisor supervisor, string requestId, int pageIndex, double scale)
    {
        if (!string.Equals(_requestId, requestId, StringComparison.Ordinal))
            return;

        SetPageState(pageIndex, PdfPageState.Rendering);
        try
        {
            await supervisor.RenderPageAsync(requestId, pageIndex, scale);
        }
        catch
        {
            if (string.Equals(_requestId, requestId, StringComparison.Ordinal)
                && _pageStates.TryGetValue(pageIndex, out PdfPageState state)
                && state == PdfPageState.Rendering)
            {
                SetPageState(pageIndex, PdfPageState.Requested);
            }
        }
    }

    private void SetPageState(int pageIndex, PdfPageState state)
    {
        _pageStates[pageIndex] = state;
    }

    private static void DisposePageVisual(Border host)
    {
        var oldChild = ElementCompositionPreview.GetElementChildVisual(host);
        if (oldChild is not SpriteVisual oldSprite)
            return;

        try { (oldSprite.Brush as IDisposable)?.Dispose(); } catch { }
        try { oldSprite.Dispose(); } catch { }
    }

    private static IEnumerable<int> PrioritizePageRequests(int first, int last, int current, bool scrollingDown)
    {
        return Enumerable.Range(first, last - first + 1)
            .OrderBy(index => Math.Abs(index - current))
            .ThenBy(index => scrollingDown ? index < current : index > current)
            .ThenBy(index => scrollingDown ? index : -index);
    }
}

internal readonly record struct PdfPreviewResult(string Status, double Width, double Height);
