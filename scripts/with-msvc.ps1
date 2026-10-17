# Runs a command inside the Visual Studio developer environment.
#
#   pwsh -File scripts/with-msvc.ps1 -- cargo test --workspace
#
# The `--` separator is optional but recommended: without a param() block every
# argument is forwarded verbatim, which matters because cargo's own flags (for
# example -p) look like PowerShell parameter names.
#
# Rust's MSVC target needs link.exe on PATH. Visual Studio is usually present
# without that environment loaded, so this discovers the installation and
# imports the variables vcvars64.bat sets.

$ErrorActionPreference = "Stop"

$Command = @($args)
if ($Command.Count -gt 0 -and $Command[0] -eq "--") {
    $Command = @($Command | Select-Object -Skip 1)
}

if ($Command.Count -eq 0) {
    Write-Error "usage: with-msvc.ps1 [--] <command> [args...]"
    exit 2
}

$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $vswhere)) {
    Write-Error "vswhere.exe not found. Install Visual Studio Build Tools with the C++ workload."
    exit 3
}

$install = & $vswhere -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -property installationPath | Select-Object -First 1

if (-not $install) {
    Write-Error "No Visual Studio installation ships the MSVC C++ tools."
    exit 4
}

$vcvars = Join-Path $install "VC\Auxiliary\Build\vcvars64.bat"
if (-not (Test-Path $vcvars)) {
    Write-Error "vcvars64.bat not found under $install"
    exit 5
}

# `set` after calling vcvars prints the resulting environment; importing it is
# more reliable than reconstructing the paths by hand.
$dump = & cmd /c "`"$vcvars`" >nul 2>&1 && set"
foreach ($line in $dump) {
    if ($line -match '^([^=]+)=(.*)$') {
        [Environment]::SetEnvironmentVariable($matches[1], $matches[2], "Process")
    }
}

$program = $Command[0]
$rest = @()
if ($Command.Count -gt 1) {
    $rest = @($Command | Select-Object -Skip 1)
}

# Build tools write progress to stderr. Under "Stop" PowerShell would treat
# that as a terminating error, so relax it for the child only.
$ErrorActionPreference = "Continue"
& $program @rest
exit $LASTEXITCODE
