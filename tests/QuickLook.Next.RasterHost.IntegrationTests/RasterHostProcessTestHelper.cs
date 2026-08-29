using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.CompilerServices;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

internal static class RasterHostProcessTestHelper
{
    // Outlive every budgeted terminal drain: 5s terminal workers + 10s metadata workers + 12s
    // PDF render drain, plus process startup, JIT, and disposal margin under heavy machine load
    // (the drains are wall-clock budgets). The exit-code assertion below still rejects fail-stop
    // paths and a genuine hang never exits, so the timeout only removes load flake.
    private static readonly TimeSpan ExitTimeout = TimeSpan.FromSeconds(45);
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
