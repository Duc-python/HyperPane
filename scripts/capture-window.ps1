# Launches a built app, finds its window, and captures that window to a PNG.
# Used to review the UI without a human at the screen.
#
#   pwsh -File scripts/capture-window.ps1 -Exe target\debug\hyperpane.exe -Out shot.png
#
# The window is located by enumerating every top-level window of the process
# rather than by MainWindowHandle: a debug build still has the console
# subsystem, and MainWindowHandle happily returns the console instead of the
# app window.

param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Out,
    [string]$Title = "HyperPane",
    [int]$WaitSeconds = 25,
    # SendKeys sequences sent after the window is focused and before the
    # capture, so a shot can show more than the initial state. "^+e" is
    # Ctrl+Shift+E.
    [string[]]$Keys = @(),
    # How many times to send the whole key list. A soak run repeats the same
    # split/close cycle and asks whether the app is still answering afterwards.
    [int]$Repeat = 1,
    [int]$KeyDelayMs = 1500,
    # How long to let the app settle after the last keystroke before sampling.
    [int]$SettleMs = 700,
    # Capture the app's own stderr, which is where a panic lands.
    [string]$ErrorLog = "",
    [switch]$KeepRunning
)

$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class WinCap {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }

    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr hWnd, int attribute, out RECT value, int size);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetWindowPos(
        IntPtr hWnd, IntPtr insertAfter, int x, int y, int cx, int cy, uint flags);

    // Topmost does not require foreground rights, so it is the reliable way to
    // guarantee the window is not occluded when the screen is sampled.
    public static void SetTopmost(IntPtr hWnd, bool topmost) {
        const uint SWP_NOSIZE = 0x0001;
        const uint SWP_NOMOVE = 0x0002;
        const uint SWP_NOACTIVATE = 0x0010;
        const uint SWP_SHOWWINDOW = 0x0040;
        IntPtr after = new IntPtr(topmost ? -1 : -2);   // HWND_TOPMOST / NOTOPMOST
        SetWindowPos(hWnd, after, 0, 0, 0, 0,
            SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int cmd);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr hWnd);

    private delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);

    public static IntPtr[] WindowsOf(uint target) {
        var found = new List<IntPtr>();
        EnumWindows((h, _) => {
            uint pid;
            GetWindowThreadProcessId(h, out pid);
            if (pid == target && IsWindowVisible(h)) found.Add(h);
            return true;
        }, IntPtr.Zero);
        return found.ToArray();
    }

    public static string TitleOf(IntPtr hWnd) {
        var sb = new StringBuilder(512);
        GetWindowText(hWnd, sb, sb.Capacity);
        return sb.ToString();
    }

    [DllImport("user32.dll")] private static extern bool AttachThreadInput(uint attach, uint attachTo, bool join);
    [DllImport("kernel32.dll")] private static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] private static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);

    // Windows refuses SetForegroundWindow to a process that does not already
    // own the foreground. Two documented conditions let a process take it: it
    // received the last input event, and its thread is attached to the current
    // foreground thread's input queue. Do both.
    public static bool ForceForeground(IntPtr hWnd) {
        const byte VK_MENU = 0x12;
        const uint KEYEVENTF_KEYUP = 0x0002;

        keybd_event(VK_MENU, 0, 0, IntPtr.Zero);
        keybd_event(VK_MENU, 0, KEYEVENTF_KEYUP, IntPtr.Zero);

        IntPtr foreground = GetForegroundWindow();
        uint ignored;
        uint foregroundThread = GetWindowThreadProcessId(foreground, out ignored);
        uint ourThread = GetCurrentThreadId();

        bool joined = false;
        if (foregroundThread != 0 && foregroundThread != ourThread) {
            joined = AttachThreadInput(ourThread, foregroundThread, true);
        }

        bool raised = SetForegroundWindow(hWnd);

        if (joined) AttachThreadInput(ourThread, foregroundThread, false);
        return raised;
    }
}
"@

$exePath = (Resolve-Path $Exe).Path
Write-Host "launching $exePath"
$startArgs = @{ FilePath = $exePath; PassThru = $true }
if ($ErrorLog) {
    $logPath = [System.IO.Path]::GetFullPath($ErrorLog)
    $startArgs.RedirectStandardError = $logPath
    $startArgs.RedirectStandardOutput = "$logPath.out"
}
$process = Start-Process @startArgs

$handle = [IntPtr]::Zero
$deadline = (Get-Date).AddSeconds($WaitSeconds)
while ((Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 500
    $process.Refresh()
    if ($process.HasExited) {
        Write-Error "process exited early with code $($process.ExitCode)"
        exit 1
    }

    $windows = [WinCap]::WindowsOf([uint32]$process.Id)
    foreach ($candidate in $windows) {
        $name = [WinCap]::TitleOf($candidate)
        Write-Host "  window $candidate '$name'"
    }

    # Prefer the exact title, then anything that is not the console window.
    $exact = $windows | Where-Object { [WinCap]::TitleOf($_) -eq $Title } | Select-Object -First 1
    if ($exact) { $handle = $exact; break }

    $nonConsole = $windows |
        Where-Object { [WinCap]::TitleOf($_) -and [WinCap]::TitleOf($_) -notmatch '^(Console|Terminal)' } |
        Select-Object -First 1
    if ($nonConsole) { $handle = $nonConsole; break }
}

if ($handle -eq [IntPtr]::Zero) {
    if (-not $KeepRunning) { $process.Kill() }
    Write-Error "no usable window found within $WaitSeconds s"
    exit 2
}

Write-Host "using window $handle '$([WinCap]::TitleOf($handle))'"

# Let the webview paint before capturing.
Start-Sleep -Seconds 3

[void][WinCap]::ShowWindow($handle, 9)   # SW_RESTORE
[void][WinCap]::BringWindowToTop($handle)

# Taking the foreground is racy: the ALT tap only grants the right if no other
# process claims the last input event in between. Retry, and trust the actual
# foreground window rather than the return value, which can report false even
# when the switch happened.
$focused = $false
for ($attempt = 1; $attempt -le 10; $attempt++) {
    [void][WinCap]::ForceForeground($handle)
    Start-Sleep -Milliseconds 350
    if ([WinCap]::GetForegroundWindow() -eq $handle) { $focused = $true; break }
}
Write-Host "app in foreground: $focused (attempt $attempt)"
Start-Sleep -Milliseconds 900

$process.Refresh()
if ($process.HasExited) {
    Write-Host "the app exited during focus, with code $($process.ExitCode)"
    exit 1
}

if ($Keys.Count -gt 0) {
    Add-Type -AssemblyName System.Windows.Forms

    # SendKeys types into whatever holds focus, so confirm the app window is in
    # front before sending anything: a stray keystroke delivered to another
    # terminal is not a mistake worth making. Note that activating the process
    # by id is wrong here, because a debug build also owns a console window and
    # that is the one the shell would raise.
    if ([WinCap]::GetForegroundWindow() -ne $handle) {
        if (-not $KeepRunning) { $process.Kill() }
        Write-Error "the app window is not in the foreground; refusing to send keystrokes"
        exit 4
    }

    for ($cycle = 1; $cycle -le $Repeat; $cycle++) {
        foreach ($sequence in $Keys) {
            # Re-acquire the foreground before every sequence. A window can lose
            # it between keystrokes, and typing an ENTER into whatever took it
            # is not a mistake worth making.
            $held = $false
            for ($attempt = 1; $attempt -le 6; $attempt++) {
                if ([WinCap]::GetForegroundWindow() -eq $handle) { $held = $true; break }
                [void][WinCap]::ForceForeground($handle)
                Start-Sleep -Milliseconds 350
            }
            if (-not $held) {
                Write-Warning "the app is not in the foreground; stopping before '$sequence'"
                break
            }

            Write-Host "cycle $cycle/$Repeat  keys '$sequence'"
            [System.Windows.Forms.SendKeys]::SendWait($sequence)
            Start-Sleep -Milliseconds $KeyDelayMs

            $process.Refresh()
            if ($process.HasExited) {
                Write-Host "the app exited after '$sequence', with code $($process.ExitCode)"
                exit 1
            }
        }
    }

    Start-Sleep -Milliseconds $SettleMs

    $process.Refresh()
    if ($process.HasExited) {
        Write-Host "the app exited while settling, with code $($process.ExitCode)"
        exit 1
    }
    Write-Host ("the app is still alive after the keys: {0} handles, {1} threads" -f `
        $process.HandleCount, $process.Threads.Count)
}

# GetWindowRect includes the drop shadow on Windows 10 and later, so it would
# also capture a few pixels of whatever sits behind the window. The DWM frame
# bounds are the visible frame. Attribute 9 is DWMWA_EXTENDED_FRAME_BOUNDS and
# the struct is four ints.
$rect = New-Object WinCap+RECT
$dwm = [WinCap]::DwmGetWindowAttribute($handle, 9, [ref]$rect, 16)
if ($dwm -ne 0) {
    Write-Host "dwm frame bounds unavailable ($dwm); falling back to the window rect"
    [void][WinCap]::GetWindowRect($handle, [ref]$rect)
}
$width = $rect.Right - $rect.Left
$height = $rect.Bottom - $rect.Top
Write-Host "rect ${width}x${height} at $($rect.Left),$($rect.Top)"

if ($width -le 0 -or $height -le 0) {
    Write-Error "window has no size; it may be minimised"
    exit 3
}

# Raise above every non-topmost window for the sample. A window can lose the
# foreground at any moment, but being topmost holds until it is cleared.
[void][WinCap]::SetTopmost($handle, $true)
Start-Sleep -Milliseconds 700

$bitmap = New-Object System.Drawing.Bitmap($width, $height)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, (New-Object System.Drawing.Size($width, $height)))
$graphics.Dispose()

[void][WinCap]::SetTopmost($handle, $false)

# Resolve against PowerShell's current location, not the process working
# directory: GetFullPath would anchor a relative path to the shell host's own
# directory, which is not where the caller cd'd to.
$outPath = if ([System.IO.Path]::IsPathRooted($Out)) {
    $Out
} else {
    Join-Path (Get-Location).ProviderPath $Out
}
$parent = [System.IO.Path]::GetDirectoryName($outPath)
if ($parent -and -not (Test-Path $parent)) {
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
}
Write-Host "writing $outPath"
try {
    $bitmap.Save($outPath, [System.Drawing.Imaging.ImageFormat]::Png)
} catch {
    $bitmap.Dispose()
    Write-Error "could not write $outPath : $($_.Exception.Message)"
    exit 5
}
$bitmap.Dispose()
Write-Host "saved $outPath"

if (-not $KeepRunning) {
    $process.Refresh()
    if ($process.HasExited) {
        Write-Host "the app had ALREADY exited with code $($process.ExitCode) before the script closed it"
    } else {
        # Ask it to close, which is what a person clicking the X does. A hang
        # during shutdown shows up here rather than in a forced kill.
        $process.CloseMainWindow() | Out-Null
        Start-Sleep -Milliseconds 1200
        $process.Refresh()
        if (-not $process.HasExited) {
            $process.Kill()
            Start-Sleep -Milliseconds 400
            $process.Refresh()
            Write-Host "app did not close on request; force-killed"
        } else {
            Write-Host "app closed on request (exit code $($process.ExitCode))"
        }
    }
}