using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Core;
using QuickLook.Next.RasterHost;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

public sealed class NativeImageDecoderGateTests
{
    private static readonly byte[] OnePixelPng = Convert.FromBase64String(
        "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEElEQVR42mP4z8DwH4QZGBgAAL8BA/2t7mQAAAAASUVORK5CYII=");

    [Fact]
    public async Task Handle_decode_deadline_while_queued_returns_failure_without_releasing_the_gate()
    {
        // A held gate models a canceled decode that has not returned yet. The queued request's
        // deadline must produce a decode failure so the host still answers that request.
        using var heldGate = new SemaphoreSlim(0, 1);
        await WithPinnedPngAsync(async pinned =>
        {
            NativeDecodedImage? image = await NativeImageDecoder.TryDecodeHandleAsync(
                heldGate,
                pinned.Handle,
                pinned.Length,
                "queued.png",
                TimeSpan.FromMilliseconds(50),
                CancellationToken.None,
                64,
                64);

            Assert.Null(image);
            Assert.Equal(0, heldGate.CurrentCount);
            Assert.False(pinned.Handle.IsClosed);
        });
    }

    [Fact]
    public async Task Handle_decode_caller_cancellation_while_queued_still_propagates()
    {
        using var heldGate = new SemaphoreSlim(0, 1);
        using var request = new CancellationTokenSource();
        await WithPinnedPngAsync(async pinned =>
        {
            Task<NativeDecodedImage?> decode = NativeImageDecoder.TryDecodeHandleAsync(
                heldGate,
                pinned.Handle,
                pinned.Length,
                "queued.png",
                TimeSpan.FromSeconds(30),
                request.Token,
                64,
                64);
            request.Cancel();

            await Assert.ThrowsAnyAsync<OperationCanceledException>(() => decode);
            Assert.Equal(0, heldGate.CurrentCount);
            Assert.False(pinned.Handle.IsClosed);
        });
    }

    private static async Task WithPinnedPngAsync(Func<(SafeFileHandle Handle, long Length), Task> test)
    {
        string path = Path.Combine(Path.GetTempPath(), $"quicklook-native-gate-{Guid.NewGuid():N}.bin");
        await File.WriteAllBytesAsync(path, OnePixelPng);
        try
        {
            var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(path);
            using (pinned.Handle)
                await test(pinned);
        }
        finally
        {
            File.Delete(path);
        }
    }
}
