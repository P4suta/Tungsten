using Microsoft.UI.Xaml;
using Windows.UI.ViewManagement;

namespace Tungsten;

public sealed partial class MainWindow : Window
{
    private readonly AccessibilitySettings accessibility = new();
    private bool closing;
    public MainWindow()
    {
        InitializeComponent();
        AppWindow.TitleBar.PreferredTheme = Microsoft.UI.Windowing.TitleBarTheme.UseDefaultAppMode;
        Page.ActualThemeChanged += (_, _) => UpdateIcon();
        accessibility.HighContrastChanged += (_, _) => DispatcherQueue.TryEnqueue(UpdateIcon);
        UpdateIcon();
        AppWindow.Closing += async (_, args) =>
        {
            if (closing) return;
            args.Cancel = true;
            if (await Page.CanCloseAsync()) { closing = true; Close(); }
        };
        Activated += async (_, args) =>
        {
            if (args.WindowActivationState != WindowActivationState.Deactivated) await Page.CheckRemoteAsync();
        };
    }

    private void UpdateIcon()
    {
        var file = accessibility.HighContrast ? "AppIcon.ico" : Page.ActualTheme == ElementTheme.Dark ? "AppIconDark.ico" : "AppIconLight.ico";
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", file));
    }
}
