param([string]$Path = (Join-Path (Split-Path $PSScriptRoot -Parent) "packaging\Install.ps1"))

$ErrorActionPreference = "Stop"
$bytes = [System.IO.File]::ReadAllBytes($Path)
if (@($bytes | Where-Object { $_ -gt 127 }).Count -ne 0) {
    throw "Install.ps1 must remain ASCII for Windows PowerShell 5.1 compatibility."
}

$tokens = $null
$errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile(
    $Path,
    [ref]$tokens,
    [ref]$errors)
if ($errors.Count -ne 0) {
    throw "Install.ps1 parse failed: $($errors[0].Message)"
}

$releaseCertificate = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2(
    (Join-Path (Split-Path $PSScriptRoot -Parent) "packaging\QuickLook.Next-Release.cer"))
$thumbprintAssignments = @($ast.FindAll({
    param($node)
    $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
        $node.Left -is [System.Management.Automation.Language.VariableExpressionAst] -and
        $node.Left.VariablePath.UserPath -eq 'expectedThumbprint'
}, $true))
$thumbprintValue = if ($thumbprintAssignments.Count -eq 1 -and
    $thumbprintAssignments[0].Right -is [System.Management.Automation.Language.CommandExpressionAst]) {
    $thumbprintAssignments[0].Right.Expression
} else {
    $null
}
if ($thumbprintValue -isnot [System.Management.Automation.Language.StringConstantExpressionAst] -or
    $thumbprintValue.Value -ne $releaseCertificate.Thumbprint) {
    throw "Install.ps1 must pin the repository release certificate thumbprint."
}

Write-Host "installer script guard passed" -ForegroundColor Green
