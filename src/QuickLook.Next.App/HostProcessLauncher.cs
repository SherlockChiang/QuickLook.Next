using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;

namespace QuickLook.Next.App;

internal static class HostProcessLauncher
{
    private const uint TokenAssignPrimary = 0x0001;
    private const uint TokenDuplicate = 0x0002;
    private const uint TokenQuery = 0x0008;
    private const uint DisableMaxPrivilege = 0x00000001;
    private const uint CreateSuspended = 0x00000004;
    private const uint CreateNoWindow = 0x08000000;

    public static Process StartRestricted(string executablePath, IEnumerable<string> arguments, HostProcessJob job)
    {
        if (!Path.IsPathFullyQualified(executablePath))
            throw new ArgumentException("Host executable path must be absolute.", nameof(executablePath));

        if (!OpenProcessToken(GetCurrentProcess(), TokenAssignPrimary | TokenDuplicate | TokenQuery, out nint processToken))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "OpenProcessToken failed.");
        try
        {
            if (!CreateRestrictedToken(processToken, DisableMaxPrivilege, 0, 0, 0, 0, 0, 0, out nint restrictedToken))
                throw new Win32Exception(Marshal.GetLastWin32Error(), "CreateRestrictedToken failed.");
            try
            {
                var startup = new StartupInfo { Cb = Marshal.SizeOf<StartupInfo>() };
                string commandLine = QuoteArgument(executablePath) + string.Concat(arguments.Select(argument => " " + QuoteArgument(argument)));
                var mutableCommandLine = new StringBuilder(commandLine);
                if (!CreateProcessAsUser(
                        restrictedToken,
                        executablePath,
                        mutableCommandLine,
                        0,
                        0,
                        false,
                        CreateSuspended | CreateNoWindow,
                        0,
                        Path.GetDirectoryName(executablePath),
                        ref startup,
                        out ProcessInformation information))
                {
                    throw new Win32Exception(Marshal.GetLastWin32Error(), "CreateProcessAsUser failed.");
                }

                try
                {
                    job.Assign(information.Process);
                    Process process = Process.GetProcessById(checked((int)information.ProcessId));
                    if (ResumeThread(information.Thread) == uint.MaxValue)
                    {
                        process.Dispose();
                        throw new Win32Exception(Marshal.GetLastWin32Error(), "ResumeThread failed.");
                    }
                    return process;
                }
                catch
                {
                    TerminateProcess(information.Process, 1);
                    throw;
                }
                finally
                {
                    CloseHandle(information.Thread);
                    CloseHandle(information.Process);
                }
            }
            finally
            {
                CloseHandle(restrictedToken);
            }
        }
        finally
        {
            CloseHandle(processToken);
        }
    }

    public static bool IsCurrentProcessInJob()
    {
        if (!IsProcessInJob(GetCurrentProcess(), 0, out bool inJob))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "IsProcessInJob failed.");
        return inJob;
    }

    public static bool CurrentProcessHasOnlyTraversalPrivilege()
    {
        if (!OpenProcessToken(GetCurrentProcess(), TokenQuery, out nint token))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "OpenProcessToken failed.");
        try
        {
            GetTokenInformation(token, 3, 0, 0, out int required);
            nint buffer = Marshal.AllocHGlobal(required);
            try
            {
                if (!GetTokenInformation(token, 3, buffer, required, out _))
                    throw new Win32Exception(Marshal.GetLastWin32Error(), "GetTokenInformation(TokenPrivileges) failed.");
                int count = Marshal.ReadInt32(buffer);
                int offset = sizeof(uint);
                int entrySize = Marshal.SizeOf<LuidAndAttributes>();
                for (int i = 0; i < count; i++)
                {
                    var entry = Marshal.PtrToStructure<LuidAndAttributes>(buffer + offset + i * entrySize);
                    if ((entry.Attributes & 0x2) == 0)
                        continue;
                    int nameLength = 0;
                    LookupPrivilegeName(null, ref entry.Luid, null, ref nameLength);
                    var name = new StringBuilder(nameLength + 1);
                    if (!LookupPrivilegeName(null, ref entry.Luid, name, ref nameLength)
                        || !string.Equals(name.ToString(), "SeChangeNotifyPrivilege", StringComparison.Ordinal))
                        return false;
                }
                return true;
            }
            finally
            {
                Marshal.FreeHGlobal(buffer);
            }
        }
        finally
        {
            CloseHandle(token);
        }
    }

    private static string QuoteArgument(string argument)
    {
        if (argument.Length > 0 && !argument.Any(static c => char.IsWhiteSpace(c) || c == '"'))
            return argument;
        var result = new StringBuilder("\"");
        int backslashes = 0;
        foreach (char character in argument)
        {
            if (character == '\\')
            {
                backslashes++;
                continue;
            }
            if (character == '"')
                result.Append('\\', backslashes * 2 + 1).Append('"');
            else
                result.Append('\\', backslashes).Append(character);
            backslashes = 0;
        }
        return result.Append('\\', backslashes * 2).Append('"').ToString();
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct StartupInfo
    {
        public int Cb;
        public string? Reserved;
        public string? Desktop;
        public string? Title;
        public int X;
        public int Y;
        public int XSize;
        public int YSize;
        public int XCountChars;
        public int YCountChars;
        public int FillAttribute;
        public int Flags;
        public short ShowWindow;
        public short Reserved2;
        public nint Reserved2Pointer;
        public nint StdInput;
        public nint StdOutput;
        public nint StdError;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct ProcessInformation
    {
        public nint Process;
        public nint Thread;
        public uint ProcessId;
        public uint ThreadId;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct Luid
    {
        public uint LowPart;
        public int HighPart;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct LuidAndAttributes
    {
        public Luid Luid;
        public uint Attributes;
    }

    [DllImport("kernel32.dll")]
    private static extern nint GetCurrentProcess();

    [DllImport("advapi32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool OpenProcessToken(nint process, uint desiredAccess, out nint token);

    [DllImport("advapi32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CreateRestrictedToken(
        nint existingToken, uint flags, uint disableSidCount, nint sidsToDisable,
        uint deletePrivilegeCount, nint privilegesToDelete, uint restrictedSidCount,
        nint sidsToRestrict, out nint newToken);

    [DllImport("advapi32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetTokenInformation(nint token, int tokenInformationClass, nint tokenInformation,
        int tokenInformationLength, out int returnLength);

    [DllImport("advapi32.dll", EntryPoint = "LookupPrivilegeNameW", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool LookupPrivilegeName(string? systemName, ref Luid luid, StringBuilder? name, ref int nameLength);

    [DllImport("advapi32.dll", EntryPoint = "CreateProcessAsUserW", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CreateProcessAsUser(
        nint token, string applicationName, StringBuilder commandLine,
        nint processAttributes, nint threadAttributes, [MarshalAs(UnmanagedType.Bool)] bool inheritHandles,
        uint creationFlags, nint environment, string? currentDirectory,
        ref StartupInfo startupInfo, out ProcessInformation processInformation);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint ResumeThread(nint thread);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool TerminateProcess(nint process, uint exitCode);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool IsProcessInJob(nint process, nint job, [MarshalAs(UnmanagedType.Bool)] out bool result);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(nint handle);
}
