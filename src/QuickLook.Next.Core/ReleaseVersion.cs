namespace QuickLook.Next.Core;

public sealed record ReleaseVersion(int Major, int Minor, int Patch, IReadOnlyList<string> PreRelease)
    : IComparable<ReleaseVersion>
{
    public static bool TryParse(string? value, out ReleaseVersion? version)
    {
        version = null;
        if (string.IsNullOrWhiteSpace(value) || value.Length > 128) return false;
        string withoutBuild = value.Split('+', 2)[0];
        string[] releaseParts = withoutBuild.Split('-', 2);
        string[] numbers = releaseParts[0].Split('.');
        if (numbers.Length != 3
            || !TryNumber(numbers[0], out int major)
            || !TryNumber(numbers[1], out int minor)
            || !TryNumber(numbers[2], out int patch)) return false;
        string[] preRelease = releaseParts.Length == 1 ? [] : releaseParts[1].Split('.');
        if (preRelease.Any(static part => part.Length == 0 || part.Length > 32
            || part.Any(static character => !char.IsAsciiLetterOrDigit(character) && character != '-'))) return false;
        version = new ReleaseVersion(major, minor, patch, preRelease);
        return true;

        static bool TryNumber(string text, out int number)
        {
            number = 0;
            return text.Length > 0
                && (text.Length == 1 || text[0] != '0')
                && int.TryParse(text, out number)
                && number >= 0;
        }
    }

    public int CompareTo(ReleaseVersion? other)
    {
        if (other is null) return 1;
        int numeric = Major.CompareTo(other.Major);
        if (numeric == 0) numeric = Minor.CompareTo(other.Minor);
        if (numeric == 0) numeric = Patch.CompareTo(other.Patch);
        if (numeric != 0) return numeric;
        if (PreRelease.Count == 0 || other.PreRelease.Count == 0)
            return PreRelease.Count == other.PreRelease.Count ? 0 : PreRelease.Count == 0 ? 1 : -1;
        for (int index = 0; index < Math.Min(PreRelease.Count, other.PreRelease.Count); index++)
        {
            string left = PreRelease[index];
            string right = other.PreRelease[index];
            bool leftNumeric = int.TryParse(left, out int leftNumber);
            bool rightNumeric = int.TryParse(right, out int rightNumber);
            int comparison = leftNumeric && rightNumeric
                ? leftNumber.CompareTo(rightNumber)
                : leftNumeric != rightNumeric
                    ? leftNumeric ? -1 : 1
                    : string.CompareOrdinal(left, right);
            if (comparison != 0) return comparison;
        }
        return PreRelease.Count.CompareTo(other.PreRelease.Count);
    }
}
