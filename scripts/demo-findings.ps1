# Writes a findings file over and over for a while.
#
#   pwsh -File scripts/demo-findings.ps1 -Pane p3 -Seconds 45
#
# Used to verify the app's half of the findings path without depending on an
# agent complying with the brief: the app clears a pane's findings when it opens,
# so a single write would race the clear. Writing once a second until the file
# is read means one of them lands in the window.

param(
    [Parameter(Mandatory = $true)][string]$Pane,
    [int]$Seconds = 45,
    [string]$Root = (Join-Path $env:APPDATA "app.hyperpane.desktop\findings")
)

New-Item -ItemType Directory -Force -Path $Root | Out-Null
$path = Join-Path $Root "$Pane.md"

$finding = @"
src/upload.ts:42 - the retry never fires when the response body is empty: the
guard checks the status code before it checks whether there is anything to
retry with, so a 200 with an empty body falls straight through.

src/http.ts:88 - the timeout is applied to the request but not to the body
read, so a server that stops mid-response holds the connection open forever.
"@

for ($i = 0; $i -lt $Seconds; $i++) {
    # No BOM: what the app reads is what a person wrote.
    [System.IO.File]::WriteAllText($path, $finding, [System.Text.UTF8Encoding]::new($false))
    Start-Sleep -Seconds 1
}

Write-Host "wrote $path for $Seconds seconds"
