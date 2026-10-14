param()
$ErrorActionPreference = 'Stop'
$repository = Split-Path $PSScriptRoot -Parent
Push-Location $repository
try {
    $metadata = & cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read package version.' }
    $Version = ($metadata.packages | Where-Object { $_.name -eq 'research-dashboard' }).version
    if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw 'MSI requires a three-part numeric release version.' }
    & cargo build --release --bins
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed.' }
    $wixTool = Join-Path $repository '.tools\wix.exe'
    if (-not (Test-Path -LiteralPath $wixTool)) {
        & dotnet tool install wix --tool-path (Join-Path $repository '.tools') --version 6.0.2
        if ($LASTEXITCODE -ne 0) { throw 'WiX setup failed.' }
    }
    New-Item -ItemType Directory -Force -Path (Join-Path $repository 'dist') | Out-Null
    & $wixTool build (Join-Path $PSScriptRoot 'Package.wxs') -arch x64 -d "Version=$Version" -d "BuildDir=$repository\target\release" -d "SourceDir=$repository" -o "$repository\dist\ResearchDashboard-$Version-x64.msi"
    if ($LASTEXITCODE -ne 0) { throw 'MSI build failed.' }
} finally { Pop-Location }
