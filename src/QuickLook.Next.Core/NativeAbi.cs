namespace QuickLook.Next.Core;

public static class NativeAbi
{
    public const uint Version = 1;

    public static void EnsureCompatible(uint actual)
    {
        if (actual != Version)
            throw new InvalidOperationException($"Native ABI mismatch: expected {Version}, received {actual}.");
    }
}
