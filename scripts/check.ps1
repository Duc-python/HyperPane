# Runs every check the repository promises, and fails on the first one that
# does not pass.
#
#   pwsh -File scripts/check.ps1
#
# Exists because a commit once went out claiming clippy was clean while clippy
# was reporting an error: the commands were chained by hand and nobody read the
# exit code. Now one command answers the question, and it exits non-zero when
# the answer is no.

$ErrorActionPreference = "Continue"
$env:Path = (Join-Path $env:USERPROFILE ".cargo\bin") + ";" + $env:Path
Push-Location (Join-Path $PSScriptRoot "..")

$failures = @()

function Invoke-Check {
    param([string]$Name, [scriptblock]$Body)

    Write-Host ""
    Write-Host "== $Name" -ForegroundColor Cyan
    & $Body
    if ($LASTEXITCODE -ne 0) {
        $script:failures += $Name
        Write-Host "   FAILED ($Name) exit $LASTEXITCODE" -ForegroundColor Red
    } else {
        Write-Host "   ok" -ForegroundColor Green
    }
}

Invoke-Check "rust format" {
    & (Join-Path $PSScriptRoot "with-msvc.ps1") -- cargo fmt --all --check 2>&1 |
        Select-Object -Last 3
}

Invoke-Check "rust clippy" {
    & (Join-Path $PSScriptRoot "with-msvc.ps1") -- cargo clippy --workspace --all-targets -- -D warnings 2>&1 |
        Select-String -Pattern "^warning:|^error:|^\s+-->|Finished" | Select-Object -First 20
}

Invoke-Check "rust tests" {
    & (Join-Path $PSScriptRoot "with-msvc.ps1") -- cargo test --workspace 2>&1 |
        Select-String -Pattern "test result:|FAILED|^error" | Select-Object -Last 12
}

Invoke-Check "web typecheck" {
    pnpm --filter @hyperpane/desktop typecheck 2>&1 | Select-Object -Last 5
}

Invoke-Check "web build" {
    pnpm --filter @hyperpane/desktop build 2>&1 | Select-Object -Last 5
}

Pop-Location

Write-Host ""
if ($failures.Count -gt 0) {
    Write-Host "FAILED: $($failures -join ', ')" -ForegroundColor Red
    exit 1
}
Write-Host "all checks passed" -ForegroundColor Green
exit 0
