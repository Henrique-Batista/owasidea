namespace LibraryWorld.wit.Exports.poc.{{name_ident}}.v0_1_0;

[OwasideaWorld("library")]
public class ApiExportsImpl : IApiExports
{
    [OwasideaExport]
    public static string Greet(string name)
    {
        return $"Hello, {name}! (from C#)";
    }
}
