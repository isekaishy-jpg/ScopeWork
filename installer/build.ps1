<#
.SYNOPSIS
Builds the Scopework MSI as a numbered package.
.DESCRIPTION
By default this reserves the next package build number before compiling: it raises both the
tracked BUILD_NUMBER and a high-water mark in Git's common directory, under an exclusive
package lock held for the whole build. Failed attempts leave gaps; numbers are never reused.
Commit the updated BUILD_NUMBER with the package. Ordinary cargo builds never reserve.

-UseCommittedNumber builds with the BUILD_NUMBER already committed and reserves nothing. CI
uses it: a release tag builds exactly the number its commit records.

The product version comes from Cargo.toml (0.13.0-alpha is 0.13.0a). The MSI ProductVersion is
major.minor.build: build numbers only grow, so every package upgrades the one before it.
#>
param(
    [switch] $UseCommittedNumber
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repository = Split-Path $PSScriptRoot -Parent
$originalTemp = $env:TEMP
$originalTmp = $env:TMP
$scratchRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$scratch = Join-Path $scratchRoot ('Scopework-package-' + [guid]::NewGuid().ToString('N'))

function ConvertTo-ProductVersion([string] $cargoVersion) {
    if ($cargoVersion -notmatch '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-(alpha|beta|rc))?$') {
        throw "Unsupported Cargo version '$cargoVersion': use X.Y.Z with -alpha, -beta, -rc or no stage."
    }
    $suffix = switch ($Matches[4]) { 'alpha' { 'a' } 'beta' { 'b' } 'rc' { 'rc' } default { 's' } }
    return [pscustomobject]@{
        Base    = "$($Matches[1]).$($Matches[2]).$($Matches[3])"
        Major   = [int]$Matches[1]
        Minor   = [int]$Matches[2]
        Product = "$($Matches[1]).$($Matches[2]).$($Matches[3])$suffix"
    }
}

function Write-Number([string] $path, [uint32] $number) {
    [IO.File]::WriteAllText($path, $number.ToString([Globalization.CultureInfo]::InvariantCulture) + "`n", [Text.UTF8Encoding]::new($false))
}

$numberPath = Join-Path $repository 'BUILD_NUMBER'
$packageLock = $null
New-Item -ItemType Directory -Path $scratch | Out-Null
$env:TEMP = $scratch
$env:TMP = $scratch
Push-Location $repository
try {
    $metadata = & cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read package version.' }
    $version = ConvertTo-ProductVersion (($metadata.packages | Where-Object { $_.name -eq 'research-dashboard' }).version)

    if ($UseCommittedNumber) {
        [uint32] $build = [uint32]::Parse([IO.File]::ReadAllText($numberPath).Trim(), [Globalization.CultureInfo]::InvariantCulture)
        Write-Host ('Building committed Build {0:D6}' -f $build)
    } else {
        $gitCommon = (& git -C $repository rev-parse --git-common-dir).Trim()
        if ($LASTEXITCODE -ne 0) { throw 'Cannot locate the package sequence directory (Git common directory).' }
        if (-not [IO.Path]::IsPathRooted($gitCommon)) { $gitCommon = Join-Path $repository $gitCommon }
        $gitCommon = [IO.Path]::GetFullPath($gitCommon)
        # No sharing for the whole build: two worktrees cannot stamp executables from one moving number.
        $packageLock = [IO.File]::Open((Join-Path $gitCommon 'scopework-package.lock'), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        $sequencePath = Join-Path $gitCommon 'scopework-build-number'
        [uint32] $tracked = [uint32]::Parse([IO.File]::ReadAllText($numberPath).Trim(), [Globalization.CultureInfo]::InvariantCulture)
        [uint32] $highWater = 0
        if (Test-Path -LiteralPath $sequencePath -PathType Leaf) {
            $highWater = [uint32]::Parse([IO.File]::ReadAllText($sequencePath).Trim(), [Globalization.CultureInfo]::InvariantCulture)
        }
        [uint32] $build = [Math]::Max($tracked, $highWater) + 1
        # Reserve before building: a later failure leaves a gap instead of a reused identity.
        Write-Number $sequencePath $build
        Write-Number $numberPath $build
        Write-Host ('Reserved Build {0:D6} for Scopework {1}' -f $build, $version.Product)
    }
    if ($version.Major -gt 255 -or $version.Minor -gt 255 -or $build -gt 65535) {
        throw "MSI ProductVersion $($version.Major).$($version.Minor).$build is out of range."
    }
    $msiVersion = "$($version.Major).$($version.Minor).$build"
    $display = 'Scopework {0} (Build {1:D6})' -f $version.Product, $build

    & cargo build --release --locked --bins
    if ($LASTEXITCODE -ne 0) { throw "Rust build failed; Build $build stays reserved." }
    $identity = & (Join-Path $repository 'target\release\research.exe') --build-info | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or $identity.build -ne $build -or $identity.version -ne $version.Product) {
        throw "The built executable reports $($identity.display), not $display."
    }

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
    $package = Join-Path $repository "dist\Scopework-$($version.Product)-x64.msi"
    & $wixTool build (Join-Path $PSScriptRoot 'Package.wxs') (Join-Path $PSScriptRoot 'Wizard.wxs') -ext $uiExtension -culture en-US -arch x64 -intermediateFolder (Join-Path $scratch 'wix') -pdbtype none -d "Version=$msiVersion" -d "DisplayVersion=$display" -d "BuildDir=$repository\target\release" -d "SourceDir=$repository" -o $package
    if ($LASTEXITCODE -ne 0) { throw 'MSI build failed.' }
    $checksum = (Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash.ToLowerInvariant()
    "$checksum  $([System.IO.Path]::GetFileName($package))" | Set-Content -LiteralPath "$package.sha256" -Encoding ascii
    Write-Host "$display packaged as $([System.IO.Path]::GetFileName($package)) (MSI ProductVersion $msiVersion)."
    if (-not $UseCommittedNumber) { Write-Host "Commit BUILD_NUMBER ($build) with this package." }
} finally {
    Pop-Location
    if ($packageLock) { $packageLock.Dispose() }
    $env:TEMP = $originalTemp
    $env:TMP = $originalTmp
    $resolvedScratch = [System.IO.Path]::GetFullPath($scratch)
    if ([System.IO.Path]::GetDirectoryName($resolvedScratch).TrimEnd('\') -ne $scratchRoot.TrimEnd('\') -or
        [System.IO.Path]::GetFileName($resolvedScratch) -notmatch '^Scopework-package-[0-9a-f]{32}$') {
        throw 'Refusing cleanup outside the owned package workspace.'
    }
    Remove-Item -LiteralPath $resolvedScratch -Recurse -Force
}
