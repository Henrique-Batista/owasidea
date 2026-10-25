[OwasideaWorld("tester")]
[OwasideaHostLog]
public static class TesterWorldExportsImpl
{
    [OwasideaExport]
    public static string Greet(string name)
    {
        TesterWorld.wit.Imports.poc.host.v0_1_0.IHostLogImports.Log($"greet called with {name}");
        return $"Hello, {name}! (from C# tester)";
    }

    [OwasideaExport]
    public static int Compute(int a, int b)
    {
        return a + b;
    }
}
