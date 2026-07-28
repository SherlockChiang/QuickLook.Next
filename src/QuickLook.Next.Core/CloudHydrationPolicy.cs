namespace QuickLook.Next.Core;

public enum CloudHydrationResult
{
    Completed,
    Deferred,
    LimitExceeded,
}

public static class CloudHydrationPolicy
{
    public const long MaxDownloadBytes = 256L * 1024 * 1024;

    public static bool IsDeclaredLengthAllowed(long length)
        => length >= 0 && length <= MaxDownloadBytes;

    public static int NextReadSize(long downloadedBytes, int bufferLength)
    {
        if (downloadedBytes < 0 || bufferLength <= 0 || downloadedBytes > MaxDownloadBytes)
            return 0;
        return (int)Math.Min(bufferLength, MaxDownloadBytes - downloadedBytes + 1);
    }

    public static int ProgressPercent(long downloadedBytes, long declaredLength)
    {
        if (declaredLength <= 0)
            return 0;
        return (int)Math.Clamp(downloadedBytes / (double)declaredLength * 100, 0, 100);
    }
}
