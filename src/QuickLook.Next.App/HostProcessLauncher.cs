using System.ComponentModel;
using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;

namespace QuickLook.Next.App;

internal static class HostProcessLauncher
{
    private const uint TokenAssignPrimary = 0x0001;
    private const uint TokenDuplicate = 0x0002;
    private const uint TokenQuery = 0x0008;
    private const uint DisableMaxPrivilege = 0x00000001;
    private const uint WriteRestricted = 0x00000008;
    private const uint CreateSuspended = 0x00000004;
    private const uint ExtendedStartupInfoPresent = 0x00080000;
    private const uint CreateNoWindow = 0x08000000;
    private const uint SemFailCriticalErrors = 0x0001;
    private const uint SemNoGpFaultErrorBox = 0x0002;
    private const uint SemNoOpenFileErrorBox = 0x8000;
    private const uint RequiredChildErrorMode =
        SemFailCriticalErrors | SemNoGpFaultErrorBox | SemNoOpenFileErrorBox;
    private const nuint ProcThreadAttributeMitigationPolicy = 0x00020007;
    private const ulong RequiredMitigationPolicy = 0x0000000100111005;

    private static readonly object ProcessCreationLock = new();
    private static readonly SecurityIdentifier RestrictedCodeSid = new("S-1-5-12");
    // WRITE_RESTRICTED consults these SIDs only for write access. World permits CLR/BCrypt kernel
    // object initialization; Restricted Code remains the explicit grant for host output and pipes.
    private static readonly SecurityIdentifier WorldSid = new(WellKnownSidType.WorldSid, null);

    public static Process StartRestricted(
        string executablePath,
        IEnumerable<string> arguments,
        HostProcessJob job,
        bool restrictWrites = false)
    {
        if (!Path.IsPathFullyQualified(executablePath))
            throw new ArgumentException("Host executable path must be absolute.", nameof(executablePath));

        if (!OpenProcessToken(GetCurrentProcess(), TokenAssignPrimary | TokenDuplicate | TokenQuery, out nint processToken))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "OpenProcessToken failed.");
        try
        {
            byte[][] restrictedSidValues = restrictWrites
                ? [GetSidBytes(RestrictedCodeSid), GetSidBytes(WorldSid)]
                : [];
            nint[] restrictedSidBytes = restrictedSidValues
                .Select(value => Marshal.AllocHGlobal(value.Length))
                .ToArray();
            int sidEntrySize = Marshal.SizeOf<SidAndAttributes>();
            nint restrictedSids = restrictedSidValues.Length == 0
                ? 0
                : Marshal.AllocHGlobal(sidEntrySize * restrictedSidValues.Length);
            try
            {
                for (int i = 0; i < restrictedSidValues.Length; i++)
                {
                    Marshal.Copy(restrictedSidValues[i], 0, restrictedSidBytes[i], restrictedSidValues[i].Length);
                    Marshal.StructureToPtr(
                        new SidAndAttributes { Sid = restrictedSidBytes[i], Attributes = 0 },
                        restrictedSids + i * sidEntrySize,
                        fDeleteOld: false);
                }
                if (!CreateRestrictedToken(
                        processToken,
                        DisableMaxPrivilege | (restrictWrites ? WriteRestricted : 0),
                        0,
                        0,
                        0,
                        0,
                        (uint)restrictedSidValues.Length,
                        restrictedSids,
                        out nint restrictedToken))
                {
                    throw new Win32Exception(Marshal.GetLastWin32Error(), "CreateRestrictedToken failed.");
                }
                try
                {
                    nuint attributeListSize = 0;
                    InitializeProcThreadAttributeList(0, 1, 0, ref attributeListSize);
                    nint attributeList = Marshal.AllocHGlobal(checked((int)attributeListSize));
                    nint mitigationPolicy = Marshal.AllocHGlobal(sizeof(long));
                    var startup = new StartupInfoEx
                    {
                        StartupInfo = new StartupInfo { Cb = Marshal.SizeOf<StartupInfoEx>() },
                        AttributeList = attributeList,
                    };
                    string commandLine = QuoteArgument(executablePath) + string.Concat(arguments.Select(argument => " " + QuoteArgument(argument)));
                    var mutableCommandLine = new StringBuilder(commandLine);
                    try
                    {
                        if (!InitializeProcThreadAttributeList(attributeList, 1, 0, ref attributeListSize))
                            throw new Win32Exception(Marshal.GetLastWin32Error(), "InitializeProcThreadAttributeList failed.");
                        Marshal.WriteInt64(mitigationPolicy, unchecked((long)RequiredMitigationPolicy));
                        if (!UpdateProcThreadAttribute(
                                attributeList, 0, ProcThreadAttributeMitigationPolicy,
                                mitigationPolicy, (nuint)sizeof(long), 0, 0))
                        {
                            throw new Win32Exception(Marshal.GetLastWin32Error(), "UpdateProcThreadAttribute failed.");
                        }
                        ProcessInformation information = default;
                        bool processCreated;
                        int processCreationError = 0;
                        lock (ProcessCreationLock)
                        {
                            uint originalErrorMode = GetErrorMode();
                            try
                            {
                                _ = SetErrorMode(originalErrorMode | RequiredChildErrorMode);
                                processCreated = CreateProcessAsUser(
                                    restrictedToken,
                                    executablePath,
                                    mutableCommandLine,
                                    0,
                                    0,
                                    false,
                                    CreateSuspended | CreateNoWindow | ExtendedStartupInfoPresent,
                                    0,
                                    Path.GetDirectoryName(executablePath),
                                    ref startup,
                                    out information);
                                if (!processCreated)
                                    processCreationError = Marshal.GetLastWin32Error();
                            }
                            finally
                            {
                                _ = SetErrorMode(originalErrorMode);
                            }
                        }
                        if (!processCreated)
                        {
                            throw new Win32Exception(processCreationError, "CreateProcessAsUser failed.");
                        }

                        try
                        {
                            job.Assign(information.Process);
                            Process process = Process.GetProcessById(checked((int)information.ProcessId));
                            _ = process.Handle;
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
                        DeleteProcThreadAttributeList(attributeList);
                        Marshal.FreeHGlobal(mitigationPolicy);
                        Marshal.FreeHGlobal(attributeList);
                    }
                }
                finally
                {
                    CloseHandle(restrictedToken);
                }
            }
            finally
            {
                if (restrictedSids != 0) Marshal.FreeHGlobal(restrictedSids);
                foreach (nint sidBytes in restrictedSidBytes) Marshal.FreeHGlobal(sidBytes);
            }
        }
        finally
        {
            CloseHandle(processToken);
        }
    }

    public static void GrantRestrictedWriteAccess(string directory)
    {
        var info = new DirectoryInfo(directory);
        DirectorySecurity security = info.GetAccessControl();
        security.AddAccessRule(new FileSystemAccessRule(
            RestrictedCodeSid,
            FileSystemRights.Modify | FileSystemRights.Synchronize,
            InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit,
            PropagationFlags.None,
            AccessControlType.Allow));
        info.SetAccessControl(security);
    }

    public static void GrantRestrictedReadAccess(string directory)
    {
        var info = new DirectoryInfo(directory);
        DirectorySecurity security = info.GetAccessControl();
        security.AddAccessRule(new FileSystemAccessRule(
            RestrictedCodeSid,
            FileSystemRights.ReadAndExecute | FileSystemRights.Synchronize,
            InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit,
            PropagationFlags.None,
            AccessControlType.Allow));
        info.SetAccessControl(security);
    }

    public static NamedPipeServerStream CreateWriteRestrictedPipe(string pipeName)
    {
        SecurityIdentifier currentUser = WindowsIdentity.GetCurrent().User
            ?? throw new InvalidOperationException("Current user SID is unavailable.");
        var security = new PipeSecurity();
        security.AddAccessRule(new PipeAccessRule(currentUser, PipeAccessRights.FullControl, AccessControlType.Allow));
        security.AddAccessRule(new PipeAccessRule(RestrictedCodeSid, PipeAccessRights.ReadWrite, AccessControlType.Allow));
        return NamedPipeServerStreamAcl.Create(
            pipeName,
            PipeDirection.InOut,
            1,
            PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous,
            0,
            0,
            security);
    }

    public static bool CurrentProcessIsWriteRestricted()
        => CurrentProcessHasRestrictedSid(RestrictedCodeSid);

    public static bool CurrentProcessHasWorldWriteRestriction()
        => CurrentProcessHasRestrictedSid(WorldSid);

    public static bool CurrentProcessHasNoDialogErrorMode()
        => (GetErrorMode() & RequiredChildErrorMode) == RequiredChildErrorMode;

    private static bool CurrentProcessHasRestrictedSid(SecurityIdentifier sid)
    {
        if (!OpenProcessToken(GetCurrentProcess(), TokenQuery, out nint token))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "OpenProcessToken failed.");
        try
        {
            GetTokenInformation(token, 11, 0, 0, out int required);
            nint buffer = Marshal.AllocHGlobal(required);
            try
            {
                if (!GetTokenInformation(token, 11, buffer, required, out _))
                    throw new Win32Exception(Marshal.GetLastWin32Error(), "GetTokenInformation(TokenRestrictedSids) failed.");
                int count = Marshal.ReadInt32(buffer);
                int offset = IntPtr.Size == 8 ? 8 : 4;
                int entrySize = Marshal.SizeOf<SidAndAttributes>();
                byte[] sidBytes = GetSidBytes(sid);
                nint expectedSid = Marshal.AllocHGlobal(sidBytes.Length);
                try
                {
                    Marshal.Copy(sidBytes, 0, expectedSid, sidBytes.Length);
                    for (int i = 0; i < count; i++)
                    {
                        var entry = Marshal.PtrToStructure<SidAndAttributes>(buffer + offset + i * entrySize);
                        if (EqualSid(entry.Sid, expectedSid)) return true;
                    }
                    return false;
                }
                finally { Marshal.FreeHGlobal(expectedSid); }
            }
            finally { Marshal.FreeHGlobal(buffer); }
        }
        finally { CloseHandle(token); }
    }

    private static byte[] GetSidBytes(SecurityIdentifier sid)
    {
        var bytes = new byte[sid.BinaryLength];
        sid.GetBinaryForm(bytes, 0);
        return bytes;
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

    public static int CurrentProcessMitigationStatus()
    {
        if (!GetProcessMitigationPolicy(GetCurrentProcess(), 0, out ulong dep, sizeof(ulong))
            || !GetProcessMitigationPolicy(GetCurrentProcess(), 1, out uint aslr, sizeof(uint))
            || !GetProcessMitigationPolicy(GetCurrentProcess(), 6, out uint extensionPoints, sizeof(uint)))
            return -Marshal.GetLastWin32Error();
        int status = 0;
        if ((dep & 0x1) != 0) status |= 1;
        if ((aslr & 0x1) != 0 && (aslr & 0x4) != 0) status |= 2;
        if ((extensionPoints & 0x1) != 0) status |= 4;
        return status;
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
    private struct StartupInfoEx
    {
        public StartupInfo StartupInfo;
        public nint AttributeList;
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

    [StructLayout(LayoutKind.Sequential)]
    private struct SidAndAttributes
    {
        public nint Sid;
        public uint Attributes;
    }

    [DllImport("kernel32.dll")]
    private static extern nint GetCurrentProcess();

    [DllImport("kernel32.dll", ExactSpelling = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern uint GetErrorMode();

    [DllImport("kernel32.dll", ExactSpelling = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern uint SetErrorMode(uint mode);

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

    [DllImport("advapi32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool EqualSid(nint firstSid, nint secondSid);

    [DllImport("advapi32.dll", EntryPoint = "CreateProcessAsUserW", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CreateProcessAsUser(
        nint token, string applicationName, StringBuilder commandLine,
        nint processAttributes, nint threadAttributes, [MarshalAs(UnmanagedType.Bool)] bool inheritHandles,
        uint creationFlags, nint environment, string? currentDirectory,
        ref StartupInfoEx startupInfo, out ProcessInformation processInformation);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool InitializeProcThreadAttributeList(
        nint attributeList, int attributeCount, uint flags, ref nuint size);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool UpdateProcThreadAttribute(
        nint attributeList, uint flags, nuint attribute, nint value, nuint size,
        nint previousValue, nint returnSize);

    [DllImport("kernel32.dll")]
    private static extern void DeleteProcThreadAttributeList(nint attributeList);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetProcessMitigationPolicy(
        nint process, int mitigationPolicy, out uint buffer, int length);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetProcessMitigationPolicy(
        nint process, int mitigationPolicy, out ulong buffer, int length);

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
