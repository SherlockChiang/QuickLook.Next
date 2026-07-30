using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal delegate void NativeAnimationFrameSpanAction(ReadOnlySpan<byte> bgra);

internal readonly record struct NativeAnimationFrameDescriptor(
    int DelayMilliseconds,
    int PixelOffset);

/// <summary>
/// Owns the App-side duplicate of a RasterHost animation section. Frame pixels stay in
/// the read-only mapping and are exposed only while the lifetime gate is held.
/// </summary>
internal sealed class NativeAnimationFrames : IDisposable
{
    private readonly object _lifetimeGate = new();
    private readonly NativeAnimationFrameDescriptor[] _frames;
    private SharedSectionView? _view;
    private readonly int _frameByteLength;

    public NativeAnimationFrames(
        int width,
        int height,
        SharedSectionView view,
        NativeAnimationFrameDescriptor[] frames)
    {
        ArgumentNullException.ThrowIfNull(view);
        ArgumentNullException.ThrowIfNull(frames);
        if (width <= 0)
            throw new ArgumentOutOfRangeException(nameof(width));
        if (height <= 0)
            throw new ArgumentOutOfRangeException(nameof(height));
        if (frames.Length == 0)
            throw new ArgumentException("At least one animation frame is required.", nameof(frames));

        int frameByteLength = checked(width * height * 4);
        foreach (NativeAnimationFrameDescriptor frame in frames)
        {
            if (frame.DelayMilliseconds is < 20 or > 1_000
                || frame.PixelOffset < 0
                || frame.PixelOffset > view.Length - frameByteLength)
            {
                throw new ArgumentException("The animation frame descriptor is invalid.", nameof(frames));
            }
        }

        Width = width;
        Height = height;
        _view = view;
        _frames = frames;
        _frameByteLength = frameByteLength;
    }

    public int Width { get; }
    public int Height { get; }
    public int FrameCount => _frames.Length;

    public int GetDelayMilliseconds(int index)
        => _frames[index].DelayMilliseconds;

    public bool TryReadFrame(int index, NativeAnimationFrameSpanAction action)
    {
        ArgumentNullException.ThrowIfNull(action);
        if ((uint)index >= (uint)_frames.Length)
            return false;

        lock (_lifetimeGate)
        {
            SharedSectionView? view = _view;
            if (view is null)
                return false;

            NativeAnimationFrameDescriptor frame = _frames[index];
            action(view.Bytes.Slice(frame.PixelOffset, _frameByteLength));
            return true;
        }
    }

    public ImageWaveform? CreateWaveform(int index)
    {
        if ((uint)index >= (uint)_frames.Length)
            return null;

        lock (_lifetimeGate)
        {
            SharedSectionView? view = _view;
            if (view is null)
                return null;

            NativeAnimationFrameDescriptor frame = _frames[index];
            return ImageWaveformBuilder.Create(
                view.Bytes.Slice(frame.PixelOffset, _frameByteLength),
                Width,
                Height);
        }
    }

    public void Dispose()
    {
        lock (_lifetimeGate)
        {
            _view?.Dispose();
            _view = null;
        }
    }
}
