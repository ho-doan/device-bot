using System.ComponentModel;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using Microsoft.Win32;

namespace win_app;

public partial class MainWindow : Window
{
    private readonly OrbitState _state = new();
    private bool _syncingPassword;
    private string _lastPath = "";

    public MainWindow()
    {
        InitializeComponent();
        DataContext = _state;
        _state.PropertyChanged += StateChanged;
        SystemEvents.UserPreferenceChanged += SystemPreferencesChanged;
        Closed += (_, _) =>
        {
            SystemEvents.UserPreferenceChanged -= SystemPreferencesChanged;
            _state.PropertyChanged -= StateChanged;
        };
        ApplySystemTheme();
        var arguments = Environment.GetCommandLineArgs();
        var routeIndex = Array.IndexOf(arguments, "--route");
        _state.Navigate(routeIndex >= 0 && routeIndex + 1 < arguments.Length
            ? arguments[routeIndex + 1] : "/");
    }

    private void StateChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (PasswordInput.Password != _state.Password)
        {
            _syncingPassword = true;
            try { PasswordInput.Password = _state.Password; }
            finally { _syncingPassword = false; }
        }
        RefreshNavigation();
        if (_lastPath == _state.Path) return;
        _lastPath = _state.Path;
        Dispatcher.BeginInvoke(new Action(() =>
        {
            WorkspaceScroll.ScrollToTop();
            RefreshLayout();
        }));
    }

    private void WindowLoaded(object sender, RoutedEventArgs e)
    {
        RefreshLayout();
        if (_state.IsAuth) EmailInput.Focus();
    }

    private void WindowSizeChanged(object sender, SizeChangedEventArgs e) => RefreshLayout();

    private void RefreshLayout()
    {
        if (!IsLoaded) return;
        var split = ActualWidth >= 960 && !SystemParameters.HighContrast;
        WelcomeColumn.Width = split ? new GridLength(1, GridUnitType.Star) : new GridLength(0);
        WelcomePanel.Visibility = split ? Visibility.Visible : Visibility.Collapsed;
        AuthContent.Margin = new Thickness(split ? 40 : 24);
        var rail = ActualWidth >= 800;
        RailColumn.Width = new GridLength(rail ? 112 : 0);
        NavigationRail.Visibility = rail ? Visibility.Visible : Visibility.Collapsed;
        BottomNavigation.Visibility = rail ? Visibility.Collapsed : Visibility.Visible;
        WorkspaceContent.Margin = new Thickness(rail ? 40 : 24);
        var contentWidth = Math.Min(1040, Math.Max(0, ActualWidth - (rail ? 112 : 0) - (rail ? 80 : 48)));
        StatsPanel.Columns = contentWidth < 640 ? 1 : 3;
    }

    private void RefreshNavigation()
    {
        foreach (var (button, selected) in new[]
        {
            (OverviewRail, _state.IsHome), (OverviewBottom, _state.IsHome),
            (AccountRail, _state.IsAccount), (AccountBottom, _state.IsAccount),
        })
        {
            button.Background = selected ? (Brush)FindResource("ContainerBrush") : Brushes.Transparent;
            button.Foreground = (Brush)FindResource(selected ? "OnContainerBrush" : "PrimaryBrush");
            AutomationProperties.SetItemStatus(button, selected ? "Current page" : "");
        }
    }

    private void PasswordChanged(object sender, RoutedEventArgs e)
    {
        if (!_syncingPassword) _state.Password = PasswordInput.Password;
    }

    private void TogglePasswordClicked(object sender, RoutedEventArgs e)
    {
        _state.Obscure = !_state.Obscure;
        Dispatcher.BeginInvoke(new Action(() =>
        {
            if (_state.Obscure) PasswordInput.Focus();
            else
            {
                VisiblePasswordInput.Focus();
                VisiblePasswordInput.CaretIndex = VisiblePasswordInput.Text.Length;
            }
        }));
    }

    private async void SubmitClicked(object sender, RoutedEventArgs e)
    {
        await _state.SubmitAsync();
        FocusSubmissionResult();
    }

    private async void TryDemoClicked(object sender, RoutedEventArgs e)
    {
        await _state.TryDemoAsync();
        FocusSubmissionResult();
    }

    private void FocusSubmissionResult()
    {
        Dispatcher.BeginInvoke(new Action(() =>
        {
            if (_state.HasMessage) ErrorSummary.Focus();
            else if (_state.ResetSent) ResetHeading.Focus();
            else if (_state.IsHome) HomeHeading.Focus();
            else if (_state.IsAccount) AccountHeading.Focus();
        }));
    }

    private async void EmailKeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key != Key.Enter || _state.Busy) return;
        e.Handled = true;
        if (_state.IsLogin)
        {
            if (_state.Obscure) PasswordInput.Focus();
            else VisiblePasswordInput.Focus();
        }
        else
        {
            await _state.SubmitAsync();
            FocusSubmissionResult();
        }
    }

    private async void PasswordKeyDown(object sender, KeyEventArgs e)
    {
        if (e.Key != Key.Enter || _state.Busy) return;
        e.Handled = true;
        await _state.SubmitAsync();
        FocusSubmissionResult();
    }

    private void ForgotPasswordClicked(object sender, RoutedEventArgs e)
    {
        _state.ForgotPassword();
        EmailInput.Focus();
    }

    private void BackToLoginClicked(object sender, RoutedEventArgs e)
    {
        _state.BackToLogin();
        EmailInput.Focus();
    }

    private void OverviewClicked(object sender, RoutedEventArgs e) => _state.Navigate("/home");
    private void AccountClicked(object sender, RoutedEventArgs e) => _state.Navigate("/account");
    private void SignOutClicked(object sender, RoutedEventArgs e)
    {
        _state.SignOut();
        EmailInput.Focus();
    }

    private void SystemPreferencesChanged(object sender, UserPreferenceChangedEventArgs e)
    {
        if (!Dispatcher.HasShutdownStarted)
            Dispatcher.BeginInvoke(new Action(ApplySystemTheme));
    }

    private void ApplySystemTheme()
    {
        var dark = false;
        try
        {
            dark = Registry.GetValue(@"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
                "AppsUseLightTheme", 1) is int value && value == 0;
        }
        catch (Exception error) when (error is System.Security.SecurityException
            or UnauthorizedAccessException or System.IO.IOException) { }

        var colors = new Dictionary<string, string>
        {
            ["PrimaryBrush"] = dark ? "#80DED0" : "#086B61",
            ["OnPrimaryBrush"] = dark ? "#003731" : "#FFFFFF",
            ["ContainerBrush"] = dark ? "#005048" : "#9FF2E5",
            ["OnContainerBrush"] = dark ? "#9FF2E5" : "#00201C",
            ["SurfaceBrush"] = dark ? "#101F21" : "#F8FAF8",
            ["InputBrush"] = dark ? "#0A1517" : "#FFFFFF",
            ["TextBrush"] = dark ? "#DEE4E2" : "#171D1C",
            ["MutedBrush"] = dark ? "#BEC9C6" : "#3F4947",
            ["OutlineBrush"] = dark ? "#3F4947" : "#BEC9C6",
            ["ErrorBrush"] = dark ? "#FFB4AB" : "#BA1A1A",
            ["ErrorBackgroundBrush"] = dark ? "#93000A" : "#FFDAD6",
            ["OnErrorBrush"] = dark ? "#FFDAD6" : "#410002",
        };
        foreach (var (key, color) in colors)
            Resources[key] = new SolidColorBrush((Color)ColorConverter.ConvertFromString(color));
        if (SystemParameters.HighContrast)
        {
            Resources["SurfaceBrush"] = Resources["InputBrush"] = Resources["ErrorBackgroundBrush"] = SystemColors.WindowBrush;
            Resources["TextBrush"] = Resources["MutedBrush"] = Resources["OutlineBrush"] = Resources["ErrorBrush"] = Resources["OnErrorBrush"] = SystemColors.WindowTextBrush;
            Resources["PrimaryBrush"] = Resources["ContainerBrush"] = SystemColors.HighlightBrush;
            Resources["OnPrimaryBrush"] = Resources["OnContainerBrush"] = SystemColors.HighlightTextBrush;
        }
        RefreshNavigation();
        RefreshLayout();
    }
}
