using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Text.RegularExpressions;

namespace win_app;

// UI-independent demo behavior shared by the WPF screens. Nothing is persisted.
public sealed class OrbitState : INotifyPropertyChanged
{
    public const string DemoEmail = "alex@orbit.demo";
    public const string DemoPassword = "Orbit123!";
    private string _path = "";
    private string? _from;
    private string _email = "";
    private string _password = "";
    private bool _obscure = true;

    public event PropertyChangedEventHandler? PropertyChanged;
    public string Path => _path;
    public bool SignedIn { get; private set; }
    public bool Busy { get; private set; }
    public bool ResetSent { get; private set; }
    public string EmailError { get; private set; } = "";
    public string PasswordError { get; private set; } = "";
    public string Message { get; private set; } = "";
    public string Email { get => _email; set { _email = value; Notify(); } }
    public string Password { get => _password; set { _password = value; Notify(); } }
    public bool Obscure { get => _obscure; set { _obscure = value; Notify(); } }
    public bool IsLogin => Path == "/login";
    public bool IsReset => Path == "/forgot-password";
    public bool IsAuth => IsLogin || IsReset;
    public bool ShowForm => IsAuth && !ResetSent;
    public bool IsHome => Path == "/home" && SignedIn;
    public bool IsAccount => Path == "/account" && SignedIn;
    public bool IsWorkspace => IsHome || IsAccount;
    public bool IsNotFound => !IsAuth && !IsWorkspace;
    public bool PasswordHidden => Obscure;
    public bool PasswordVisible => !Obscure;
    public bool CanEdit => !Busy;
    public bool HasMessage => Message.Length > 0;
    public bool HasEmailError => EmailError.Length > 0;
    public bool HasPasswordError => PasswordError.Length > 0;
    public string AuthHeading => IsReset ? "Forgot your password?" : "Welcome back.";
    public string AuthDescription => IsReset
        ? "Enter your email address to try a password reset."
        : "Make room for your best work. Sign in to your workspace.";
    public string SubmitLabel => Busy ? "Please wait" : IsReset ? "Try password reset" : "Sign in";
    public string ToggleLabel => Obscure ? "Show password" : "Hide password";
    public string DemoCredentials => $"{DemoEmail}\n{DemoPassword}";
    public int CompletedCount => Tasks.Count(task => task.IsCompleted);
    public string CompletedSummary => $"{CompletedCount}/3";
    public string ProgressDescription => $"{CompletedCount} of 3 tasks complete";
    public ObservableCollection<OrbitTask> Tasks { get; } =
    [
        new("task-1-checkbox", "Make space for a fresh start", "5 min · A little planning", true),
        new("task-2-checkbox", "Choose your most important task", "10 min · Set your intention", false),
        new("task-3-checkbox", "Take a mindful break", "5 min · Recharge", false),
    ];

    public OrbitState()
    {
        foreach (var task in Tasks)
            task.PropertyChanged += (_, _) => Notify();
    }

    public static bool IsValidEmail(string value) =>
        Regex.IsMatch(value.Trim(), @"^[^\s@]+@[^\s@]+\.[^\s@]+$", RegexOptions.CultureInvariant);

    public static string Destination(string? from) => from == "/account" ? "/account" : "/home";

    public void Navigate(string location, string? from = null)
    {
        if (Busy) return;
        if (string.IsNullOrEmpty(location)) location = "/";
        var parts = location.Split('?', 2);
        var path = parts[0] == "/" ? "/home" : parts[0];
        if (parts.Length == 2)
        {
            foreach (var item in parts[1].Split('&'))
            {
                var pair = item.Split('=', 2);
                if (pair.Length == 2 && pair[0] == "from")
                    from = Uri.UnescapeDataString(pair[1].Replace('+', ' '));
            }
        }
        if ((path == "/home" || path == "/account") && !SignedIn)
        {
            from = parts[0] == "/" ? "/home" : location;
            path = "/login";
        }
        else if ((path == "/login" || path == "/forgot-password") && SignedIn)
        {
            path = Destination(from);
        }
        if (path == "/home" && _path != path)
        {
            for (var i = 0; i < Tasks.Count; i++) Tasks[i].IsCompleted = i == 0;
        }
        _path = path;
        _from = from;
        _email = _password = "";
        _obscure = true;
        EmailError = PasswordError = Message = "";
        ResetSent = false;
        Notify();
    }

    public void ForgotPassword() => Navigate("/forgot-password", _from);
    public void BackToLogin() => Navigate("/login", _from);

    public async Task TryDemoAsync()
    {
        if (Busy) return;
        Email = DemoEmail;
        Password = DemoPassword;
        await SubmitAsync();
    }

    public async Task SubmitAsync()
    {
        if (Busy || !ShowForm) return;
        EmailError = IsValidEmail(Email) ? "" : "Enter a valid email address.";
        PasswordError = IsLogin && Password.Length == 0 ? "Enter your password." : "";
        if (HasEmailError || HasPasswordError)
        {
            Message = "Check the highlighted fields to continue.";
            Notify();
            return;
        }
        Message = "";
        Busy = true;
        Notify();
        await Task.Delay(700);
        Busy = false;
        if (IsReset)
        {
            ResetSent = true;
            Notify();
            return;
        }
        if (!string.Equals(Email.Trim(), DemoEmail, StringComparison.OrdinalIgnoreCase)
            || Password != DemoPassword)
        {
            Message = "Email or password is incorrect. Try the demo account below.";
            Notify();
            return;
        }
        SignedIn = true;
        Navigate(Destination(_from));
    }

    public void SignOut()
    {
        if (!IsWorkspace) return;
        SignedIn = false;
        Navigate(Path);
    }

    private void Notify() => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(string.Empty));
}

public sealed class OrbitTask(string id, string title, string description, bool completed)
    : INotifyPropertyChanged
{
    private bool _isCompleted = completed;
    public string Id { get; } = id;
    public string Title { get; } = title;
    public string Description { get; } = description;
    public event PropertyChangedEventHandler? PropertyChanged;
    public bool IsCompleted
    {
        get => _isCompleted;
        set
        {
            if (_isCompleted == value) return;
            _isCompleted = value;
            PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(IsCompleted)));
        }
    }
}
