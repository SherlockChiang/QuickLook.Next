using System.ComponentModel;
using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using Xunit;

namespace QuickLook.Next.Core.Tests;

[CollectionDefinition(Name, DisableParallelization = true)]
public sealed class SupervisedHostCrashProbeCollection
{
    public const string Name = "Supervised host crash probe";
}

[Collection(SupervisedHostCrashProbeCollection.Name)]
public sealed class SupervisedHostCrashProbeTests
{
    private const uint DxgiFacilityException = 0x0000087A;
    private const string DxgiMode = "dxgi";
    private const string FailFastMode = "failfast";
    private static readonly TimeSpan HandshakeTimeout = TimeSpan.FromSeconds(5);
    private static readonly TimeSpan ProbeTimeout = TimeSpan.FromSeconds(8);
    private static readonly TimeSpan ProcessStopTimeout = TimeSpan.FromSeconds(5);
    private static readonly TimeSpan PostExitWindowGrace = TimeSpan.FromSeconds(2);
    private static readonly TimeSpan WindowPollInterval = TimeSpan.FromMilliseconds(25);

    [Theory]
    [InlineData(DxgiMode)]
    [InlineData(FailFastMode)]
    public async Task Real_crash_exits_without_an_Application_Error_window(string mode)
    {
        if (!OperatingSystem.IsWindows())
            return;

        CrashProbeOutcome outcome = await RunCrashProbeAsync(mode);

        Assert.False(outcome.TimedOut, "Crash probe did not terminate within the fixed timeout.");
        Assert.Empty(outcome.ErrorWindows);
        Assert.NotEqual(0u, outcome.ExitCode);
        if (string.Equals(mode, DxgiMode, StringComparison.Ordinal))
            Assert.Equal(DxgiFacilityException, outcome.ExitCode);
    }

    private static async Task<CrashProbeOutcome> RunCrashProbeAsync(string mode)
    {
        string probePath = Path.Combine(
            AppContext.BaseDirectory,
            "CrashProbe",
            "QuickLook.Next.SupervisedHostCrashProbe.exe");
        Assert.True(File.Exists(probePath), $"Missing supervised-host crash probe: {probePath}");

        string pipeName = $"QuickLook.Next.SupervisedHostCrashProbe.{Guid.NewGuid():N}";
        string token = RandomNumberGenerator.GetHexString(32);
        string titleMarker = Path.GetFileNameWithoutExtension(probePath);
        HashSet<nint> baselineWindows = EnumerateVisibleWindows()
            .Select(window => window.Handle)
            .ToHashSet();
        var errorWindows = new Dictionary<nint, string>();

        using var pipe = new NamedPipeServerStream(
            pipeName,
            PipeDirection.InOut,
            maxNumberOfServerInstances: 1,
            PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        var startInfo = new ProcessStartInfo(probePath)
        {
            CreateNoWindow = true,
            UseShellExecute = false,
            WorkingDirectory = Path.GetDirectoryName(probePath)
                ?? throw new InvalidOperationException("Crash probe directory is unavailable."),
        };
        startInfo.ArgumentList.Add("--pipe");
        startInfo.ArgumentList.Add(pipeName);
        startInfo.ArgumentList.Add("--mode");
        startInfo.ArgumentList.Add(mode);
        startInfo.ArgumentList.Add("--token");
        startInfo.ArgumentList.Add(token);

        Process? probe = null;
        try
        {
            probe = Process.Start(startInfo)
                ?? throw new InvalidOperationException("Supervised-host crash probe did not start.");

            Task connection = pipe.WaitForConnectionAsync();
            await AwaitWithWindowMonitoringAsync(
                connection,
                probe,
                baselineWindows,
                titleMarker,
                errorWindows,
                "pipe connection");

            using var reader = new StreamReader(
                pipe,
                new UTF8Encoding(encoderShouldEmitUTF8Identifier: false),
                detectEncodingFromByteOrderMarks: false,
                bufferSize: 256,
                leaveOpen: true);
            using var writer = new StreamWriter(
                pipe,
                new UTF8Encoding(encoderShouldEmitUTF8Identifier: false),
                bufferSize: 256,
                leaveOpen: true)
            {
                AutoFlush = true,
            };

            Assert.Equal(
                $"READY {token}",
                await ReadProtocolLineAsync(
                    reader,
                    probe,
                    baselineWindows,
                    titleMarker,
                    errorWindows,
                    "READY"));
            await writer.WriteLineAsync($"ARM {token}");
            Assert.Equal(
                $"ARMED {token}",
                await ReadProtocolLineAsync(
                    reader,
                    probe,
                    baselineWindows,
                    titleMarker,
                    errorWindows,
                    "ARMED"));

            CollectErrorWindows(probe, baselineWindows, titleMarker, errorWindows);
            ThrowIfErrorWindowObserved(errorWindows);
            await writer.WriteLineAsync($"FIRE {token}");

            return await ObserveCrashAsync(
                probe,
                baselineWindows,
                titleMarker,
                errorWindows);
        }
        finally
        {
            if (probe is not null)
            {
                try
                {
                    await StopProbeAsync(probe);
                }
                finally
                {
                    probe.Dispose();
                }
            }
        }
    }

    private static async Task AwaitWithWindowMonitoringAsync(
        Task task,
        Process probe,
        HashSet<nint> baselineWindows,
        string titleMarker,
        Dictionary<nint, string> errorWindows,
        string stage)
    {
        var watch = Stopwatch.StartNew();
        while (!task.IsCompleted)
        {
            CollectErrorWindows(probe, baselineWindows, titleMarker, errorWindows);
            ThrowIfErrorWindowObserved(errorWindows);
            if (probe.HasExited)
            {
                throw new InvalidOperationException(
                    $"Crash probe exited with 0x{unchecked((uint)probe.ExitCode):X8} before {stage}.");
            }
            if (watch.Elapsed >= HandshakeTimeout)
                throw new TimeoutException($"Crash probe timed out during {stage}.");

            await Task.WhenAny(task, Task.Delay(WindowPollInterval));
        }

        await task;
        CollectErrorWindows(probe, baselineWindows, titleMarker, errorWindows);
        ThrowIfErrorWindowObserved(errorWindows);
    }

    private static async Task<string> ReadProtocolLineAsync(
        StreamReader reader,
        Process probe,
        HashSet<nint> baselineWindows,
        string titleMarker,
        Dictionary<nint, string> errorWindows,
        string stage)
    {
        Task<string?> read = reader.ReadLineAsync();
        await AwaitWithWindowMonitoringAsync(
            read,
            probe,
            baselineWindows,
            titleMarker,
            errorWindows,
            stage);
        return await read
            ?? throw new EndOfStreamException($"Crash probe closed the pipe before {stage}.");
    }

    private static async Task<CrashProbeOutcome> ObserveCrashAsync(
        Process probe,
        HashSet<nint> baselineWindows,
        string titleMarker,
        Dictionary<nint, string> errorWindows)
    {
        var watch = Stopwatch.StartNew();
        while (watch.Elapsed < ProbeTimeout)
        {
            CollectErrorWindows(probe, baselineWindows, titleMarker, errorWindows);
            if (errorWindows.Count > 0 || probe.HasExited)
                break;

            await Task.Delay(WindowPollInterval);
        }

        bool timedOut = errorWindows.Count == 0 && !probe.HasExited;
        if (!probe.HasExited)
            await StopProbeAsync(probe);
        else
            await probe.WaitForExitAsync().WaitAsync(ProcessStopTimeout);

        var grace = Stopwatch.StartNew();
        while (grace.Elapsed < PostExitWindowGrace)
        {
            CollectErrorWindows(probe, baselineWindows, titleMarker, errorWindows);
            await Task.Delay(WindowPollInterval);
        }

        return new CrashProbeOutcome(
            unchecked((uint)probe.ExitCode),
            timedOut,
            errorWindows.Values.Order(StringComparer.Ordinal).ToArray());
    }

    private static async Task StopProbeAsync(Process probe)
    {
        try
        {
            if (!probe.HasExited)
                probe.Kill(entireProcessTree: true);
        }
        catch (InvalidOperationException)
        {
            // The process exited between HasExited and Kill.
        }
        catch (Win32Exception) when (probe.HasExited)
        {
            // The process exited between HasExited and Kill.
        }

        await probe.WaitForExitAsync().WaitAsync(ProcessStopTimeout);
    }

    private static void CollectErrorWindows(
        Process probe,
        HashSet<nint> baselineWindows,
        string titleMarker,
        Dictionary<nint, string> errorWindows)
    {
        bool aliveBeforeEnumeration = !probe.HasExited;
        WindowInfo[] currentWindows = EnumerateVisibleWindows();
        bool aliveAfterEnumeration = !probe.HasExited;

        foreach (WindowInfo window in currentWindows)
        {
            bool belongsToLiveProbe = aliveBeforeEnumeration
                && aliveAfterEnumeration
                && window.OwnerProcessId == (uint)probe.Id;
            bool appearedDuringProbe = !baselineWindows.Contains(window.Handle);
            bool namesProbe = appearedDuringProbe
                && window.Title.Contains(titleMarker, StringComparison.OrdinalIgnoreCase);
            bool isApplicationError = appearedDuringProbe
                && string.Equals(window.ClassName, "#32770", StringComparison.Ordinal)
                && (window.Title.Contains("Application Error", StringComparison.OrdinalIgnoreCase)
                    || window.Title.Contains("应用程序错误", StringComparison.Ordinal));
            if (!belongsToLiveProbe && !namesProbe && !isApplicationError)
                continue;

            errorWindows.TryAdd(
                window.Handle,
                $"HWND=0x{window.Handle.ToInt64():X}; PID={window.OwnerProcessId}; "
                + $"Class={window.ClassName}; Title={window.Title}");
        }
    }

    private static WindowInfo[] EnumerateVisibleWindows()
    {
        var windows = new List<WindowInfo>();
        EnumWindowsProc callback = (window, unusedParameter) =>
        {
            if (!IsWindowVisible(window))
                return true;

            var title = new StringBuilder(512);
            _ = GetWindowText(window, title, title.Capacity);
            var className = new StringBuilder(128);
            _ = GetClassName(window, className, className.Capacity);
            _ = GetWindowThreadProcessId(window, out uint ownerProcessId);
            windows.Add(new WindowInfo(
                window,
                ownerProcessId,
                className.ToString(),
                title.ToString()));
            return true;
        };
        if (!EnumWindows(callback, nint.Zero))
        {
            int error = Marshal.GetLastPInvokeError();
            throw new Win32Exception(
                error,
                "EnumWindows failed while observing the supervised-host crash probe.");
        }

        return windows.ToArray();
    }

    private static void ThrowIfErrorWindowObserved(Dictionary<nint, string> errorWindows)
    {
        if (errorWindows.Count == 0)
            return;

        throw new InvalidOperationException(
            "Crash probe displayed an interactive error window: "
            + string.Join(" | ", errorWindows.Values.Order(StringComparer.Ordinal)));
    }

    private readonly record struct CrashProbeOutcome(
        uint ExitCode,
        bool TimedOut,
        string[] ErrorWindows);

    private readonly record struct WindowInfo(
        nint Handle,
        uint OwnerProcessId,
        string ClassName,
        string Title);

    private delegate bool EnumWindowsProc(nint window, nint parameter);

    [DllImport("user32.dll", ExactSpelling = true, SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool EnumWindows(EnumWindowsProc callback, nint parameter);

    [DllImport("user32.dll", ExactSpelling = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool IsWindowVisible(nint window);

    [DllImport("user32.dll", EntryPoint = "GetWindowTextW", CharSet = CharSet.Unicode)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern int GetWindowText(nint window, StringBuilder title, int maximumCount);

    [DllImport("user32.dll", EntryPoint = "GetClassNameW", CharSet = CharSet.Unicode)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern int GetClassName(nint window, StringBuilder className, int maximumCount);

    [DllImport("user32.dll", ExactSpelling = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern uint GetWindowThreadProcessId(nint window, out uint processId);
}
