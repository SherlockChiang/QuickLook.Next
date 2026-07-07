using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class ExifPreviewPresenter
{
    private readonly StackPanel _detailsList;
    private readonly ScrollViewer _scrollViewer;
    private readonly FrameworkElement _emptyPanel;
    private readonly TextBlock _unavailableText;
    private readonly FrameworkElement _mapsButton;
    private readonly TextBlock _statusText;
    private readonly FrameworkElement _statusBar;
    private double? _latitude;
    private double? _longitude;

    public ExifPreviewPresenter(
        StackPanel detailsList,
        ScrollViewer scrollViewer,
        FrameworkElement emptyPanel,
        TextBlock unavailableText,
        FrameworkElement mapsButton,
        TextBlock statusText,
        FrameworkElement statusBar)
    {
        _detailsList = detailsList;
        _scrollViewer = scrollViewer;
        _emptyPanel = emptyPanel;
        _unavailableText = unavailableText;
        _mapsButton = mapsButton;
        _statusText = statusText;
        _statusBar = statusBar;
    }

    public bool HasLocation
        => _latitude is { } latitude
            && _longitude is { } longitude
            && latitude >= -90
            && latitude <= 90
            && longitude >= -180
            && longitude <= 180;

    public void Reset()
    {
        _latitude = null;
        _longitude = null;
        _detailsList.Children.Clear();
        _scrollViewer.Visibility = Visibility.Collapsed;
        _emptyPanel.Visibility = Visibility.Visible;
        _mapsButton.Visibility = Visibility.Collapsed;
        _unavailableText.Text = UiStrings.NoExifData;
    }

    public void RenderRows(IReadOnlyList<(string Label, string Value)> rows, double? latitude, double? longitude)
    {
        _detailsList.Children.Clear();
        _latitude = latitude;
        _longitude = longitude;
        if (rows.Count == 0)
        {
            _scrollViewer.Visibility = Visibility.Collapsed;
            _emptyPanel.Visibility = Visibility.Visible;
            return;
        }

        foreach (var (label, value) in rows)
            AddRailDetail(label, value);

        _mapsButton.Visibility = HasLocation ? Visibility.Visible : Visibility.Collapsed;
        _emptyPanel.Visibility = Visibility.Collapsed;
        _scrollViewer.Visibility = Visibility.Visible;
    }

    public void OpenLocationInGoogleMaps()
    {
        if (!HasLocation || _latitude is not { } latitude || _longitude is not { } longitude)
            return;

        MapLocation location = ExifMapLocation.NormalizeForGoogleMaps(latitude, longitude);
        string query = location.ToQueryString();
        string url = "https://www.google.com/maps/search/?api=1&query=" + Uri.EscapeDataString(query);

        try
        {
            System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
            {
                FileName = url,
                UseShellExecute = true,
            });
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "open EXIF location failed: " + ex.Message);
            _statusText.Text = ex.Message;
            _statusBar.Visibility = Visibility.Visible;
        }
    }

    private void AddRailDetail(string label, string value)
    {
        var stack = new StackPanel { Spacing = 2 };
        stack.Children.Add(new TextBlock
        {
            Text = label,
            FontSize = 11,
            Foreground = (Brush)Application.Current.Resources["TextFillColorSecondaryBrush"],
        });
        stack.Children.Add(new TextBlock
        {
            Text = value,
            FontSize = 13,
            TextWrapping = TextWrapping.WrapWholeWords,
        });
        _detailsList.Children.Add(stack);
    }
}
