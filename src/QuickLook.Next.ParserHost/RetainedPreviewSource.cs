using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Core;

namespace QuickLook.Next.ParserHost;

[Flags]
internal enum RetainedPreviewFollowUps
{
    None = 0,
    ArchiveEntry = 1,
    OfficeHero = 2,
    PackageHero = 4,
    OfficeLayoutImage = 8,
}

internal sealed class RetainedPreviewSource(
    SafeFileHandle handle,
    long length,
    string logicalName,
    string sourceKind,
    RetainedPreviewFollowUps followUps,
    IReadOnlyDictionary<string, long>? officeLayoutImages = null) : IDisposable
{
    private readonly object _gate = new();
    private readonly IReadOnlyDictionary<string, long> _officeLayoutImages =
        officeLayoutImages is null
            ? new Dictionary<string, long>(StringComparer.Ordinal)
            : new Dictionary<string, long>(officeLayoutImages, StringComparer.Ordinal);
    private bool _disposed;

    public SafeFileHandle Handle { get; } = handle;
    public long Length { get; } = length;
    public string LogicalName { get; } = logicalName;
    public string SourceKind { get; } = sourceKind;
    public RetainedPreviewFollowUps FollowUps { get; } = followUps;

    public bool TryAcquireOfficeLayoutImage(
        string imageRef,
        out long imageByteLength,
        out RetainedPreviewSourceLease? lease)
    {
        lock (_gate)
        {
            imageByteLength = 0;
            lease = null;
            if (_disposed
                || Handle.IsClosed
                || Handle.IsInvalid
                || SourceKind != "office"
                || (FollowUps & RetainedPreviewFollowUps.OfficeLayoutImage) == 0
                || !_officeLayoutImages.TryGetValue(imageRef, out imageByteLength))
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
                imageByteLength = 0;
                return false;
            }
        }
    }

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
                || !AllowsSourceKind(followUp))
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

    private bool AllowsSourceKind(RetainedPreviewFollowUps followUp)
        => followUp switch
        {
            RetainedPreviewFollowUps.ArchiveEntry => SourceKind is "archive" or "ebook",
            RetainedPreviewFollowUps.OfficeHero => SourceKind == "office",
            RetainedPreviewFollowUps.PackageHero => SourceKind == "package",
            RetainedPreviewFollowUps.OfficeLayoutImage => SourceKind == "office",
            _ => false,
        };

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
