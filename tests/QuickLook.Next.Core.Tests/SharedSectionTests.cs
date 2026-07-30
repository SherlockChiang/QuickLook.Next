using System.Diagnostics;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class SharedSectionTests
{
    [Fact]
    public void Read_only_duplicate_survives_owner_close_and_rejects_writes()
    {
        if (!OperatingSystem.IsWindows())
            return;

        byte[] expected = "anonymous shared section"u8.ToArray();
        using Process process = Process.GetCurrentProcess();
        using var owner = SharedSectionOwner.Create(4096);
        using (SharedSectionView writable = owner.MapWritable())
            expected.CopyTo(writable.WritableBytes);

        long remoteHandle = owner.Handle.DangerousGetHandle().ToInt64();
        using SharedSectionView readOnly = SharedSectionView.DuplicateAndMapReadOnly(
            process.SafeHandle,
            remoteHandle,
            expected.Length);
        Assert.True(readOnly.Bytes.SequenceEqual(expected));
        Assert.Throws<InvalidOperationException>(() => readOnly.WritableBytes.Clear());

        owner.Dispose();
        Assert.True(readOnly.Bytes.SequenceEqual(expected));
        Assert.Throws<System.ComponentModel.Win32Exception>(() =>
        {
            using SharedSectionView _ = SharedSectionView.DuplicateAndMapReadOnly(
                process.SafeHandle,
                remoteHandle,
                expected.Length);
        });
    }

    [Fact]
    public void Read_only_duplicate_is_denied_a_writable_kernel_mapping()
    {
        if (!OperatingSystem.IsWindows())
            return;

        using Process process = Process.GetCurrentProcess();
        using var owner = SharedSectionOwner.Create(4096);
        using SafeSectionHandle readOnlySection = SharedSectionView.DuplicateReadOnlySection(
            process.SafeHandle,
            owner.Handle.DangerousGetHandle().ToInt64());

        using SafeSectionViewHandle attemptedWrite = NativeMethods.MapViewOfFile(
            readOnlySection,
            NativeMethods.FileMapWrite,
            0,
            0,
            4096);
        int error = System.Runtime.InteropServices.Marshal.GetLastWin32Error();

        Assert.True(attemptedWrite.IsInvalid);
        Assert.Equal(5, error); // ERROR_ACCESS_DENIED
    }

    [Fact]
    public void Shared_section_rejects_invalid_sizes_and_handles()
    {
        if (!OperatingSystem.IsWindows())
            return;

        using Process process = Process.GetCurrentProcess();
        Assert.Throws<ArgumentOutOfRangeException>(() => SharedSectionOwner.Create(0));
        Assert.Throws<ArgumentOutOfRangeException>(() =>
            SharedSectionView.DuplicateAndMapReadOnly(process.SafeHandle, 1, 0));
        Assert.Throws<System.ComponentModel.Win32Exception>(() =>
            SharedSectionView.DuplicateAndMapReadOnly(process.SafeHandle, 0, 1));

        using var owner = SharedSectionOwner.Create(64);
        Assert.Throws<System.ComponentModel.Win32Exception>(() =>
            SharedSectionView.DuplicateAndMapReadOnly(
                process.SafeHandle,
                owner.Handle.DangerousGetHandle().ToInt64(),
                64 * 1024));

        string path = Path.GetTempFileName();
        try
        {
            var file = WindowsHandleTransfer.OpenReadOnlyFile(path);
            using (file.Handle)
            {
                Assert.Throws<System.ComponentModel.Win32Exception>(() =>
                    SharedSectionView.DuplicateAndMapReadOnly(
                        process.SafeHandle,
                        file.Handle.DangerousGetHandle().ToInt64(),
                        1));
            }
        }
        finally
        {
            File.Delete(path);
        }
    }
}
