namespace PolicyWorld.wit.Exports.shipyard.policy.v0_1_0;

public static class ApiExportsImpl
{
    public static string Classify(int score)
    {
        PolicyWorld.wit.Imports.poc.host.v0_1_0.IHostLogImports.Log($"policy classify({score})");
        if (score >= 85) return "ship";
        if (score >= 65) return "review";
        if (score >= 40) return "hold";
        return "blocked";
    }

    public static string NextVersion(string current, string bump)
    {
        var parts = current.TrimStart('v').Split('.');
        var major = int.Parse(parts[0]);
        var minor = parts.Length > 1 ? int.Parse(parts[1]) : 0;
        var patch = parts.Length > 2 ? int.Parse(parts[2]) : 0;
        var next = bump switch
        {
            "major" => $"v{major + 1}.0.0",
            "minor" => $"v{major}.{minor + 1}.0",
            _ => $"v{major}.{minor}.{patch + 1}",
        };
        PolicyWorld.wit.Imports.poc.host.v0_1_0.IHostLogImports.Log($"policy next-version({current}, {bump}) -> {next}");
        return next;
    }

    public static string Summarize(string version, int score)
    {
        var level = Classify(score);
        return $"{version}: score {score}, policy decision {level}";
    }
}
