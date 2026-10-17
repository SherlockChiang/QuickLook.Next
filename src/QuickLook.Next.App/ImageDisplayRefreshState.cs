using QuickLook.Next.Core;

namespace QuickLook.Next.App;

// UI-thread state. Display events can race initial decode, navigation and surface delivery.
internal sealed class ImageDisplayRefreshState
{
    private string? _requestId;
    private bool _requestedHdr;
    private long _generation;
    private long _displayedGeneration;

    public void Begin(string requestId, bool hdrEnabled)
    {
        _requestId = requestId;
        _requestedHdr = hdrEnabled;
        _generation = 0;
        _displayedGeneration = 0;
    }

    public void Clear() => _requestId = null;

    public PreviewImageRefresh? Request(string requestId, bool hdrEnabled)
    {
        if (_requestId != requestId || _requestedHdr == hdrEnabled)
            return null;
        _requestedHdr = hdrEnabled;
        return new PreviewImageRefresh(requestId, ++_generation, hdrEnabled);
    }

    public bool CanAccept(PreviewSurface surface, bool hdrEnabled)
        => surface.RequestId == _requestId
            && surface.ImageGeneration > 0
            && surface.ImageGeneration == _generation
            && surface.ImageGeneration > _displayedGeneration
            && _requestedHdr == hdrEnabled;

    public PreviewImageRefreshClose? Commit(PreviewSurface surface)
    {
        long previous = _displayedGeneration;
        _displayedGeneration = surface.ImageGeneration;
        return previous > 0
            ? new PreviewImageRefreshClose(surface.RequestId, previous)
            : null;
    }
}
