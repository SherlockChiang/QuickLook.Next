using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Core;

namespace QuickLook.Next.RasterHost;

[Flags]
internal enum RetainedRasterOperations
{
    None = 0,
    StaticImage = 1,
    Animation = 2,
    Metadata = 4,
}

internal sealed class RetainedRasterSource(
    SafeFileHandle handle,
    long length,
    string logicalName,
    RetainedRasterOperations operations) : IDisposable
{
    private readonly object _gate = new();
    private bool _disposed;

    public SafeFileHandle Handle { get; } = handle;
    public long Length { get; } = length;
    public string LogicalName { get; } = logicalName;
    public RetainedRasterOperations Operations { get; } = operations;

    public bool TryAcquire(RetainedRasterOperations operation, out RetainedRasterSourceLease? lease)
    {
        lock (_gate)
        {
            lease = null;
            if (_disposed
                || Handle.IsClosed
                || Handle.IsInvalid
                || (Operations & operation) != operation)
                return false;

            try
            {
                lease = new RetainedRasterSourceLease(
                    WindowsHandleTransfer.ReopenReadOnlyFile(Handle, Length),
                    Length,
                    LogicalName);
                return true;
            }
            catch
            {
                return false;
            }
        }
    }

    public void Dispose()
    {
        lock (_gate)
        {
            if (_disposed)
                return;
            _disposed = true;
            Handle.Dispose();
        }
    }
}

internal sealed class RetainedRasterSourceLease(
    SafeFileHandle handle,
    long length,
    string logicalName) : IDisposable
{
    private int _disposed;

    public SafeFileHandle Handle { get; } = handle;
    public long Length { get; } = length;
    public string LogicalName { get; } = logicalName;

    public void Dispose()
    {
        if (Interlocked.Exchange(ref _disposed, 1) == 0)
            Handle.Dispose();
    }
}
