param([switch]$Test, [switch]$Package, [string]$SourcesDir, [switch]$AllowDirty)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/env.ps1"
Push-Location $LauncherRoot
try {
    if ($Package) {
        if (-not $SourcesDir) { throw '-Package requires an explicit -SourcesDir' }
        $StageArgs = @("$PSScriptRoot/stage.py", '--sources-dir', $SourcesDir)
        if ($AllowDirty) { $StageArgs += '--allow-dirty' }
        python @StageArgs
        if ($LASTEXITCODE -ne 0) { throw 'Payload staging failed' }
        python "$PSScriptRoot/browser-payload.py"
        if ($LASTEXITCODE -ne 0) { throw 'Browser Agent compilation failed' }
    }
    if ($Test) {
        python -m unittest discover -s "$LauncherRoot/tests" -p 'test_*.py'
        if ($LASTEXITCODE -ne 0) { throw 'Packaging tests failed' }
        cargo test --locked --offline
        if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed' }
    }
    cargo build --release --bin hh-clicker-launcher --locked --offline
    if ($LASTEXITCODE -ne 0) { throw 'Rust release build failed' }
    if ($Package) {
        python "$PSScriptRoot/package-single.py"
        if ($LASTEXITCODE -ne 0) { throw 'Packaging failed' }
        $Version = python -c "import sys; sys.path.insert(0,'scripts'); from build_inputs import product_version; print(product_version())"
        if ($LASTEXITCODE -ne 0) { throw 'Could not read product version' }
        python "$PSScriptRoot/verify-single-exe.py" "$LauncherRoot/dist/HH-Clicker-Rust-portable-$Version-win-x64.exe"
        if ($LASTEXITCODE -ne 0) { throw 'Single EXE verification failed' }
    }
} finally { Pop-Location }
