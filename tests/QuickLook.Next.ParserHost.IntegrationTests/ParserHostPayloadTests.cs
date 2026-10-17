using System.Text.Json;
using Xunit;

namespace QuickLook.Next.ParserHost.IntegrationTests;

public sealed class ParserHostPayloadTests
{
    [Fact]
    public void Staged_parser_host_does_not_require_WinRT_projection_runtime()
    {
        string hostDirectory = Path.Combine(AppContext.BaseDirectory, "ParserHost");
        Assert.True(File.Exists(Path.Combine(hostDirectory, "QuickLook.Next.ParserHost.exe")));
        Assert.False(File.Exists(Path.Combine(hostDirectory, "Microsoft.Windows.SDK.NET.dll")));
        Assert.False(File.Exists(Path.Combine(hostDirectory, "WinRT.Runtime.dll")));

        using JsonDocument dependencies = JsonDocument.Parse(File.ReadAllText(
            Path.Combine(hostDirectory, "QuickLook.Next.ParserHost.deps.json")));
        Assert.DoesNotContain(dependencies.RootElement.GetProperty("libraries").EnumerateObject(), library =>
            library.Name.StartsWith("Microsoft.Windows.SDK.NET", StringComparison.OrdinalIgnoreCase)
            || library.Name.StartsWith("Microsoft.Windows.CsWinRT", StringComparison.OrdinalIgnoreCase));
    }
}
