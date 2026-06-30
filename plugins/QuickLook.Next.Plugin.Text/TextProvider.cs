using System.Text;
using QuickLook.Next.Contracts;

namespace QuickLook.Next.Plugin.Text;

public sealed class TextProvider : IPreviewProvider
{
    private const int MaxPreviewBytes = 512 * 1024;

    private static readonly Dictionary<string, (string Format, string Language)> KnownExtensions = new(StringComparer.OrdinalIgnoreCase)
    {
        [".md"] = ("markdown", "markdown"),
        [".markdown"] = ("markdown", "markdown"),
        [".txt"] = ("plain", "text"),
        [".log"] = ("plain", "log"),
        [".csv"] = ("plain", "csv"),
        [".tsv"] = ("plain", "tsv"),
        [".env"] = ("code", "env"),
        [".bat"] = ("code", "batch"),
        [".cmd"] = ("code", "batch"),
        [".ps1"] = ("code", "powershell"),
        [".sh"] = ("code", "shell"),
        [".bash"] = ("code", "shell"),
        [".zsh"] = ("code", "shell"),
        [".json"] = ("code", "json"),
        [".xml"] = ("code", "xml"),
        [".xaml"] = ("code", "xaml"),
        [".xsd"] = ("code", "xml"),
        [".resx"] = ("code", "xml"),
        [".config"] = ("code", "xml"),
        [".ini"] = ("code", "ini"),
        [".cfg"] = ("code", "ini"),
        [".conf"] = ("code", "ini"),
        [".properties"] = ("code", "properties"),
        [".yml"] = ("code", "yaml"),
        [".yaml"] = ("code", "yaml"),
        [".toml"] = ("code", "toml"),
        [".cs"] = ("code", "csharp"),
        [".csproj"] = ("code", "xml"),
        [".sln"] = ("plain", "text"),
        [".props"] = ("code", "xml"),
        [".targets"] = ("code", "xml"),
        [".rs"] = ("code", "rust"),
        [".js"] = ("code", "javascript"),
        [".jsx"] = ("code", "javascript"),
        [".mjs"] = ("code", "javascript"),
        [".cjs"] = ("code", "javascript"),
        [".ts"] = ("code", "typescript"),
        [".tsx"] = ("code", "typescript"),
        [".css"] = ("code", "css"),
        [".scss"] = ("code", "scss"),
        [".sass"] = ("code", "sass"),
        [".less"] = ("code", "less"),
        [".html"] = ("code", "html"),
        [".htm"] = ("code", "html"),
        [".py"] = ("code", "python"),
        [".c"] = ("code", "c"),
        [".h"] = ("code", "c"),
        [".cc"] = ("code", "cpp"),
        [".cpp"] = ("code", "cpp"),
        [".cxx"] = ("code", "cpp"),
        [".hpp"] = ("code", "cpp"),
        [".hxx"] = ("code", "cpp"),
        [".java"] = ("code", "java"),
        [".go"] = ("code", "go"),
        [".php"] = ("code", "php"),
        [".rb"] = ("code", "ruby"),
        [".pl"] = ("code", "perl"),
        [".swift"] = ("code", "swift"),
        [".kt"] = ("code", "kotlin"),
        [".kts"] = ("code", "kotlin"),
        [".sql"] = ("code", "sql"),
        [".lua"] = ("code", "lua"),
        [".fs"] = ("code", "fsharp"),
        [".fsx"] = ("code", "fsharp"),
        [".vb"] = ("code", "vb"),
        [".dart"] = ("code", "dart"),
        [".scala"] = ("code", "scala"),
        [".r"] = ("code", "r"),
        [".dockerfile"] = ("code", "dockerfile"),
    };

    private static readonly Dictionary<string, (string Format, string Language)> KnownFileNames = new(StringComparer.OrdinalIgnoreCase)
    {
        ["Dockerfile"] = ("code", "dockerfile"),
        ["Containerfile"] = ("code", "dockerfile"),
        ["Makefile"] = ("code", "makefile"),
        ["CMakeLists.txt"] = ("code", "cmake"),
        [".editorconfig"] = ("code", "ini"),
        [".gitignore"] = ("plain", "text"),
        [".gitattributes"] = ("plain", "text"),
        [".dockerignore"] = ("plain", "text"),
        [".env"] = ("code", "env"),
    };

    public bool CanHandle(FileProbe probe)
    {
        if (KnownExtensions.ContainsKey(probe.Extension)) return true;
        if (KnownFileNames.ContainsKey(Path.GetFileName(probe.Path))) return true;
        return probe.MagicPrefix.Length > 0 && !probe.MagicPrefix.Contains((byte)0);
    }

    public async Task<PreviewResult> OpenAsync(string path, FileProbe probe, IPreviewContext context)
    {
        context.ReportStatus("TextProvider: reading text...");
        var info = GetFormatInfo(path);
        byte[] bytes;
        await using (var fs = File.OpenRead(path))
        {
            int length = (int)Math.Min(fs.Length, MaxPreviewBytes + 1);
            bytes = new byte[length];
            int read = await fs.ReadAsync(bytes);
            if (read < bytes.Length) Array.Resize(ref bytes, read);
        }

        bool truncated = bytes.Length > MaxPreviewBytes;
        if (truncated) Array.Resize(ref bytes, MaxPreviewBytes);

        string text = Decode(bytes);
        if (truncated)
            text += Environment.NewLine + Environment.NewLine + $"[Preview truncated at {MaxPreviewBytes:N0} bytes]";

        return new PreviewResult(info.Format == "markdown" ? "markdown" : "text", Path.GetFileName(path))
        {
            PreferredWidth = 920,
            PreferredHeight = 720,
            Text = text,
            TextFormat = info.Format,
            TextLanguage = info.Language,
        };
    }

    private static (string Format, string Language) GetFormatInfo(string path)
    {
        if (KnownFileNames.TryGetValue(Path.GetFileName(path), out var byName))
            return byName;
        return KnownExtensions.GetValueOrDefault(Path.GetExtension(path), (Format: "plain", Language: "text"));
    }

    private static string Decode(byte[] bytes)
    {
        if (bytes.Length >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF)
            return Encoding.UTF8.GetString(bytes, 3, bytes.Length - 3);
        if (bytes.Length >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE)
            return Encoding.Unicode.GetString(bytes, 2, bytes.Length - 2);
        if (bytes.Length >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF)
            return Encoding.BigEndianUnicode.GetString(bytes, 2, bytes.Length - 2);
        return Encoding.UTF8.GetString(bytes);
    }
}
