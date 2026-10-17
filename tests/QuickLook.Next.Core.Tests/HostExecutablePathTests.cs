using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class HostExecutablePathTests
{
    [Theory]
    [InlineData(PreviewHostKind.Raster, "RasterHost")]
    [InlineData(PreviewHostKind.Parser, "ParserHost")]
    [InlineData(PreviewHostKind.ShellBroker, "ShellBroker")]
    public void Deployed_subfolder_wins_over_flat_and_development_outputs(PreviewHostKind kind, string folder)
    {
        using var layout = new Layout(folder);
        layout.Add(layout.Deployed);
        layout.Add(layout.Flat);
        layout.Add(layout.Development);
        Assert.Equal(layout.Deployed, HostExecutablePath.Resolve(kind, layout.AppDirectory));
    }

    [Theory]
    [InlineData(PreviewHostKind.Raster, "RasterHost")]
    [InlineData(PreviewHostKind.Parser, "ParserHost")]
    [InlineData(PreviewHostKind.ShellBroker, "ShellBroker")]
    public void Flat_deployment_wins_over_development_output(PreviewHostKind kind, string folder)
    {
        using var layout = new Layout(folder);
        layout.Add(layout.Flat);
        layout.Add(layout.Development);
        Assert.Equal(layout.Flat, HostExecutablePath.Resolve(kind, layout.AppDirectory));
    }

    [Theory]
    [InlineData(PreviewHostKind.Raster, "RasterHost")]
    [InlineData(PreviewHostKind.Parser, "ParserHost")]
    [InlineData(PreviewHostKind.ShellBroker, "ShellBroker")]
    public void Sibling_development_executable_is_only_used_in_debug(PreviewHostKind kind, string folder)
    {
        using var layout = new Layout(folder);
        layout.Add(layout.Development);
#if DEBUG
        Assert.Equal(layout.Development, HostExecutablePath.Resolve(kind, layout.AppDirectory));
#else
        Assert.Equal(layout.Deployed, HostExecutablePath.Resolve(kind, layout.AppDirectory));
#endif
    }

    [Theory]
    [InlineData(PreviewHostKind.Raster, "RasterHost")]
    [InlineData(PreviewHostKind.Parser, "ParserHost")]
    [InlineData(PreviewHostKind.ShellBroker, "ShellBroker")]
    public void Missing_host_stays_at_expected_deployment_path(PreviewHostKind kind, string folder)
    {
        using var layout = new Layout(folder);
        Assert.Equal(layout.Deployed, HostExecutablePath.Resolve(kind, layout.AppDirectory));
    }

    [Fact]
    public void Relative_base_directory_is_rejected()
        => Assert.Throws<ArgumentException>(() => HostExecutablePath.Resolve(PreviewHostKind.Raster, "relative"));

    [Fact]
    public void Unknown_host_is_rejected()
        => Assert.Throws<ArgumentOutOfRangeException>(() =>
            HostExecutablePath.Resolve((PreviewHostKind)999, Path.GetTempPath()));

    private sealed class Layout : IDisposable
    {
        private readonly string _root = Path.Combine(Path.GetTempPath(), $"quicklook-host-path-{Guid.NewGuid():N}");

        public Layout(string folder)
        {
            string source = Path.Combine(_root, "src");
            AppDirectory = Path.Combine(source, "QuickLook.Next.App", "bin", "Release", "net10.0-windows10.0.19041.0", "win-x64");
            string executable = $"QuickLook.Next.{folder}.exe";
            Deployed = Path.Combine(AppDirectory, folder, executable);
            Flat = Path.Combine(AppDirectory, executable);
            string framework = folder == "ParserHost" ? "net10.0-windows" : "net10.0-windows10.0.19041.0";
            Development = Path.Combine(source, $"QuickLook.Next.{folder}", "bin", "Debug", framework, "win-x64", executable);
            Directory.CreateDirectory(AppDirectory);
        }

        public string AppDirectory { get; }
        public string Deployed { get; }
        public string Flat { get; }
        public string Development { get; }

        public void Add(string path)
        {
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            File.WriteAllText(path, "host path fixture");
        }

        public void Dispose() => Directory.Delete(_root, recursive: true);
    }
}
