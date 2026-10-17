using QuickLook.Next.Core;
using QuickLook.Next.RasterHost;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

public sealed class SystemImageCancellationTests
{
    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public async Task Canceled_decode_preserves_pinned_source_and_allows_fresh_decode(bool enableHdr)
    {
        string path = Path.Combine(Path.GetTempPath(), $"quicklook-system-cancel-{Guid.NewGuid():N}.bin");
        byte[] bytes = Convert.FromBase64String(
            "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEElEQVR42mP4z8DwH4QZGBgAAL8BA/2t7mQAAAAASUVORK5CYII=");
        await File.WriteAllBytesAsync(path, bytes);
        try
        {
            var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(path);
            using (pinned.Handle)
            using (var canceled = new CancellationTokenSource())
            {
                canceled.Cancel();
                string logicalName = Path.Combine(Path.GetTempPath(), $"missing-{Guid.NewGuid():N}.png");
                await Assert.ThrowsAnyAsync<OperationCanceledException>(() =>
                    SystemImageDecoder.TryDecodeHandleAsync(
                        pinned.Handle, pinned.Length, logicalName, canceled.Token, enableHdr: enableHdr));
                Assert.False(pinned.Handle.IsClosed);
                Assert.False(pinned.Handle.IsInvalid);
                Assert.Throws<IOException>(() =>
                {
                    using var writer = new FileStream(path, FileMode.Open, FileAccess.Write, FileShare.ReadWrite);
                });

                NativeDecodedImage? image = await SystemImageDecoder.TryDecodeHandleAsync(
                    pinned.Handle, pinned.Length, logicalName, CancellationToken.None, enableHdr: enableHdr);
                Assert.NotNull(image);
                Assert.Equal(2, image.Width);
                Assert.Equal(1, image.Height);
                Assert.Equal(8, image.Bgra.Length);
                Assert.False(pinned.Handle.IsClosed);
            }
            using var recovered = new FileStream(path, FileMode.Open, FileAccess.Write, FileShare.ReadWrite);
        }
        finally
        {
            File.Delete(path);
        }
    }
}
