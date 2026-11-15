namespace Tungsten.Core;

public static class StoragePaths
{
    public static bool IsUiTest(IEnumerable<string> arguments) => arguments.Skip(1).Contains("--ui-test", StringComparer.Ordinal);
    public static string For(string localFolder, bool uiTest) => uiTest ? Path.Combine(localFolder, "ui-tests") : localFolder;
}
