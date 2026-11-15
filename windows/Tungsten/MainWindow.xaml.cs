using Microsoft.UI.System;
using Microsoft.UI.Xaml;

namespace Tungsten;

public sealed partial class MainWindow : Window
{
    private readonly ThemeSettings themes;
    private bool closing;
    public MainWindow()
    {
        InitializeComponent();
        themes = ThemeSettings.CreateForWindowId(AppWindow.Id);
        AppWindow.TitleBar.PreferredTheme = Microsoft.UI.Windowing.TitleBarTheme.UseDefaultAppMode;
        Page.ActualThemeChanged += (_, _) => UpdateIcon();
        themes.Changed += ThemeChanged;
        Closed += (_, _) => themes.Changed -= ThemeChanged;
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
        var file = themes.HighContrast ? "AppIcon.ico" : Page.ActualTheme == ElementTheme.Dark ? "AppIconDark.ico" : "AppIconLight.ico";
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", file));
    }

    private void ThemeChanged(ThemeSettings sender, object args) => DispatcherQueue.TryEnqueue(UpdateIcon);
}
