namespace QuickLook.Next.Core;

public readonly record struct TextLineRange(int Number, int Start, int Length);

public sealed class TextLineIndex
{
    private readonly TextLineRange[] _lines;

    private TextLineIndex(TextLineRange[] lines) => _lines = lines;

    public IReadOnlyList<TextLineRange> Lines => _lines;

    public static TextLineIndex Create(string text)
    {
        var lines = new List<TextLineRange>();
        int start = 0;
        int number = 1;
        for (int i = 0; i < text.Length; i++)
        {
            if (text[i] is not ('\r' or '\n'))
                continue;
            lines.Add(new TextLineRange(number++, start, i - start));
            if (text[i] == '\r' && i + 1 < text.Length && text[i + 1] == '\n')
                i++;
            start = i + 1;
        }
        lines.Add(new TextLineRange(number, start, text.Length - start));
        return new TextLineIndex(lines.ToArray());
    }

    public int FindLineIndex(int offset)
    {
        if (_lines.Length == 0)
            return 0;
        offset = Math.Clamp(offset, 0, _lines[^1].Start + _lines[^1].Length);
        int low = 0;
        int high = _lines.Length - 1;
        while (low <= high)
        {
            int mid = low + ((high - low) / 2);
            if (_lines[mid].Start <= offset)
                low = mid + 1;
            else
                high = mid - 1;
        }
        return Math.Max(0, high);
    }
}
