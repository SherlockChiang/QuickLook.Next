// Build-time inspection only. Never load or execute the inspected DLL.
// PE export directory layout: https://learn.microsoft.com/en-us/windows/win32/debug/pe-format#export-directory-table
using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
using System.Text;

public static class NativePeExports
{
    private const int MaxFileBytes = 64 * 1024 * 1024;
    private const int MaxExports = 4096;
    private const int MaxNameBytes = 256;

    private static BlobReader ReadRange(PEReader pe, uint rva, int bytes)
    {
        if (rva == 0 || rva > int.MaxValue || bytes <= 0)
            throw new InvalidDataException("Invalid export RVA/range.");
        // Bound to actual file-backed section bytes, not a zero-filled virtual tail.
        foreach (SectionHeader section in pe.PEHeaders.SectionHeaders)
        {
            long offset = (long)rva - section.VirtualAddress;
            if (offset >= 0 && offset + bytes <= section.SizeOfRawData)
                return pe.GetSectionData((int)rva).GetReader(0, bytes);
        }
        throw new InvalidDataException("Export range is outside file-backed sections.");
    }

    public static string[] Read(Stream stream)
    {
        if (!stream.CanSeek || stream.Length < 512 || stream.Length > MaxFileBytes)
            throw new InvalidDataException("Native DLL size/stream is outside inspection bounds.");
        using (var pe = new PEReader(stream, PEStreamOptions.LeaveOpen))
        {
            PEHeader header = pe.PEHeaders.PEHeader;
            if (header == null || header.Magic != PEMagic.PE32Plus ||
                pe.PEHeaders.CoffHeader.Machine != Machine.Amd64 ||
                (pe.PEHeaders.CoffHeader.Characteristics & Characteristics.Dll) == 0 ||
                pe.HasMetadata)
                throw new InvalidDataException("Expected a native win-x64 PE32+ DLL.");
            DirectoryEntry directory = header.ExportTableDirectory;
            if (directory.Size < 40 || directory.RelativeVirtualAddress <= 0)
                throw new InvalidDataException("Missing export directory.");
            var table = ReadRange(pe, (uint)directory.RelativeVirtualAddress, directory.Size);
            table.Offset = 20;
            uint functionCount = table.ReadUInt32();
            uint nameCount = table.ReadUInt32();
            uint functionRva = table.ReadUInt32();
            uint namesRva = table.ReadUInt32();
            uint ordinalsRva = table.ReadUInt32();
            if (functionCount == 0 || functionCount > MaxExports || nameCount != functionCount)
                throw new InvalidDataException("Export count is unbounded or includes unnamed/aliased entries.");
            var functions = ReadRange(pe, functionRva, checked((int)functionCount * 4));
            var names = ReadRange(pe, namesRva, checked((int)nameCount * 4));
            var ordinals = ReadRange(pe, ordinalsRva, checked((int)nameCount * 2));
            var seenNames = new HashSet<string>(StringComparer.Ordinal);
            var seenOrdinals = new HashSet<ushort>();
            var result = new List<string>();
            long directoryEnd = (long)directory.RelativeVirtualAddress + directory.Size;
            for (int i = 0; i < nameCount; i++)
            {
                uint nameRva = names.ReadUInt32();
                ushort ordinal = ordinals.ReadUInt16();
                if (ordinal >= functionCount || !seenOrdinals.Add(ordinal))
                    throw new InvalidDataException("Invalid or aliased export ordinal.");
                functions.Offset = ordinal * 4;
                uint bodyRva = functions.ReadUInt32();
                if (bodyRva >= directory.RelativeVirtualAddress && bodyRva < directoryEnd)
                    throw new InvalidDataException("Forwarded exports are not part of the native ABI.");
                ReadRange(pe, bodyRva, 1);
                bool executable = false;
                foreach (SectionHeader section in pe.PEHeaders.SectionHeaders)
                {
                    long offset = (long)bodyRva - section.VirtualAddress;
                    if (offset >= 0 && offset < section.SizeOfRawData &&
                        (section.SectionCharacteristics & SectionCharacteristics.MemExecute) != 0)
                        executable = true;
                }
                if (!executable)
                    throw new InvalidDataException("Export does not point to executable section bytes.");
                var nameBytes = new List<byte>();
                bool terminated = false;
                for (int j = 0; j < MaxNameBytes; j++)
                {
                    byte value = ReadRange(pe, checked(nameRva + (uint)j), 1).ReadByte();
                    if (value == 0) { terminated = true; break; }
                    if (value > 127)
                        throw new InvalidDataException("Export names must be ASCII.");
                    nameBytes.Add(value);
                }
                if (!terminated || nameBytes.Count == 0)
                    throw new InvalidDataException("Empty or unterminated export name.");
                string name = Encoding.ASCII.GetString(nameBytes.ToArray());
                if (!seenNames.Add(name))
                    throw new InvalidDataException("Duplicate export name.");
                result.Add(name);
            }
            result.Sort(StringComparer.Ordinal);
            return result.ToArray();
        }
    }
}
