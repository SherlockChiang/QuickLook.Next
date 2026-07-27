using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;

namespace QuickLook.Next.App;

// Explicit Main (DISABLE_XAML_GENERATED_MAIN): the App owns startup so it can wire the native bridge and
// host supervision. Spike learning: the auto-generated Main did not fire OnLaunched reliably here.
public static class Program
{
    private static Mutex? _singleInstanceMutex;
    private static readonly nint DpiAwarenessContextPerMonitorAwareV2 = new(-4);

    [STAThread]
    private static void Main(string[] args)
    {
        _ = SetProcessDpiAwarenessContext(DpiAwarenessContextPerMonitorAwareV2);
        AppStartupTiming.Start();
        if (args is ["--restricted-host-probe-child", var allowedProbeRoot, var deniedProbeRoot, var probePipeName])
        {
            try
            {
                if (!HostProcessLauncher.IsCurrentProcessInJob()) Environment.ExitCode = 10;
                else if (!HostProcessLauncher.CurrentProcessHasOnlyTraversalPrivilege()) Environment.ExitCode = 12;
                else if (!HostProcessLauncher.CurrentProcessIsWriteRestricted()) Environment.ExitCode = 17;
                else
                {
                    Environment.ExitCode = HostProcessLauncher.CurrentProcessMitigationStatus() switch
                    {
                        7 => 0,
                        int status when status < 0 => 100 + Math.Min(99, -status),
                        int status when (status & 1) == 0 => 13,
                        int status when (status & 2) == 0 => 15,
                        _ => 16,
                    };
                }
                if (Environment.ExitCode == 0 && !HostProcessJob.CurrentProcessHasRequiredPolicy())
                    Environment.ExitCode = 14;
                if (Environment.ExitCode == 0)
                {
                    File.WriteAllText(Path.Combine(allowedProbeRoot, "allowed.txt"), "allowed");
                    try
                    {
                        File.WriteAllText(Path.Combine(deniedProbeRoot, "denied.txt"), "denied");
                        Environment.ExitCode = 18;
                    }
                    catch (UnauthorizedAccessException) { }
                    if (Environment.ExitCode == 0)
                    {
                        using var pipe = new NamedPipeClientStream(
                            ".",
                            probePipeName,
                            PipeDirection.InOut,
                            PipeOptions.None);
                        pipe.Connect(5_000);
                        pipe.WriteByte(0x51);
                        if (pipe.ReadByte() != 0x4C)
                            Environment.ExitCode = 20;
                    }
                }
            }
            catch { Environment.ExitCode = 19; }
            return;
        }
        if (args is ["--smoke-restricted-host-launch"])
        {
            try
            {
                string probeRoot = Path.Combine(Path.GetTempPath(), "QuickLookNext", "RestrictedHostProbe", Guid.NewGuid().ToString("n"));
                string writableRoot = Path.Combine(probeRoot, "allowed");
                string deniedRoot = Path.Combine(probeRoot, "denied");
                string pipeName = $"quicklook_next_restricted_probe_{Environment.ProcessId}_{Guid.NewGuid():N}";
                Directory.CreateDirectory(writableRoot);
                Directory.CreateDirectory(deniedRoot);
                HostProcessLauncher.GrantRestrictedWriteAccess(writableRoot);
                using NamedPipeServerStream pipe = HostProcessLauncher.CreateWriteRestrictedPipe(pipeName);
                using var job = new HostProcessJob((nint)(128L * 1024 * 1024));
                using Process child = HostProcessLauncher.StartRestricted(
                    Environment.ProcessPath ?? throw new InvalidOperationException("Current process path is unavailable."),
                    ["--restricted-host-probe-child", writableRoot, deniedRoot, pipeName],
                    job,
                    restrictWrites: true);
                using var pipeTimeout = new CancellationTokenSource(TimeSpan.FromSeconds(10));
                Task connected = pipe.WaitForConnectionAsync(pipeTimeout.Token);
                Task exited = child.WaitForExitAsync(pipeTimeout.Token);
                Task first = Task.WhenAny(connected, exited).GetAwaiter().GetResult();
                if (ReferenceEquals(first, exited))
                    Environment.ExitCode = child.ExitCode;
                else
                {
                    connected.GetAwaiter().GetResult();
                    if (pipe.ReadByte() != 0x51)
                        Environment.ExitCode = 22;
                    else
                        pipe.WriteByte(0x4C);
                }
                if (!child.WaitForExit(10_000))
                    Environment.ExitCode = 3;
                else if (Environment.ExitCode == 0)
                    Environment.ExitCode = child.ExitCode;
                try { Directory.Delete(probeRoot, recursive: true); } catch { }
            }
            catch (System.ComponentModel.Win32Exception ex) { Environment.ExitCode = 1000 + ex.NativeErrorCode; }
            catch (UnauthorizedAccessException) { Environment.ExitCode = 23; }
            catch (IOException) { Environment.ExitCode = 24; }
            catch (ArgumentException) { Environment.ExitCode = 25; }
            catch (PlatformNotSupportedException) { Environment.ExitCode = 26; }
            catch
            {
                Environment.ExitCode = 21;
            }
            return;
        }
        if (args is ["--smoke-write-restricted-parser-host", var parserHostPath])
        {
            string sourcePath = Path.Combine(Path.GetTempPath(), $"quicklook-parser-smoke-{Guid.NewGuid():N}.txt");
            var supervisor = new ParserHostSupervisor(parserHostPath);
            try
            {
                const string contents = "write-restricted parser HANDLE smoke";
                File.WriteAllText(sourcePath, contents);
                using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(15));
                supervisor.EnsureStartedAsync(timeout.Token).GetAwaiter().GetResult();
                var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(sourcePath);
                using (pinned.Handle)
                {
                    string logicalPath = Path.Combine(Path.GetTempPath(), $"missing-parser-smoke-{Guid.NewGuid():N}.txt");
                    var probe = new FileProbe(logicalPath, ".txt", "write-restricted"u8.ToArray())
                    {
                        Kind = "text",
                        Size = pinned.Length,
                    };
                    var (_, completion) = supervisor.BeginOpenHandle(
                        logicalPath,
                        probe,
                        pinned.Handle,
                        pinned.Length,
                        TimeSpan.FromSeconds(10));
                    PreviewReady ready = completion.WaitAsync(timeout.Token).GetAwaiter().GetResult() as PreviewReady
                        ?? throw new InvalidDataException("Write-restricted ParserHost returned no preview.");
                    if (!string.Equals(ready.TextContent, contents, StringComparison.Ordinal))
                        throw new InvalidDataException("Write-restricted ParserHost returned unexpected content.");
                }
                Environment.ExitCode = 0;
            }
            catch (System.ComponentModel.Win32Exception ex) { Environment.ExitCode = 1000 + ex.NativeErrorCode; }
            catch { Environment.ExitCode = 27; }
            finally
            {
                supervisor.Stop();
                try { File.Delete(sourcePath); } catch { }
            }
            return;
        }

        // Single-instance guard: if another instance is already running (holding the named pipe),
        // exit immediately instead of becoming a broken tray-zombie process.
        _singleInstanceMutex = new Mutex(initiallyOwned: true, name: @"Global\QuickLook.Next.App", out bool createdNew);
        if (!createdNew)
            return;

        WinRT.ComWrappersSupport.InitializeComWrappers();
        Application.Start(_ =>
        {
            var context = new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread());
            System.Threading.SynchronizationContext.SetSynchronizationContext(context);
            new App();
        });
    }

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetProcessDpiAwarenessContext(nint value);
}
