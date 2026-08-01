using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal readonly record struct NativeAnimationFrameDescriptor(
    int DelayMilliseconds,
    int PixelOffset);

/// <summary>
/// Owns the App-side duplicate of a RasterHost animation section. Frame pixels stay in
/// the read-only mapping and are exposed only while the lifetime gate is held.
/// </summary>
internal sealed class NativeAnimationFrames : IDisposable
{
    private readonly ReaderWriterLockSlim _lifetimeGate = new(LockRecursionPolicy.NoRecursion);
    private readonly NativeAnimationFrameDescriptor[] _frames;
    private readonly ImageWaveform?[] _waveforms;
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
        _waveforms = new ImageWaveform?[frames.Length];
        _frameByteLength = frameByteLength;
    }

    public int Width { get; }
    public int Height { get; }
    public int FrameCount => _frames.Length;

    public int GetDelayMilliseconds(int index)
        => _frames[index].DelayMilliseconds;

    public bool TryWriteFrame(int index, Stream destination)
    {
        ArgumentNullException.ThrowIfNull(destination);
        if ((uint)index >= (uint)_frames.Length)
            return false;

        _lifetimeGate.EnterReadLock();
        try
        {
            SharedSectionView? view = _view;
            if (view is null)
                return false;

            NativeAnimationFrameDescriptor frame = _frames[index];
            destination.Write(view.Bytes.Slice(frame.PixelOffset, _frameByteLength));
            return true;
        }
        finally
        {
            _lifetimeGate.ExitReadLock();
        }
    }

    public ImageWaveform? CreateWaveform(int index)
    {
        if ((uint)index >= (uint)_frames.Length)
            return null;

        _lifetimeGate.EnterReadLock();
        try
        {
            SharedSectionView? view = _view;
            if (view is null)
                return null;

            ImageWaveform? cached = Volatile.Read(ref _waveforms[index]);
            if (cached is not null)
                return cached;

            NativeAnimationFrameDescriptor frame = _frames[index];
            var created = ImageWaveformBuilder.Create(
                view.Bytes.Slice(frame.PixelOffset, _frameByteLength),
                Width,
                Height);
            return Interlocked.CompareExchange(ref _waveforms[index], created, null) ?? created;
        }
        finally
        {
            _lifetimeGate.ExitReadLock();
        }
    }

    public void Dispose()
    {
        _lifetimeGate.EnterWriteLock();
        try
        {
            _view?.Dispose();
            _view = null;
            Array.Clear(_waveforms);
        }
        finally
        {
            _lifetimeGate.ExitWriteLock();
        }
    }
}
