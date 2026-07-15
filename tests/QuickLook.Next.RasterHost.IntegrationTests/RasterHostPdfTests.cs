using System.Diagnostics;
using System.IO.Pipes;
using System.Security.Cryptography;
using System.Text;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

public sealed class RasterHostPdfTests
{
    [Fact]
    public async Task Handle_backed_pdf_renders_a_page_and_closes_its_anchor()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(30));
        string pipeName = $"quicklook_next_raster_pdf_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        string hostPath = Path.Combine(AppContext.BaseDirectory, "RasterHost", "QuickLook.Next.RasterHost.exe");
        using Process host = Process.Start(new ProcessStartInfo(hostPath)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            ArgumentList = { "--pipe", pipeName, "--session-token", token },
        }) ?? throw new InvalidOperationException("RasterHost did not start");
        string path = Path.Combine(Path.GetTempPath(), $"quicklook-next-{Guid.NewGuid():N}.pdf");

        try
        {
            await File.WriteAllBytesAsync(path, CreateOnePagePdf(), timeout.Token);
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(path);
            long hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(pinned.Handle, host.SafeHandle);
            pinned.Handle.Dispose();
            string requestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
            var probe = new FileProbe(path, ".pdf", "%PDF"u8.ToArray())
            {
                Kind = "pdf",
                Size = pinned.Length,
            };
            await channel.SendAsync(new PreviewOpenHandle(requestId, hostHandle, pinned.Length, path, probe), timeout.Token);

            string anchorPath = Path.Combine(
                Path.GetTempPath(), "QuickLookNext", "raster-inputs", host.Id.ToString(), "input-" + requestId, "source.pdf");
            var ready = Assert.IsType<PreviewReady>(await ReceiveUntilAsync<PreviewReady>(channel, timeout.Token));
            Assert.Equal("pdf", ready.Kind);
            Assert.Equal(1, ready.PageCount);
            Assert.Equal(400d, ready.PageWidth, precision: 3);
            Assert.Equal(266.667d, ready.PageHeight, precision: 3);
            Assert.True(File.Exists(anchorPath));
            File.WriteAllText(path, "not the requested pdf");

            await channel.SendAsync(new PreviewPageOpen(requestId, 1, 1, 1), timeout.Token);
            var pageError = Assert.IsType<PreviewPageError>(await ReceiveUntilAsync<PreviewPageError>(channel, timeout.Token));
            Assert.Equal((requestId, 1, 1L), (pageError.RequestId, pageError.PageIndex, pageError.PageGeneration));
            Assert.False(pageError.TimedOut);

            await channel.SendAsync(new PreviewPageOpen(requestId, 0, 2, 1), timeout.Token);
            var surface = Assert.IsType<PreviewSurface>(await ReceiveUntilAsync<PreviewSurface>(channel, timeout.Token));
            Assert.Equal((requestId, 0, 2L), (surface.RequestId, surface.PageIndex, surface.PageGeneration));
            Assert.InRange(surface.Width, 1u, 2200u);
            Assert.InRange(surface.Height, 1u, 2200u);
            using (var localSurface = new Microsoft.Win32.SafeHandles.SafeFileHandle(
                WindowsHandleTransfer.DuplicateHandleFromProcess(host.SafeHandle, surface.SharedHandle), ownsHandle: true))
                Assert.False(localSurface.IsInvalid);
            await channel.SendAsync(new PreviewSurfaceRelease(surface.TransferId), timeout.Token);

            await channel.SendAsync(new PreviewClose(requestId), timeout.Token);
            while (Directory.Exists(Path.GetDirectoryName(anchorPath)))
                await Task.Delay(10, timeout.Token);
        }
        finally
        {
            try { pipe.Dispose(); } catch { }
            try { await host.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(5)); }
            catch { try { host.Kill(entireProcessTree: true); } catch { } }
            try { File.Delete(path); } catch { }
        }
    }

    private static async Task<ControlMessage> ReceiveUntilAsync<T>(PipeChannel channel, CancellationToken cancellationToken)
        where T : ControlMessage
    {
        while (true)
        {
            ControlMessage message = await channel.ReceiveAsync(cancellationToken)
                ?? throw new EndOfStreamException("RasterHost closed before completing the PDF request.");
            if (message is PreviewError error)
                throw new Xunit.Sdk.XunitException(error.Message);
            if (message is T)
                return message;
        }
    }

    private static byte[] CreateOnePagePdf()
    {
        string[] objects =
        [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << >> /Contents 4 0 R >>",
            "<< /Length 0 >>\nstream\n\nendstream",
        ];
        using var stream = new MemoryStream();
        WriteAscii(stream, "%PDF-1.4\n");
        var offsets = new List<long> { 0 };
        for (int i = 0; i < objects.Length; i++)
        {
            offsets.Add(stream.Position);
            WriteAscii(stream, $"{i + 1} 0 obj\n{objects[i]}\nendobj\n");
        }
        long xref = stream.Position;
        WriteAscii(stream, $"xref\n0 {objects.Length + 1}\n0000000000 65535 f \n");
        for (int i = 1; i < offsets.Count; i++)
            WriteAscii(stream, $"{offsets[i]:D10} 00000 n \n");
        WriteAscii(stream, $"trailer\n<< /Size {objects.Length + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n");
        return stream.ToArray();
    }

    private static void WriteAscii(Stream stream, string value)
        => stream.Write(Encoding.ASCII.GetBytes(value));
}
