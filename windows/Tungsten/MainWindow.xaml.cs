using Microsoft.UI.Xaml;

namespace Tungsten;

public sealed partial class MainWindow : Window
{
    private bool closing;
    public MainWindow()
    {
        InitializeComponent();
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.ico"));
        AppWindow.TitleBar.PreferredTheme = Microsoft.UI.Windowing.TitleBarTheme.UseDefaultAppMode;
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
}
