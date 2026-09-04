using System.Reflection;

namespace QuickLook.Next.App;

internal static class ApplicationVersion
{
    public static string Current { get; } = Resolve();

    private static string Resolve()
        => typeof(ApplicationVersion).Assembly
            .GetCustomAttribute<AssemblyInformationalVersionAttribute>()?
            .InformationalVersion.Split('+')[0]
            ?? typeof(ApplicationVersion).Assembly.GetName().Version?.ToString(3)
            ?? "0.0.0";
}
