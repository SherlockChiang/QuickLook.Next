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
    [switch]$Install
)

$ErrorActionPreference = "Stop"
& (Join-Path $PSScriptRoot "tools\build-local.ps1") @PSBoundParameters
