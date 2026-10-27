namespace LibraryWorld.wit.Exports.poc.{{name}}.v0_1_0;

public class ApiExportsImpl : IApiExports
{
    public static string Greet(string name)
    {
        return $"Hello, {name}! (from C#)";
    }
}
