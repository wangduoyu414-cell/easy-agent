param(
    [ValidateSet('x64', 'arm64')]
    [string]$Architecture = 'x64'
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$target = if ($Architecture -eq 'x64') { 'x86_64-pc-windows-msvc' } else { 'aarch64-pc-windows-msvc' }
$suffix = if ($Architecture -eq 'x64') { 'windows-x64' } else { 'windows-arm64' }
$distDir = Join-Path $repoRoot 'dist'
$sourceExe = Join-Path $repoRoot "target\$target\release\ai-client-installer.exe"
$outputExe = Join-Path $distDir "AI-Client-Installer-$suffix.exe"

Push-Location $repoRoot
try {
    cargo check --all-targets
    cargo test --test resolver_fixtures
    cargo test --test security_boundaries
    cargo build --release --target $target
    New-Item -ItemType Directory -Force -Path $distDir | Out-Null
    Copy-Item -LiteralPath $sourceExe -Destination $outputExe -Force
    $hash = (Get-FileHash -LiteralPath $outputExe -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $(Split-Path -Leaf $outputExe)" | Set-Content -LiteralPath (Join-Path $distDir "SHA256SUMS-$suffix.txt") -Encoding utf8NoBOM
    $artifactFiles = Get-ChildItem -LiteralPath $distDir -File | Where-Object { $_.Name -like 'AI-Client-Installer-*.exe' -or $_.Name -like 'AI-Client-Installer-*.dmg' } | Sort-Object Name
    $sumLines = foreach ($artifact in $artifactFiles) {
        $artifactHash = (Get-FileHash -LiteralPath $artifact.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        "$artifactHash  $($artifact.Name)"
    }
    $sumLines | Set-Content -LiteralPath (Join-Path $distDir 'SHA256SUMS.txt') -Encoding utf8NoBOM
    $manifestArtifacts = foreach ($artifact in $artifactFiles) {
        $artifactHash = (Get-FileHash -LiteralPath $artifact.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        [ordered]@{
            name = $artifact.Name
            bytes = $artifact.Length
            sha256 = $artifactHash
            signed = if ($artifact.Extension -eq '.exe') { (Get-AuthenticodeSignature -LiteralPath $artifact.FullName).Status -eq 'Valid' } else { $null }
        }
    }
    $manifest = [ordered]@{
        schema_version = 1
        version = '0.1.0'
        status = 'validation_pending'
        generated_at_utc = [DateTime]::UtcNow.ToString('o')
        artifacts = @($manifestArtifacts)
        pending_targets = @('windows-arm64', 'macos-universal', 'release-signing', 'clean-machine-product-install')
    }
    $manifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $distDir 'release-manifest.json') -Encoding utf8NoBOM
    Write-Host "Built: $outputExe"
    Write-Host "SHA256: $hash"
}
finally {
    Pop-Location
}
