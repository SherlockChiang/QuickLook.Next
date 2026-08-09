using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.CompilerServices;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

internal static class RasterHostProcessTestHelper
{
    // Outlive the host's 5-second terminal-worker drain plus its 12-second PDF render drain.
    // The exit-code assertion below still rejects either fail-stop path instead of masking it.
    private static readonly TimeSpan ExitTimeout = TimeSpan.FromSeconds(20);
    private static readonly TimeSpan KillTimeout = TimeSpan.FromSeconds(2);
    private static readonly ConditionalWeakTable<Process, CompletionState> CompletionStates = new();

    public static async Task<bool> CompleteAsync(NamedPipeServerStream pipe, Process host)
    {
        CompletionState state = CompletionStates.GetValue(host, static _ => new CompletionState());
        state.Outcome = CompletionOutcome.Unknown;
        state.WaitFailure = null;

        try
        {
            pipe.Dispose();
        }
        catch
        {
        }

        try
        {
            await host.WaitForExitAsync().WaitAsync(ExitTimeout);
            state.Outcome = CompletionOutcome.Exited;
            return true;
        }
        catch (TimeoutException)
        {
            state.Outcome = CompletionOutcome.TimedOut;
            await KillAndWaitAsync(host);
            return false;
        }
        catch (Exception exception)
        {
            state.Outcome = CompletionOutcome.WaitFailed;
            state.WaitFailure = exception;
            await KillAndWaitAsync(host);
            return false;
        }
    }

    public static void AssertCleanExit(Process host, bool exited)
    {
        CompletionStates.TryGetValue(host, out CompletionState? state);
        CompletionStates.Remove(host);
        CompletionOutcome outcome = state?.Outcome ?? CompletionOutcome.Unknown;
        string failureMessage = outcome switch
        {
            CompletionOutcome.TimedOut =>
                $"RasterHost process {host.Id} did not exit within {ExitTimeout}.",
            CompletionOutcome.WaitFailed =>
                $"Waiting for RasterHost process {host.Id} failed: "
                + $"{state?.WaitFailure?.GetType().Name}: {state?.WaitFailure?.Message}",
            CompletionOutcome.Exited =>
                $"RasterHost process {host.Id} reported an inconsistent exit result.",
            _ =>
                $"RasterHost process {host.Id} has no recorded exit result.",
        };

        Assert.True(exited && outcome == CompletionOutcome.Exited, failureMessage);
        Assert.Equal(0, host.ExitCode);
    }

    private static async Task KillAndWaitAsync(Process host)
    {
        TryKill(host);
        try
        {
            await host.WaitForExitAsync().WaitAsync(KillTimeout);
        }
        catch
        {
        }
    }

    private static void TryKill(Process host)
    {
        try
        {
            if (!host.HasExited)
                host.Kill();
        }
        catch
        {
        }
    }

    private enum CompletionOutcome
    {
        Unknown,
        Exited,
        TimedOut,
        WaitFailed,
    }

    private sealed class CompletionState
    {
        public CompletionOutcome Outcome { get; set; }

        public Exception? WaitFailure { get; set; }
    }
}
