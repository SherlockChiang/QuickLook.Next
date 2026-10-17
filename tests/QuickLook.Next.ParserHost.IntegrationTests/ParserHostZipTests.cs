using System.Diagnostics;
using System.IO.Pipes;
using System.Security.Cryptography;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.ParserHost.IntegrationTests;

public sealed partial class ParserHostIntegrationTests
{
    // Synthetic fixtures: secret.txt contains "secret\n", password test-password.
    // Generated and validated with 7-Zip 26.03:
    // 7z a -tzip fixture.zip secret.txt -ptest-password -mem=<encryption> -mm=Store
    // The staged host loads the production DLL, without Cargo's dev-feature unification.
    [Theory]
    [InlineData("AES128", "UEsDBDMAAQBjAEl/TF0AAAAAGwAAAAcAAAAKAAsAc2VjcmV0LnR4dAGZBwACAEFFAQAAQAS75nYA7ZlN6L54sy7quZNapVFcQ92l+nX8UEsBAj8AMwABAGMASX9MXQAAAAAbAAAABwAAAAoALwAAAAAAAAAgAAAAAAAAAHNlY3JldC50eHQKACAAAAAAAAEAGAD0qA1wH1rdAQAAAAAAAAAAAAAAAAAAAAABmQcAAgBBRQEAAFBLBQYAAAAAAQABAGcAAABOAAAAAAA=")]
    [InlineData("AES256", "UEsDBDMAAQBjAEl/TF0AAAAAIwAAAAcAAAAKAAsAc2VjcmV0LnR4dAGZBwACAEFFAwAAZttClGYpvpALKuQqPJwQEsC0iJ+1lmyUbrwf9uYOolo5B5dQSwECPwAzAAEAYwBJf0xdAAAAACMAAAAHAAAACgAvAAAAAAAAACAAAAAAAAAAc2VjcmV0LnR4dAoAIAAAAAAAAQAYAPSoDXAfWt0BAAAAAAAAAAAAAAAAAAAAAAGZBwACAEFFAwAAUEsFBgAAAAABAAEAZwAAAFYAAAAAAA==")]
    [InlineData("ZipCrypto", "UEsDBBQAAQAAAEl/TF2MsuviEwAAAAcAAAAKAAAAc2VjcmV0LnR4dCDcSrNd+Xix4Mdi4nuHf+Ds21RQSwECPwAUAAEAAABJf0xdjLLr4hMAAAAHAAAACgAkAAAAAAAAACAAAAAAAAAAc2VjcmV0LnR4dAoAIAAAAAAAAQAYAPSoDXAfWt0BAAAAAAAAAAAAAAAAAAAAAFBLBQYAAAAAAQABAFwAAAA7AAAAAAA=")]
    public async Task Archive_handle_lists_encrypted_zip_and_rejects_entry_extraction(
        string encryption, string fixtureBase64)
    {
        string tempDirectory = Path.Combine(Path.GetTempPath(), "QuickLookNextParserHostTests", Guid.NewGuid().ToString("n"));
        Directory.CreateDirectory(tempDirectory);
        string sourcePath = Path.Combine(tempDirectory, "physical-encrypted.bin");
        string logicalPath = Path.Combine(tempDirectory, encryption + ".zip");
        await File.WriteAllBytesAsync(sourcePath, Convert.FromBase64String(fixtureBase64));

        string pipeName = $"quicklook_next_parser_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        using Process host = StartHost(pipeName, token);
        try
        {
            using var timeout = new CancellationTokenSource(Timeout);
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<ParserReady>(await channel.ReceiveAsync(timeout.Token));

            string previewRequestId = Guid.NewGuid().ToString("n");
            var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(sourcePath);
            using var sourceHandle = pinned.Handle;
            long hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(sourceHandle, host.SafeHandle);
            var probe = new FileProbe(logicalPath, ".zip", [0x50, 0x4B, 0x03, 0x04])
            {
                Kind = "archive",
                Size = pinned.Length,
            };
            await channel.SendAsync(new PreviewOpenHandle(
                previewRequestId, hostHandle, pinned.Length, logicalPath, probe), timeout.Token);
            sourceHandle.Dispose();

            PreviewReady ready = Assert.IsType<PreviewReady>(await channel.ReceiveAsync(timeout.Token));
            Assert.Equal(previewRequestId, ready.RequestId);
            PreviewListing listing = Assert.IsType<PreviewListing>(ready.Listing);
            Assert.Equal("archive", listing.ListingKind);
            Assert.Equal(1, listing.EncryptedFileCount);
            PreviewListingItem entry = Assert.Single(listing.Items);
            Assert.Equal("secret.txt", entry.Path);
            Assert.Equal(7, entry.Size);
            Assert.True(entry.IsEncrypted);

            string extractRequestId = Guid.NewGuid().ToString("n");
            var output = CreateArchiveOutput(tempDirectory, extractRequestId);
            using FileStream outputWriter = output.Writer;
            long remoteOutputHandle = WindowsHandleTransfer.DuplicateFileToProcess(outputWriter.SafeFileHandle, host.SafeHandle);
            await channel.SendAsync(new ArchiveEntryExtract(
                extractRequestId, "", entry.Path, remoteOutputHandle, NativeAbi.MaxArchiveEntryOutputBytes)
            {
                ParentPreviewRequestId = previewRequestId,
            }, timeout.Token);

            PreviewError error = Assert.IsType<PreviewError>(await channel.ReceiveAsync(timeout.Token));
            Assert.Equal(extractRequestId, error.RequestId);
            using (FileStream outputReader = await WaitForArchiveOutputReaderAsync(outputWriter, output.Path, timeout.Token))
                Assert.Equal(0, outputReader.Length);
            Assert.Empty(EnumerateExtractionRoots(Path.Combine(GetWritableRoot(host), "archive-preview")));

            await channel.SendAsync(new PreviewClose(previewRequestId), timeout.Token);
            await WaitUntilAsync(() => TryOverwriteFile(sourcePath, "released"), timeout.Token);
            Assert.False(File.Exists(logicalPath));
        }
        finally
        {
            try { pipe.Dispose(); } catch { }
            await StopHostAsync(host);
            try { Directory.Delete(tempDirectory, recursive: true); } catch { }
        }
    }
}
