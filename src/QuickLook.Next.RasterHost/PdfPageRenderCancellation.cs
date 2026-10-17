using System.Collections.Concurrent;
using QuickLook.Next.Core;

namespace QuickLook.Next.RasterHost;

internal static class PdfPageRenderCancellation
{
    public static void CancelSuperseded(
        ConcurrentDictionary<(string RequestId, int PageIndex, long PageGeneration), CancellationTokenSource> requests,
        string requestId,
        int pageIndex,
        long pageGeneration)
    {
        foreach (var pair in requests)
        {
            var key = pair.Key;
            if (!string.Equals(key.RequestId, requestId, StringComparison.Ordinal)
                || key.PageIndex != pageIndex
                || key.PageGeneration >= pageGeneration
                || !((ICollection<KeyValuePair<(string RequestId, int PageIndex, long PageGeneration), CancellationTokenSource>>)requests).Remove(pair))
            {
                continue;
            }

            // Only cancel the obsolete waiter. Its worker still owns CTS disposal,
            // and the session retains the shared WinRT render until it drains.
            try { pair.Value.Cancel(); } catch (ObjectDisposedException) { }
            DiagLog.Write(
                "RasterHost",
                $"page render superseded: request={requestId} page={pageIndex}; old-generation={key.PageGeneration}; new-generation={pageGeneration}");
        }
    }
}
