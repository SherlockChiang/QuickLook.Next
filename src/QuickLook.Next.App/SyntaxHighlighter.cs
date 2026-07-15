using System.Text;

namespace QuickLook.Next.App;

public enum TokenKind { Default, Keyword, Str, Comment, Number, Type, Property, Punctuation }

/// <summary>
/// Lightweight, language-aware tokenizer for preview coloring. It is deliberately conservative:
/// emitted token text concatenates back to the original input, so selection/copy remains exact.
/// </summary>
internal static class SyntaxHighlighter
{
    private sealed record Lang(string[] LineComments, string? BlockStart, string? BlockEnd, char[] Quotes);

    private static readonly Lang CLikeLang = new(["//"], "/*", "*/", ['"', '\'']);
    private static readonly Lang SqlLang = new(["--"], "/*", "*/", ['"', '\'']);
    private static readonly Lang BatchLang = new(["::", "REM ", "rem "], null, null, ['"']);
    private static readonly Lang PropertyLang = new([";", "#"], null, null, ['"', '\'']);
    private static readonly Lang HashCommentLang = new(["#"], null, null, ['"', '\'']);
    private static readonly Lang LuaLang = new(["--"], "--[[", "]]", ['"', '\'']);
    private static readonly Lang FSharpLang = new(["//"], "(*", "*)", ['"', '\'']);
    private static readonly Lang DefaultLang = new(["//", "#"], "/*", "*/", ['"', '\'']);

    private static readonly Dictionary<string, Lang> LanguageSpecs = new(StringComparer.Ordinal)
    {
        ["csharp"] = CLikeLang,
        ["rust"] = CLikeLang,
        ["javascript"] = CLikeLang,
        ["typescript"] = CLikeLang,
        ["java"] = CLikeLang,
        ["go"] = CLikeLang,
        ["c"] = CLikeLang,
        ["cpp"] = CLikeLang,
        ["php"] = CLikeLang,
        ["swift"] = CLikeLang,
        ["kotlin"] = CLikeLang,
        ["scala"] = CLikeLang,
        ["dart"] = CLikeLang,
        ["sql"] = SqlLang,
        ["batch"] = BatchLang,
        ["ini"] = PropertyLang,
        ["toml"] = PropertyLang,
        ["properties"] = PropertyLang,
        ["env"] = PropertyLang,
        ["python"] = HashCommentLang,
        ["shell"] = HashCommentLang,
        ["powershell"] = HashCommentLang,
        ["yaml"] = HashCommentLang,
        ["ruby"] = HashCommentLang,
        ["perl"] = HashCommentLang,
        ["makefile"] = HashCommentLang,
        ["dockerfile"] = HashCommentLang,
        ["lua"] = LuaLang,
        ["fsharp"] = FSharpLang,
    };

    private static readonly HashSet<string> Keywords = new(StringComparer.Ordinal)
    {
        "if","else","elif","for","foreach","while","do","switch","case","default","break","continue","return",
        "function","func","fn","def","class","struct","enum","interface","trait","impl","record","protocol",
        "namespace","using","import","from","export","module","package","use","mod","pub","open",
        "public","private","protected","internal","static","readonly","const","let","var","val","mut","final",
        "void","int","uint","long","short","byte","float","double","decimal","bool","boolean","char","string","str",
        "true","false","null","nil","none","None","True","False","undefined","NaN","new","this","self","super","base",
        "ref","out","in","async","await","yield","try","catch","except","finally","throw","throws","raise","defer",
        "as","is","typeof","instanceof","sizeof","where","with","match","when","lambda","guard","defer",
        "goto","then","fi","elif","esac","done","echo","exit","param","begin","end","select","unset","set",
        "extends","implements","virtual","override","abstract","sealed","partial","operator","delegate","event",
        "loop","crate","dyn","move","unsafe","extern","type","alias","macro","macro_rules","require","include",
        "SELECT","FROM","WHERE","JOIN","LEFT","RIGHT","INNER","OUTER","GROUP","ORDER","BY","INSERT","UPDATE",
        "DELETE","CREATE","ALTER","DROP","TABLE","VIEW","INDEX","VALUES","INTO","AND","OR","NOT","NULL",
    };

    private static readonly HashSet<string> TypeWords = new(StringComparer.Ordinal)
    {
        "String","Object","Array","Map","Set","List","Dictionary","Task","ValueTask","DateTime","Guid","Exception",
        "Int32","Int64","Boolean","Double","Float","Number","Promise","Console","Math","Regex","Path","File",
    };

    private static readonly HashSet<string> PropertyLanguages = new(StringComparer.Ordinal)
    {
        "json","yaml","toml","ini","env","properties","xml","html","xaml","css","scss","sass","less",
    };

    public static string NormalizeLanguage(string language)
    {
        string value = language.Trim().TrimStart('.').ToLowerInvariant();
        return value switch
        {
            "cs" => "csharp",
            "js" or "mjs" or "cjs" or "jsx" => "javascript",
            "ts" or "tsx" => "typescript",
            "ps1" => "powershell",
            "sh" or "bash" or "zsh" => "shell",
            "cmd" or "bat" => "batch",
            "yml" => "yaml",
            "htm" => "html",
            "xhtml" => "html",
            "csproj" or "props" or "targets" or "config" or "resx" or "xsd" => "xml",
            "cxx" or "cc" or "hpp" or "hxx" => "cpp",
            "kt" or "kts" => "kotlin",
            "rb" => "ruby",
            "pl" => "perl",
            "fs" or "fsx" => "fsharp",
            "dockerfile" => "dockerfile",
            "makefile" => "makefile",
            _ => value,
        };
    }

    public static List<(string Text, TokenKind Kind)> Highlight(string code, string language)
    {
        language = NormalizeLanguage(language);
        if (language is "xml" or "html" or "xaml") return HighlightMarkup(code);
        if (language is "json") return HighlightJson(code);
        if (language is "css" or "scss" or "sass" or "less") return HighlightCss(code);
        if (language is "csv" or "tsv") return HighlightDelimited(code, language == "tsv" ? '\t' : ',');

        Lang lang = SpecFor(language);
        var tokens = new List<(string, TokenKind)>();
        var pending = new StringBuilder();
        int i = 0, n = code.Length;

        void Flush() { if (pending.Length > 0) { tokens.Add((pending.ToString(), TokenKind.Default)); pending.Clear(); } }

        while (i < n)
        {
            if (lang.BlockStart is not null && Match(code, i, lang.BlockStart))
            {
                Flush();
                int end = code.IndexOf(lang.BlockEnd!, i + lang.BlockStart.Length, StringComparison.Ordinal);
                int stop = end < 0 ? n : end + lang.BlockEnd!.Length;
                tokens.Add((code[i..stop], TokenKind.Comment));
                i = stop;
                continue;
            }

            if (MatchesAny(code, i, lang.LineComments))
            {
                Flush();
                int eol = code.IndexOf('\n', i);
                int stop = eol < 0 ? n : eol;
                tokens.Add((code[i..stop], TokenKind.Comment));
                i = stop;
                continue;
            }

            char c = code[i];
            if (Array.IndexOf(lang.Quotes, c) >= 0)
            {
                Flush();
                int stop = ReadQuoted(code, i, c, allowNewLine: language is "yaml" or "toml");
                tokens.Add((code[i..stop], TokenKind.Str));
                i = stop;
                continue;
            }

            if (char.IsDigit(c) || (c is '-' or '+' && i + 1 < n && char.IsDigit(code[i + 1])))
            {
                Flush();
                int j = i + 1;
                while (j < n && (char.IsLetterOrDigit(code[j]) || code[j] is '.' or '_' or 'x' or 'X')) j++;
                tokens.Add((code[i..j], TokenKind.Number));
                i = j;
                continue;
            }

            if (IsWordStart(c))
            {
                int j = i + 1;
                while (j < n && IsWordPart(code[j])) j++;
                string word = code[i..j];
                TokenKind kind = ClassifyWord(code, i, j, word, language);
                if (kind == TokenKind.Default) pending.Append(word);
                else { Flush(); tokens.Add((word, kind)); }
                i = j;
                continue;
            }

            if ("{}[]();,.=:+-*/%!<>|&?".IndexOf(c) >= 0)
            {
                Flush();
                tokens.Add((code[i..(i + 1)], TokenKind.Punctuation));
                i++;
                continue;
            }

            pending.Append(c);
            i++;
        }

        Flush();
        return tokens;
    }

    private static List<(string, TokenKind)> HighlightJson(string code)
    {
        var tokens = new List<(string, TokenKind)>();
        int i = 0, n = code.Length;
        while (i < n)
        {
            char c = code[i];
            if (char.IsWhiteSpace(c))
            {
                int j = i + 1;
                while (j < n && char.IsWhiteSpace(code[j])) j++;
                tokens.Add((code[i..j], TokenKind.Default));
                i = j;
            }
            else if (c == '"')
            {
                int stop = ReadQuoted(code, i, '"', allowNewLine: false);
                TokenKind kind = NextNonWhite(code, stop) == ':' ? TokenKind.Property : TokenKind.Str;
                tokens.Add((code[i..stop], kind));
                i = stop;
            }
            else if (char.IsDigit(c) || c == '-')
            {
                int j = i + 1;
                while (j < n && (char.IsDigit(code[j]) || code[j] is '.' or 'e' or 'E' or '+' or '-')) j++;
                tokens.Add((code[i..j], TokenKind.Number));
                i = j;
            }
            else if (IsWordStart(c))
            {
                int j = i + 1;
                while (j < n && IsWordPart(code[j])) j++;
                tokens.Add((code[i..j], TokenKind.Keyword));
                i = j;
            }
            else
            {
                tokens.Add((code[i..(i + 1)], "{}[]:,".IndexOf(c) >= 0 ? TokenKind.Punctuation : TokenKind.Default));
                i++;
            }
        }
        return tokens;
    }

    private static List<(string, TokenKind)> HighlightMarkup(string code)
    {
        var tokens = new List<(string, TokenKind)>();
        var pending = new StringBuilder();
        int i = 0, n = code.Length;
        void Flush() { if (pending.Length > 0) { tokens.Add((pending.ToString(), TokenKind.Default)); pending.Clear(); } }

        while (i < n)
        {
            if (Match(code, i, "<!--"))
            {
                Flush();
                int end = code.IndexOf("-->", i + 4, StringComparison.Ordinal);
                int stop = end < 0 ? n : end + 3;
                tokens.Add((code[i..stop], TokenKind.Comment));
                i = stop;
                continue;
            }
            if (code[i] == '<')
            {
                Flush();
                tokens.Add(("<", TokenKind.Punctuation));
                int j = i + 1;
                if (j < n && code[j] == '/') { tokens.Add(("/", TokenKind.Punctuation)); j++; }
                int tag = j;
                while (j < n && IsMarkupNamePart(code[j])) j++;
                if (j > tag) tokens.Add((code[tag..j], TokenKind.Keyword));

                while (j < n && code[j] != '>')
                {
                    if (char.IsWhiteSpace(code[j]))
                    {
                        int w = j + 1;
                        while (w < n && char.IsWhiteSpace(code[w])) w++;
                        tokens.Add((code[j..w], TokenKind.Default));
                        j = w;
                    }
                    else if (code[j] is '"' or '\'')
                    {
                        char q = code[j];
                        int stop = ReadQuoted(code, j, q, allowNewLine: true);
                        tokens.Add((code[j..stop], TokenKind.Str));
                        j = stop;
                    }
                    else if (IsMarkupNamePart(code[j]))
                    {
                        int a = j;
                        while (j < n && IsMarkupNamePart(code[j])) j++;
                        tokens.Add((code[a..j], TokenKind.Property));
                    }
                    else
                    {
                        tokens.Add((code[j..(j + 1)], TokenKind.Punctuation));
                        j++;
                    }
                }
                if (j < n) { tokens.Add((">", TokenKind.Punctuation)); j++; }
                i = j;
                continue;
            }
            pending.Append(code[i]);
            i++;
        }
        Flush();
        return tokens;
    }

    private static List<(string, TokenKind)> HighlightCss(string code)
    {
        var tokens = new List<(string, TokenKind)>();
        var pending = new StringBuilder();
        int i = 0, n = code.Length;
        void Flush() { if (pending.Length > 0) { tokens.Add((pending.ToString(), TokenKind.Default)); pending.Clear(); } }

        while (i < n)
        {
            if (Match(code, i, "/*"))
            {
                Flush();
                int end = code.IndexOf("*/", i + 2, StringComparison.Ordinal);
                int stop = end < 0 ? n : end + 2;
                tokens.Add((code[i..stop], TokenKind.Comment));
                i = stop;
            }
            else if (code[i] is '"' or '\'')
            {
                Flush();
                int stop = ReadQuoted(code, i, code[i], allowNewLine: false);
                tokens.Add((code[i..stop], TokenKind.Str));
                i = stop;
            }
            else if (code[i] == '@')
            {
                Flush();
                int j = i + 1;
                while (j < n && IsWordPart(code[j])) j++;
                tokens.Add((code[i..j], TokenKind.Keyword));
                i = j;
            }
            else if (code[i] == '#')
            {
                Flush();
                int j = i + 1;
                while (j < n && Uri.IsHexDigit(code[j])) j++;
                tokens.Add((code[i..j], TokenKind.Number));
                i = j;
            }
            else if (IsCssNameStart(code[i]))
            {
                int j = i + 1;
                while (j < n && IsCssNamePart(code[j])) j++;
                TokenKind kind = NextNonWhite(code, j) == ':' ? TokenKind.Property : TokenKind.Default;
                if (kind == TokenKind.Default) pending.Append(code[i..j]);
                else { Flush(); tokens.Add((code[i..j], kind)); }
                i = j;
            }
            else if ("{}[]();,.=:+-*/%!<>|&?".IndexOf(code[i]) >= 0)
            {
                Flush();
                tokens.Add((code[i..(i + 1)], TokenKind.Punctuation));
                i++;
            }
            else
            {
                pending.Append(code[i]);
                i++;
            }
        }
        Flush();
        return tokens;
    }

    private static List<(string, TokenKind)> HighlightDelimited(string code, char delimiter)
    {
        var tokens = new List<(string, TokenKind)>();
        int i = 0, n = code.Length;
        while (i < n)
        {
            if (code[i] == '"')
            {
                int j = i + 1;
                while (j < n)
                {
                    if (code[j] == '"' && j + 1 < n && code[j + 1] == '"') { j += 2; continue; }
                    if (code[j] == '"') { j++; break; }
                    j++;
                }
                tokens.Add((code[i..j], TokenKind.Str));
                i = j;
            }
            else if (code[i] == delimiter)
            {
                tokens.Add((code[i..(i + 1)], TokenKind.Punctuation));
                i++;
            }
            else
            {
                int j = i + 1;
                while (j < n && code[j] != '"' && code[j] != delimiter) j++;
                tokens.Add((code[i..j], TokenKind.Default));
                i = j;
            }
        }
        return tokens;
    }

    private static TokenKind ClassifyWord(string code, int start, int end, string word, string language)
    {
        if (Keywords.Contains(word) || Keywords.Contains(word.ToUpperInvariant()))
            return TokenKind.Keyword;
        if (TypeWords.Contains(word) || char.IsUpper(word[0]) && language is not ("yaml" or "ini" or "env" or "properties"))
            return TokenKind.Type;
        if (PropertyLanguages.Contains(language) && NextNonWhite(code, end) is ':' or '=')
            return TokenKind.Property;
        if (start > 0 && code[start - 1] == '.')
            return TokenKind.Property;
        return TokenKind.Default;
    }

    private static int ReadQuoted(string code, int start, char quote, bool allowNewLine)
    {
        int j = start + 1;
        while (j < code.Length)
        {
            if (code[j] == '\\' && j + 1 < code.Length) { j += 2; continue; }
            if (code[j] == quote) return j + 1;
            if (!allowNewLine && code[j] == '\n') return j;
            j++;
        }
        return j;
    }

    private static char NextNonWhite(string code, int start)
    {
        int i = start;
        while (i < code.Length && char.IsWhiteSpace(code[i])) i++;
        return i < code.Length ? code[i] : '\0';
    }

    private static bool Match(string s, int i, string token)
        => i + token.Length <= s.Length && string.CompareOrdinal(s, i, token, 0, token.Length) == 0;

    private static bool MatchesAny(string s, int i, string[] tokens)
    {
        for (int tokenIndex = 0; tokenIndex < tokens.Length; tokenIndex++)
        {
            if (Match(s, i, tokens[tokenIndex]))
                return true;
        }

        return false;
    }

    private static bool IsWordStart(char c) => char.IsLetter(c) || c == '_' || c == '$';
    private static bool IsWordPart(char c) => char.IsLetterOrDigit(c) || c is '_' or '-' or '$';
    private static bool IsMarkupNamePart(char c) => char.IsLetterOrDigit(c) || c is ':' or '-' or '_' or '.';
    private static bool IsCssNameStart(char c) => char.IsLetter(c) || c is '_' or '-' or '.';
    private static bool IsCssNamePart(char c) => char.IsLetterOrDigit(c) || c is '_' or '-' or '.';

    private static Lang SpecFor(string language)
        => LanguageSpecs.TryGetValue(language, out var lang) ? lang : DefaultLang;
}
