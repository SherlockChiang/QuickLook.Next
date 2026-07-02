using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class ListingPreviewPresenter
{
    private static readonly Microsoft.UI.Xaml.Media.SolidColorBrush UiGrayBrush = new(Microsoft.UI.Colors.Gray);

    private readonly TextBlock _title;
    private readonly TextBlock _summary;
    private readonly StackPanel _breadcrumbPanel;
    private readonly ListView _listView;
    private readonly Button _nameHeader;
    private readonly Button _modifiedHeader;
    private readonly Button _typeHeader;
    private readonly Button _sizeHeader;
    private readonly Func<string, PreviewListing?> _loadFolderListing;
    private readonly Func<int> _getGeneration;
    private readonly Func<int, bool> _isGenerationCurrent;
    private readonly Action<ListingRow> _openItem;

    private PreviewListing? _currentListing;
    private string _currentPath = "";
    private string _sortColumn = "name";
    private bool _sortAscending = true;

    public ListingPreviewPresenter(
        TextBlock title,
        TextBlock summary,
        StackPanel breadcrumbPanel,
        ListView listView,
        Button nameHeader,
        Button modifiedHeader,
        Button typeHeader,
        Button sizeHeader,
        Func<string, PreviewListing?> loadFolderListing,
        Func<int> getGeneration,
        Func<int, bool> isGenerationCurrent,
        Action<ListingRow> openItem)
    {
        _title = title;
        _summary = summary;
        _breadcrumbPanel = breadcrumbPanel;
        _listView = listView;
        _nameHeader = nameHeader;
        _modifiedHeader = modifiedHeader;
        _typeHeader = typeHeader;
        _sizeHeader = sizeHeader;
        _loadFolderListing = loadFolderListing;
        _getGeneration = getGeneration;
        _isGenerationCurrent = isGenerationCurrent;
        _openItem = openItem;
    }

    public ListingPreviewResult Render(PreviewReady ready, (double Width, double Height) maxContent)
    {
        _currentListing = ready.Listing;
        _currentPath = "";
        RenderListing();
        var size = EstimatePreviewSize(maxContent);
        return new ListingPreviewResult($"{ready.Kind}: {ready.Title}", size.Width, size.Height);
    }

    public void Reset()
    {
        _currentListing = null;
        _currentPath = "";
        _listView.ItemsSource = null;
        _breadcrumbPanel.Children.Clear();
    }

    public void UpdateSortHeaders()
    {
        SetHeader(_nameHeader, "name", "名称");
        SetHeader(_modifiedHeader, "modified", "修改日期");
        SetHeader(_typeHeader, "type", "类型");
        SetHeader(_sizeHeader, "size", "大小");
    }

    public void OnSortClick(object sender)
    {
        if (sender is not Button { Tag: string column })
            return;

        if (_sortColumn == column)
            _sortAscending = !_sortAscending;
        else
        {
            _sortColumn = column;
            _sortAscending = column != "modified";
        }
        RenderListing();
    }

    public void OnItemClick(ItemClickEventArgs e)
    {
        if (e.ClickedItem is ListingRow row)
            _listView.SelectedItem = row;
    }

    public async Task OnDoubleTappedAsync()
    {
        if (_listView.SelectedItem is not ListingRow row)
            return;

        if (row.IsFolder)
            await NavigateIntoFolderAsync(row);
        else
            _openItem(row);
    }

    public async Task OnKeyDownAsync(Microsoft.UI.Xaml.Input.KeyRoutedEventArgs e)
    {
        if (e.Key == Windows.System.VirtualKey.Back)
        {
            NavigateBack();
            e.Handled = true;
            return;
        }

        if (e.Key == Windows.System.VirtualKey.Enter && _listView.SelectedItem is ListingRow row)
        {
            if (row.IsFolder)
                await NavigateIntoFolderAsync(row);
            else
                _openItem(row);
            e.Handled = true;
        }
    }

    private void RenderListing()
    {
        if (_currentListing is null)
            return;

        string title = _currentListing.RootName;
        if (!string.IsNullOrEmpty(_currentPath))
        {
            string path = _currentPath.TrimEnd('/');
            int slash = path.LastIndexOf('/');
            title = slash >= 0 ? path[(slash + 1)..] : path;
        }

        RenderBreadcrumb();

        var visibleItems = _currentListing.Items
            .Where(i => string.Equals(NormalizePath(i.ParentPath), _currentPath, StringComparison.OrdinalIgnoreCase))
            .ToList();
        var rows = visibleItems
            .OrderBy(i => i, Comparer<PreviewListingItem>.Create(CompareItems))
            .Select(i => new ListingRow(i))
            .ToList();

        _title.Text = title;
        _summary.Text = BuildSummary(_currentListing, visibleItems);
        _listView.ItemsSource = rows;
        UpdateSortHeaders();
    }

    private int CompareItems(PreviewListingItem left, PreviewListingItem right)
    {
        int folderCompare = right.IsFolder.CompareTo(left.IsFolder);
        if (folderCompare != 0)
            return folderCompare;

        int result = _sortColumn switch
        {
            "modified" => left.ModifiedUnix.CompareTo(right.ModifiedUnix),
            "type" => string.Compare(left.IsFolder ? "文件夹" : left.Type, right.IsFolder ? "文件夹" : right.Type, StringComparison.OrdinalIgnoreCase),
            "size" => left.Size.CompareTo(right.Size),
            _ => string.Compare(left.Name, right.Name, StringComparison.OrdinalIgnoreCase),
        };
        if (result == 0)
            result = string.Compare(left.Name, right.Name, StringComparison.OrdinalIgnoreCase);
        return _sortAscending ? result : -result;
    }

    private void RenderBreadcrumb()
    {
        _breadcrumbPanel.Children.Clear();
        if (_currentListing is null)
            return;

        AddBreadcrumbButton(_currentListing.RootName, "");

        string current = _currentPath.TrimEnd('/');
        if (current.Length == 0)
            return;

        string acc = "";
        foreach (string part in current.Split('/', StringSplitOptions.RemoveEmptyEntries))
        {
            _breadcrumbPanel.Children.Add(new TextBlock
            {
                Text = ">",
                VerticalAlignment = VerticalAlignment.Center,
                Foreground = UiGrayBrush,
            });
            acc = acc.Length == 0 ? part + "/" : acc + part + "/";
            AddBreadcrumbButton(part, acc);
        }
    }

    private void AddBreadcrumbButton(string text, string path)
    {
        var button = new Button
        {
            Content = text,
            Tag = path,
            Padding = new Thickness(8, 2, 8, 2),
            MinHeight = 26,
        };
        button.Click += OnBreadcrumbClick;
        _breadcrumbPanel.Children.Add(button);
    }

    private void OnBreadcrumbClick(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string path })
        {
            _currentPath = NormalizePath(path);
            RenderListing();
        }
    }

    private async Task NavigateIntoFolderAsync(ListingRow row)
    {
        _currentPath = NormalizePath(row.Path);
        RenderListing();
        await TryLoadPhysicalFolderLevelAsync(row, _currentPath, _getGeneration());
    }

    private void NavigateBack()
    {
        if (string.IsNullOrEmpty(_currentPath))
            return;

        string current = _currentPath.TrimEnd('/');
        int slash = current.LastIndexOf('/');
        _currentPath = slash < 0 ? "" : current[..(slash + 1)];
        RenderListing();
    }

    private async Task TryLoadPhysicalFolderLevelAsync(ListingRow row, string parentPath, int generation)
    {
        if (_currentListing is null
            || !_currentListing.ListingKind.Equals("folder", StringComparison.OrdinalIgnoreCase)
            || string.IsNullOrWhiteSpace(row.NativePath))
        {
            return;
        }

        bool alreadyLoaded = _currentListing.Items.Any(i =>
            string.Equals(NormalizePath(i.ParentPath), parentPath, StringComparison.OrdinalIgnoreCase));
        if (alreadyLoaded)
            return;

        try
        {
            _summary.Text = "正在读取文件夹...";
            var childListing = await Task.Run(() => _loadFolderListing(row.NativePath));
            if (!_isGenerationCurrent(generation) || _currentListing is null)
                return;
            if (childListing is null)
                return;

            var children = childListing.Items
                .Select(i => PrefixItem(parentPath, i))
                .ToArray();
            var merged = _currentListing.Items
                .Concat(children)
                .GroupBy(i => NormalizePath(i.Path), StringComparer.OrdinalIgnoreCase)
                .Select(g => g.Last())
                .ToArray();

            _currentListing = _currentListing with
            {
                Items = merged,
                IsPartial = _currentListing.IsPartial || childListing.IsPartial,
            };
            if (string.Equals(_currentPath, parentPath, StringComparison.OrdinalIgnoreCase))
                RenderListing();
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "folder navigation failed: " + ex);
            _summary.Text = "无法读取此文件夹";
        }
    }

    private (double Width, double Height) EstimatePreviewSize((double Width, double Height) maxContent)
    {
        if (_currentListing is null)
            return (Math.Min(720, maxContent.Width), Math.Min(480, maxContent.Height));

        var visibleItems = _currentListing.Items
            .Where(i => string.Equals(NormalizePath(i.ParentPath), _currentPath, StringComparison.OrdinalIgnoreCase))
            .ToArray();

        int visibleRows = Math.Clamp(visibleItems.Length, 4, 16);
        int longestName = visibleItems
            .Take(200)
            .Select(i => i.Name.Length)
            .DefaultIfEmpty(_currentListing.RootName.Length)
            .Max();

        double nameColumn = Math.Clamp(longestName * 7.4 + 96, 300, 520);
        double width = nameColumn + 170 + 140 + 110 + 110;
        double height = 128 + visibleRows * 36;

        return (
            Math.Clamp(width, 620, maxContent.Width),
            Math.Clamp(height, 320, maxContent.Height));
    }

    private string BuildSummary(PreviewListing listing, IReadOnlyCollection<PreviewListingItem> visibleItems)
    {
        if (string.IsNullOrEmpty(_currentPath))
            return listing.Summary + (listing.IsPartial ? " - 部分内容" : "");

        int folders = visibleItems.Count(i => i.IsFolder);
        int files = visibleItems.Count - folders;
        long bytes = visibleItems.Where(i => !i.IsFolder).Sum(i => i.Size);
        return $"{files:N0} files, {folders:N0} folders - {MainWindow.FormatBytes(bytes)}";
    }

    private void SetHeader(Button button, string column, string label)
    {
        string arrow = _sortColumn == column ? (_sortAscending ? " ↑" : " ↓") : "";
        button.Content = label + arrow;
    }

    private static PreviewListingItem PrefixItem(string parentPath, PreviewListingItem item)
    {
        string itemPath = item.Path.Replace('\\', '/').TrimStart('/');
        string itemParent = item.ParentPath.Replace('\\', '/').TrimStart('/');
        return item with
        {
            Path = parentPath + itemPath,
            ParentPath = string.IsNullOrEmpty(itemParent) ? parentPath : parentPath + NormalizePath(itemParent),
        };
    }

    private static string NormalizePath(string path)
    {
        path = path.Replace('\\', '/').TrimStart('/');
        return path.Length > 0 && !path.EndsWith('/') ? path + "/" : path;
    }
}

internal readonly record struct ListingPreviewResult(string Status, double Width, double Height);
