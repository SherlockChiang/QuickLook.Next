using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Core;

namespace QuickLook.Next.ParserHost;

[Flags]
internal enum RetainedPreviewFollowUps
{
    None = 0,
    ArchiveEntry = 1,
}

internal sealed class RetainedPreviewSource(
    SafeFileHandle handle,
    long length,
    string logicalName,
    string sourceKind,
    RetainedPreviewFollowUps followUps) : IDisposable
{
    private readonly object _gate = new();
    private bool _disposed;

    public SafeFileHandle Handle { get; } = handle;
    public long Length { get; } = length;
    public string LogicalName { get; } = logicalName;
    public string SourceKind { get; } = sourceKind;
    public RetainedPreviewFollowUps FollowUps { get; } = followUps;

    public bool TryAcquire(
        RetainedPreviewFollowUps followUp,
        out RetainedPreviewSourceLease? lease)
    {
        lock (_gate)
        {
            lease = null;
            if (_disposed
                || Handle.IsClosed
                || Handle.IsInvalid
                || (FollowUps & followUp) != followUp
                || SourceKind is not ("archive" or "ebook"))
            {
                return false;
            }

            try
            {
                var leaseHandle = WindowsHandleTransfer.ReopenReadOnlyFile(Handle, Length);
                lease = new RetainedPreviewSourceLease(leaseHandle, Length, LogicalName);
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

internal sealed class RetainedPreviewSourceLease(
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
