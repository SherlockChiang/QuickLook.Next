param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"
$output = & dotnet list (Join-Path $Root "QuickLook.Next.slnx") package --vulnerable --include-transitive --format json
if ($LASTEXITCODE -ne 0) {
    throw "NuGet vulnerability audit failed with exit code $LASTEXITCODE."
}

$json = ($output -join [Environment]::NewLine) | ConvertFrom-Json -Depth 100
$findings = @(
    foreach ($project in @($json.projects)) {
        foreach ($framework in @($project.frameworks)) {
            $packages = @()
            if ($null -ne $framework.topLevelPackages) { $packages += @($framework.topLevelPackages) }
            if ($null -ne $framework.transitivePackages) { $packages += @($framework.transitivePackages) }
            foreach ($package in $packages) {
                if ($null -ne $package.vulnerabilities -and @($package.vulnerabilities).Count -gt 0) {
                    [pscustomobject]@{
                        Project = $project.path
                        Framework = $framework.framework
                        Package = $package.id
                        Version = $package.resolvedVersion
                        Vulnerabilities = @($package.vulnerabilities)
                    }
                }
            }
        }
    }
)

if ($findings.Count -gt 0) {
    $findings | Format-Table Project, Framework, Package, Version -AutoSize | Out-String | Write-Error
    throw "NuGet vulnerability audit found $($findings.Count) vulnerable package entries."
}

Write-Host "NuGet vulnerability audit passed" -ForegroundColor Green
