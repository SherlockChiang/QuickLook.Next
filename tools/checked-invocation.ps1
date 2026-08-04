function Invoke-CheckedScript {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [hashtable]$Arguments = @{},
        [string]$FailureMessage = ""
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Required child script is missing: $Path"
    }

    # A previous native/script invocation may have left a non-zero automatic
    # variable behind. Reset it, invoke exactly one child, and capture both
    # status channels before any later command can overwrite them.
    $global:LASTEXITCODE = 0
    & $Path @Arguments
    $invocationSucceeded = $?
    $exitCode = $global:LASTEXITCODE
    if (-not $invocationSucceeded -or $exitCode -ne 0) {
        if (-not $FailureMessage) {
            $FailureMessage = "Child script failed: $Path"
        }
        throw "$FailureMessage (exit code $exitCode)."
    }
}

function Invoke-CheckedScriptBlock {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [scriptblock]$Script,
        [Parameter(Mandatory = $true)]
        [string]$FailureMessage
    )

    $global:LASTEXITCODE = 0
    & $Script
    $invocationSucceeded = $?
    $exitCode = $global:LASTEXITCODE
    if (-not $invocationSucceeded -or $exitCode -ne 0) {
        throw "$FailureMessage (exit code $exitCode)."
    }
}
