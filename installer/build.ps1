param()
$ErrorActionPreference = 'Stop'
$repository = Split-Path $PSScriptRoot -Parent
$originalTemp = $env:TEMP
$originalTmp = $env:TMP
$scratchRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$scratch = Join-Path $scratchRoot ('Scopework-package-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
$env:TEMP = $scratch
$env:TMP = $scratch
Push-Location $repository
try {
    $metadata = & cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read package version.' }
    $Version = ($metadata.packages | Where-Object { $_.name -eq 'research-dashboard' }).version
    if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw 'MSI requires a three-part numeric release version.' }
    & cargo build --release --locked --bins
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed.' }
    $wixTool = Join-Path $repository '.tools\wix.exe'
    if (-not (Test-Path -LiteralPath $wixTool)) {
        & dotnet tool install wix --tool-path (Join-Path $repository '.tools') --version 6.0.2
        if ($LASTEXITCODE -ne 0) { throw 'WiX setup failed.' }
    }
    $uiExtension = Join-Path $repository '.wix\extensions\WixToolset.UI.wixext\6.0.2\wixext6\WixToolset.UI.wixext.dll'
    if (-not (Test-Path -LiteralPath $uiExtension)) {
        & $wixTool extension add WixToolset.UI.wixext/6.0.2
        if ($LASTEXITCODE -ne 0) { throw 'WiX dialog library setup failed.' }
    }
    New-Item -ItemType Directory -Force -Path (Join-Path $repository 'dist') | Out-Null
    & $wixTool build (Join-Path $PSScriptRoot 'Package.wxs') (Join-Path $PSScriptRoot 'Wizard.wxs') -ext $uiExtension -culture en-US -arch x64 -intermediateFolder (Join-Path $scratch 'wix') -pdbtype none -d "Version=$Version" -d "BuildDir=$repository\target\release" -d "SourceDir=$repository" -o "$repository\dist\Scopework-$Version-x64.msi"
    if ($LASTEXITCODE -ne 0) { throw 'MSI build failed.' }
    $package = Join-Path $repository "dist\Scopework-$Version-x64.msi"
    $checksum = (Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash.ToLowerInvariant()
    "$checksum  $([System.IO.Path]::GetFileName($package))" | Set-Content -LiteralPath "$package.sha256" -Encoding ascii
} finally {
    Pop-Location
    $env:TEMP = $originalTemp
    $env:TMP = $originalTmp
    $resolvedScratch = [System.IO.Path]::GetFullPath($scratch)
    if ([System.IO.Path]::GetDirectoryName($resolvedScratch).TrimEnd('\') -ne $scratchRoot.TrimEnd('\') -or
        [System.IO.Path]::GetFileName($resolvedScratch) -notmatch '^Scopework-package-[0-9a-f]{32}$') {
        throw 'Refusing cleanup outside the owned package workspace.'
    }
    Remove-Item -LiteralPath $resolvedScratch -Recurse -Force
}
