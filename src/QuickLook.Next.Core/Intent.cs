namespace QuickLook.Next.Core;

/// <summary>High-level intents the Rust native layer emits over its FFI callback (never raw key events).</summary>
public enum PreviewIntent
{
    Open,       // space pressed on a valid selection
    Toggle,     // space toggles the open preview
    Switch,     // arrow keys: preview the newly selected item
    Close,      // escape
    Reload,     // F5
    Fullscreen, // F11
    ZoomIn,     // + / numpad+
    ZoomOut,    // - / numpad-
}

/// <summary>A decoded native intent. <see cref="Paths"/> carries the current shell selection for Open/Switch.</summary>
public sealed record NativeIntent(PreviewIntent Intent, IReadOnlyList<string> Paths)
{
    public string? PrimaryPath => Paths.Count > 0 ? Paths[0] : null;

    /// <summary>
    /// Parse the tab-delimited line the native callback emits, e.g. "OPEN\tC:\a.png\tC:\b.png".
    /// (Spike 3 used this wire shape; see spikes/spike3-native/SPIKE3_FINDINGS.md.)
    /// </summary>
    public static NativeIntent? TryParse(string line)
    {
        if (string.IsNullOrEmpty(line)) return null;
        string[] parts = line.Split('\t');
        PreviewIntent? intent = parts[0] switch
        {
            "OPEN" => PreviewIntent.Open,
            "TOGGLE" => PreviewIntent.Toggle,
            "SWITCH" => PreviewIntent.Switch,
            "CLOSE" => PreviewIntent.Close,
            "RELOAD" => PreviewIntent.Reload,
            "FULLSCREEN" => PreviewIntent.Fullscreen,
            "ZOOM_IN" => PreviewIntent.ZoomIn,
            "ZOOM_OUT" => PreviewIntent.ZoomOut,
            _ => null,
        };
        if (intent is null) return null;
        var paths = parts.Skip(1).Where(p => p.Length > 0 && !p.StartsWith('<')).ToArray();
        return new NativeIntent(intent.Value, paths);
    }
}
