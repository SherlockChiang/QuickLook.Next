using System.Diagnostics.CodeAnalysis;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Text;
using QuickLook.Next.Core;

SupervisedHostProcessPolicy.SuppressInteractiveErrorUi();
return await SupervisedHostCrashProbe.RunAsync(args).ConfigureAwait(false);

internal static class SupervisedHostCrashProbe
{
    private const uint DxgiFacilityException = 0x0000087A;
    private const uint ExceptionNoncontinuable = 0x00000001;
    private const string DxgiMode = "dxgi";
    private const string FailFastMode = "failfast";

    public static async Task<int> RunAsync(string[] args)
    {
        if (!TryGetArgument(args, "--pipe", out string pipeName)
            || !TryGetArgument(args, "--mode", out string mode)
            || !TryGetArgument(args, "--token", out string token)
            || !IsValidMode(mode)
            || !IsValidToken(token))
        {
            return 2;
        }

        using var pipe = new NamedPipeClientStream(
            ".",
            pipeName,
            PipeDirection.InOut,
            PipeOptions.Asynchronous);
        await pipe.ConnectAsync(5_000).ConfigureAwait(false);

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

        await writer.WriteLineAsync($"READY {token}").ConfigureAwait(false);
        if (!string.Equals(
                await reader.ReadLineAsync().ConfigureAwait(false),
                $"ARM {token}",
                StringComparison.Ordinal))
        {
            return 3;
        }

        await writer.WriteLineAsync($"ARMED {token}").ConfigureAwait(false);
        if (!string.Equals(
                await reader.ReadLineAsync().ConfigureAwait(false),
                $"FIRE {token}",
                StringComparison.Ordinal))
        {
            return 4;
        }

        Crash(mode);
        return 5;
    }

    [DoesNotReturn]
    private static void Crash(string mode)
    {
        if (string.Equals(mode, DxgiMode, StringComparison.Ordinal))
        {
            var exceptionRecord = new NativeExceptionRecord
            {
                ExceptionCode = DxgiFacilityException,
                ExceptionFlags = ExceptionNoncontinuable,
            };
            RaiseFailFastException(ref exceptionRecord, nint.Zero, 0);
            Environment.FailFast("RaiseFailFastException unexpectedly returned.");
        }

        Environment.FailFast("QuickLook Next supervised-host no-dialog probe.");
    }

    private static bool TryGetArgument(string[] args, string name, out string value)
    {
        value = string.Empty;
        for (int index = 0; index + 1 < args.Length; index += 2)
        {
            if (!string.Equals(args[index], name, StringComparison.Ordinal))
                continue;

            value = args[index + 1];
            return !string.IsNullOrWhiteSpace(value);
        }

        return false;
    }

    private static bool IsValidMode(string mode) =>
        string.Equals(mode, DxgiMode, StringComparison.Ordinal)
        || string.Equals(mode, FailFastMode, StringComparison.Ordinal);

    private static bool IsValidToken(string token)
    {
        if (token.Length != 32)
            return false;

        foreach (char character in token)
        {
            if (!(character is >= '0' and <= '9')
                && !(character is >= 'A' and <= 'F'))
            {
                return false;
            }
        }

        return true;
    }

    [DllImport("kernel32.dll", ExactSpelling = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern void RaiseFailFastException(
        ref NativeExceptionRecord exceptionRecord,
        nint contextRecord,
        uint flags);

    [StructLayout(LayoutKind.Explicit, Size = 152)]
    private struct NativeExceptionRecord
    {
        [FieldOffset(0)]
        public uint ExceptionCode;

        [FieldOffset(4)]
        public uint ExceptionFlags;

        [FieldOffset(8)]
        private nint NestedExceptionRecord;

        [FieldOffset(16)]
        private nint ExceptionAddress;

        [FieldOffset(24)]
        private uint NumberParameters;
    }
}
