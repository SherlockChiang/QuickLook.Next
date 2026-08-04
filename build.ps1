[CmdletBinding(PositionalBinding = $false)]
param(
    [Parameter(Position = 0)]
    [string]$Version = "",
    [ValidateSet("None", "Patch", "Minor", "Major")]
    [string]$Bump = "None",
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",
    [string]$VersionSuffix = "",
    [switch]$NoRestore,
    [switch]$Test,
    [switch]$Package,
    [switch]$Install
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "tools\checked-invocation.ps1")
Invoke-CheckedScript -Path (Join-Path $PSScriptRoot "tools\build-local.ps1") `
    -Arguments $PSBoundParameters `
    -FailureMessage "Local build workflow failed"
