$ErrorActionPreference = 'Stop'
$LauncherRoot = Split-Path $PSScriptRoot -Parent
$env:CARGO_HOME = Join-Path $LauncherRoot '.tools/cargo-home'
$env:CARGO_TARGET_DIR = Join-Path $LauncherRoot 'target'
$ToolchainBin = Join-Path $LauncherRoot '.tools/rust/bin'
$VsCandidates = foreach ($Base in @(${env:ProgramFiles(x86)}, $env:ProgramFiles)) {
    $VsRoot = Join-Path $Base 'Microsoft Visual Studio'
    if (Test-Path -LiteralPath $VsRoot) {
        Get-ChildItem -LiteralPath $VsRoot -Filter link.exe -Recurse -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match '\\VC\\Tools\\MSVC\\[^\\]+\\bin\\Hostx64\\x64\\link.exe$' }
    }
}
$Link = $VsCandidates | Sort-Object FullName -Descending | Select-Object -First 1
if (-not $Link) { throw 'MSVC x64 Build Tools are required to build (not to run) the launcher' }
$VsTools = Split-Path (Split-Path (Split-Path (Split-Path $Link.FullName -Parent) -Parent) -Parent) -Parent
$WindowsSdk = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10'
$Sdk = Get-ChildItem -LiteralPath "$WindowsSdk/Lib" -Directory | Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'um/x64/kernel32.lib') } | Sort-Object Name -Descending | Select-Object -First 1
if (-not $Sdk) { throw 'Windows SDK x64 is required to build' }
$SdkVersion = $Sdk.Name
$env:PATH = "$ToolchainBin;$VsTools\bin\Hostx64\x64;$WindowsSdk\bin\$SdkVersion\x64;$env:PATH"
$env:LIB = "$VsTools\lib\x64;$WindowsSdk\Lib\$SdkVersion\ucrt\x64;$WindowsSdk\Lib\$SdkVersion\um\x64"
$env:INCLUDE = "$VsTools\include;$WindowsSdk\Include\$SdkVersion\ucrt;$WindowsSdk\Include\$SdkVersion\shared;$WindowsSdk\Include\$SdkVersion\um"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = "$VsTools\bin\Hostx64\x64\link.exe"
# Unit-separator encoding preserves the workspace path when it contains spaces.
# These flags take precedence over RUSTFLAGS and also cover dependency panic paths.
$RustBuildFlags = @('-C', 'target-feature=+crt-static', "--remap-path-prefix=$LauncherRoot=/hh-clicker/launcher-rust")
$env:CARGO_ENCODED_RUSTFLAGS = $RustBuildFlags -join [char]31
