using Microsoft.Graphics.Display;
using Microsoft.UI;
using Microsoft.UI.Windowing;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

/// <summary>
/// Tracks the advanced-color state of the display containing the preview window.
/// Rendering remains SDR until a source and an FP16 surface are both validated.
/// </summary>
internal sealed class HdrDisplayCapabilityTracker : IDisposable
{
    private DisplayInformation? _displayInformation;
    private WindowId? _windowId;
    private bool _disposed;

    public DisplayColorCapability Capability { get; private set; } = DisplayColorCapability.Unknown;

    public event Action<DisplayColorCapability>? CapabilityChanged;

    public void Attach(WindowId windowId)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        if (_windowId == windowId && _displayInformation is not null)
        {
            Refresh(_displayInformation);
            return;
        }

        Detach();
        try
        {
            DisplayInformation displayInformation = DisplayInformation.CreateForWindowId(windowId);
            displayInformation.AdvancedColorInfoChanged += OnAdvancedColorInfoChanged;
            _displayInformation = displayInformation;
            _windowId = windowId;
            Refresh(displayInformation);
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "HDR display capability unavailable: " + ex.Message);
            _displayInformation = null;
            _windowId = null;
            SetCapability(DisplayColorCapability.Unknown);
        }
    }

    public void Refresh(WindowId windowId)
    {
        if (_windowId != windowId || _displayInformation is null)
            Attach(windowId);
        else
            Refresh(_displayInformation);
    }

    private void OnAdvancedColorInfoChanged(DisplayInformation sender, object args)
        => Refresh(sender);

    private void Refresh(DisplayInformation displayInformation)
    {
        try
        {
            DisplayAdvancedColorKind kind = displayInformation.GetAdvancedColorInfo().CurrentAdvancedColorKind;
            SetCapability(kind == DisplayAdvancedColorKind.HighDynamicRange
                ? DisplayColorCapability.HighDynamicRange
                : DisplayColorCapability.StandardDynamicRange);
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "HDR display capability refresh failed: " + ex.Message);
            SetCapability(DisplayColorCapability.Unknown);
        }
    }

    private void SetCapability(DisplayColorCapability capability)
    {
        if (Capability == capability)
            return;

        Capability = capability;
        CapabilityChanged?.Invoke(capability);
    }

    private void Detach()
    {
        if (_displayInformation is not null)
            _displayInformation.AdvancedColorInfoChanged -= OnAdvancedColorInfoChanged;
        _displayInformation = null;
        _windowId = null;
    }

    public void Dispose()
    {
        if (_disposed)
            return;

        _disposed = true;
        Detach();
        CapabilityChanged = null;
    }
}
