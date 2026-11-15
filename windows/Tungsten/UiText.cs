using System.Globalization;
using Microsoft.Windows.ApplicationModel.Resources;

namespace Tungsten;

public static class UiText
{
    private static readonly ResourceLoader Resources = new();
    public static string Get(string key, params object[] arguments)
    {
        var value = Resources.GetString(key);
        if (value.Length == 0) throw new InvalidOperationException($"Missing UI resource: {key}");
        return arguments.Length == 0 ? value : string.Format(CultureInfo.CurrentCulture, value, arguments);
    }
}
