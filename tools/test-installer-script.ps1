param([string]$Path = (Join-Path (Split-Path $PSScriptRoot -Parent) "packaging\Install.ps1"))

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "checked-invocation.ps1")
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

function Get-InstallerCommands([string]$Name) {
    return @($ast.FindAll({
        param($node)
        $node -is [System.Management.Automation.Language.CommandAst] -and
            $node.GetCommandName() -eq $Name
    }, $true))
}

function Get-Ancestor {
    param(
        [Parameter(Mandatory = $true)]$Node,
        [Parameter(Mandatory = $true)][type]$Type
    )

    $current = $Node.Parent
    while ($current) {
        if ($Type.IsInstanceOfType($current)) { return $current }
        $current = $current.Parent
    }
    return $null
}

function Assert-OneCommand([string]$Name) {
    $commands = @(Get-InstallerCommands -Name $Name)
    if ($commands.Count -ne 1) {
        throw "Install.ps1 must invoke $Name exactly once; found $($commands.Count)."
    }
    return $commands[0]
}

$signatureCommands = @(Get-InstallerCommands -Name "Get-AuthenticodeSignature")
if ($signatureCommands.Count -ne 2) {
    throw "Install.ps1 must verify the MSIX signature before and after machine trust."
}
$signatureCommands = @($signatureCommands | Sort-Object { $_.Extent.StartOffset })
$identityCommand = Assert-OneCommand -Name "Get-MsixIdentity"
$trustProbeCommands = @(Get-InstallerCommands -Name "Test-MachineCertificateTrust" |
    Sort-Object { $_.Extent.StartOffset })
$elevationCommands = @(Get-InstallerCommands -Name "Start-Process" |
    Sort-Object { $_.Extent.StartOffset })
$installCommand = Assert-OneCommand -Name "Add-AppxPackage"
$installedPackageCommands = @(Get-InstallerCommands -Name "Get-AppxPackage" |
    Sort-Object { $_.Extent.StartOffset })
$rollbackCommands = @(Get-InstallerCommands -Name "Remove-MachineCertificateTrust" |
    Sort-Object { $_.Extent.StartOffset })
if ($trustProbeCommands.Count -ne 2 -or $elevationCommands.Count -ne 2 -or
    $installedPackageCommands.Count -ne 2 -or
    $rollbackCommands.Count -ne 2) {
    throw "Installer trust setup and rollback helper structure is incomplete."
}
$existingPackageCommand = $installedPackageCommands[0]
$installedPackageCommand = $installedPackageCommands[1]

$preflightOffsets = @(
    $signatureCommands[0].Extent.StartOffset,
    $identityCommand.Extent.StartOffset,
    $existingPackageCommand.Extent.StartOffset,
    $trustProbeCommands[0].Extent.StartOffset)
if (@($preflightOffsets | Where-Object {
            $_ -ge $elevationCommands[0].Extent.StartOffset }).Count -ne 0) {
    throw "Installer file, identity, installed-state, and trust preflight must run before elevation."
}
if ($existingPackageCommand.Extent.StartOffset -le
        $identityCommand.Extent.StartOffset -or
    $trustProbeCommands[0].Extent.StartOffset -le
        $existingPackageCommand.Extent.StartOffset -or
    $signatureCommands[1].Extent.StartOffset -le $elevationCommands[0].Extent.StartOffset -or
    $installCommand.Extent.StartOffset -le $signatureCommands[1].Extent.StartOffset -or
    $installedPackageCommand.Extent.StartOffset -le $installCommand.Extent.StartOffset -or
    $elevationCommands[1].Extent.StartOffset -le $installedPackageCommand.Extent.StartOffset) {
    throw "Installer installed-state, trust, installation, and postcondition checks are out of order."
}

$trustOnlyBranches = @($ast.FindAll({
    param($node)
    $node -is [System.Management.Automation.Language.IfStatementAst] -and
        $node.Clauses.Count -eq 1 -and
        $node.Clauses[0].Item1.Extent.Text -match '^\s*\$TrustOnly\s*$'
}, $true))
if ($trustOnlyBranches.Count -ne 1 -or
    $trustOnlyBranches[0].Extent.Text -match 'Add-AppxPackage|Get-AppxPackage' -or
    $trustOnlyBranches[0].Extent.Text -notmatch 'Add-MachineCertificateTrust' -or
    $trustOnlyBranches[0].Extent.Text -notmatch 'Remove-MachineCertificateTrust' -or
    $trustOnlyBranches[0].Extent.Text -notmatch 'exit\s+0' -or
    $trustOnlyBranches[0].Extent.Text -notmatch 'exit\s+10') {
    throw "The elevated TrustOnly branch must manage only machine trust and then exit."
}
if ($existingPackageCommand.Extent.StartOffset -le
    $trustOnlyBranches[0].Extent.EndOffset) {
    throw "The installed-package preflight must run only on the original-user path."
}

$alreadyCurrentBranches = @($ast.FindAll({
    param($node)
    $node -is [System.Management.Automation.Language.IfStatementAst] -and
        $node.Clauses.Count -eq 1 -and
        $node.Clauses[0].Item1.Extent.Text -match
            '^\s*\$targetVersion\s+-eq\s+\$installedVersion\s*$'
}, $true))
if ($alreadyCurrentBranches.Count -ne 1 -or
    $alreadyCurrentBranches[0].Extent.EndOffset -ge
        $trustProbeCommands[0].Extent.StartOffset -or
    $alreadyCurrentBranches[0].Extent.Text -notmatch
        'Write-Host[\s\S]*already installed at this version[\s\S]*return' -or
    $alreadyCurrentBranches[0].Extent.Text -match
        'Add-AppxPackage|Start-Process|Add-MachineCertificateTrust') {
    throw "An identical installed version must return success before trust or registration."
}

$exitStatements = @($ast.FindAll({
    param($node)
    $node -is [System.Management.Automation.Language.ExitStatementAst]
}, $true))
foreach ($exitStatement in $exitStatements) {
    $trustOnlyAncestor = Get-Ancestor -Node $exitStatement `
        -Type ([System.Management.Automation.Language.IfStatementAst])
    while ($trustOnlyAncestor -and
        $trustOnlyAncestor.Extent.StartOffset -ne
            $trustOnlyBranches[0].Extent.StartOffset) {
        $trustOnlyAncestor = Get-Ancestor -Node $trustOnlyAncestor `
            -Type ([System.Management.Automation.Language.IfStatementAst])
    }
    if (-not $trustOnlyAncestor) {
        throw "The original-user installer path must never exit after elevation."
    }
}

$installTry = Get-Ancestor -Node $installCommand `
    -Type ([System.Management.Automation.Language.TryStatementAst])
$rollbackCatch = Get-Ancestor -Node $rollbackCommands[1] `
    -Type ([System.Management.Automation.Language.CatchClauseAst])
$rollbackIfs = @()
$rollbackAncestor = $rollbackCommands[1].Parent
while ($rollbackAncestor) {
    if ($rollbackAncestor -is
        [System.Management.Automation.Language.IfStatementAst]) {
        $rollbackIfs += $rollbackAncestor
    }
    $rollbackAncestor = $rollbackAncestor.Parent
}
$rollbackGate = @($rollbackIfs | Where-Object {
    $_.Clauses[0].Item1.Extent.Text -match '(?i)\$addedCertificate' -and
        $_.Clauses[0].Item1.Extent.Text -match '(?i)-not\s+\$registrationCompleted'
})
if (-not $installTry -or $installTry.CatchClauses.Count -eq 0 -or
    -not $rollbackCatch -or $rollbackGate.Count -ne 1) {
    throw "Installer rollback must require added trust and incomplete registration."
}
if ($installTry.Extent.StartOffset -ge $installCommand.Extent.StartOffset -or
    $installTry.Extent.EndOffset -le $installedPackageCommand.Extent.EndOffset) {
    throw "Installation and current-user package verification must share the rollback boundary."
}

$text = [System.Text.Encoding]::ASCII.GetString($bytes)
$requiredPatterns = @(
    @('DtdProcessing\]::Prohibit[\s\S]*XmlResolver\s*=\s*\$null', "MSIX manifest parsing must prohibit external XML resolution."),
    @('\[string\]\$signature\.Status\s+-notin\s+@\("Valid",\s*"NotTrusted",\s*"UnknownError"\)', "Pre-trust signature checks must reject integrity failures while allowing known trust states."),
    @('\$targetVersion\s*=\s*\$null[\s\S]{0,500}\[version\]::TryParse\([\s\S]{0,150}\$packageIdentity\.Version[\s\S]{0,250}\$targetVersion\.Major\s+-gt\s+65535[\s\S]{0,250}\$targetVersion\.Revision\s+-gt\s+65535', "The manifest target version must parse and remain within MSIX component bounds before side effects."),
    @('param\([\s\S]{0,200}\[switch\]\$TrustOnly[\s\S]{0,100}\[switch\]\$RemoveTrust', "Trust elevation must use explicit internal helper switches."),
    @('if\s*\(\$TrustOnly\)[\s\S]{0,900}exit\s+10', "The trust-only helper must finish before the original-user install path."),
    @('\$arguments\s*=\s*@\([\s\S]{0,400}"-TrustOnly"\)[\s\S]{0,300}Start-Process[\s\S]{0,1000}\$addedCertificate\s*=\s*\$elevated\.ExitCode\s+-eq\s+0', "Only the trust helper may be elevated before original-user registration."),
    @('\$rollbackArguments\s*=\s*@\([\s\S]{0,400}"-TrustOnly",\s*"-RemoveTrust"\)[\s\S]{0,500}Start-Process', "A standard-user rollback must elevate only the trust helper."),
    @('Open\("ReadOnly"\)[\s\S]{0,300}FindByThumbprint', "The pre-elevation machine-trust probe must be read-only and thumbprint-specific."),
    @('Open\("ReadWrite"\)[\s\S]{0,500}\$trustStore\.Add\(\$Certificate\)[\s\S]{0,100}return\s+\$true', "The trust helper must report exactly when it adds machine trust."),
    @('\$existingPackages\.Count\s+-gt\s+1[\s\S]{0,300}installed QuickLook Next package identity is invalid', "Ambiguous current-user package results must fail closed."),
    @('\$existingPackages\s*=\s*@\(\s*Get-AppxPackage\s+-Name\s+\$expectedPackageName\s+-ErrorAction\s+Stop[\s\S]{0,900}\[string\]\$existingPackage\.Name\s+-ne\s+\$packageIdentity\.Name[\s\S]{0,500}\$existingPackage\.Publisher[\s\S]{0,500}\[version\]::TryParse[\s\S]{0,500}\$installedVersion\.Revision\s+-gt\s+65535', "The original-user preflight must fail closed while validating and bounding the installed package name, publisher, and version."),
    @('\$targetVersion\s+-eq\s+\$installedVersion[\s\S]{0,900}already installed at this version[\s\S]{0,500}return[\s\S]{0,180}\$targetVersion\s+-lt\s+\$installedVersion', "An already-current package must succeed before trust setup without application shutdown or re-registration."),
    @('\$targetVersion\s+-lt\s+\$installedVersion[\s\S]{0,300}newer version of QuickLook Next', "The installer must reject a current-user downgrade before trust setup."),
    @('Add-AppxPackage\s+-Path\s+\$package\.FullName\s+-ForceApplicationShutdown', "The installer must update the package without uninstalling it."),
    @('Add-AppxPackage[\s\S]{0,150}\$registrationCompleted\s*=\s*\$true[\s\S]{0,700}Certificate trust was retained', "Successful registration must retain trust if its postcondition fails."),
    @('\$installedPackages\s*=\s*@\(\s*Get-AppxPackage\s+-Name\s+\$expectedPackageName[\s\S]{0,500}\.Name\s+-ne\s+\$packageIdentity\.Name[\s\S]{0,200}\.Publisher\s+-ne\s+\$packageIdentity\.Publisher[\s\S]{0,200}Version\.ToString\(\)\s+-ne\s+\$packageIdentity\.Version', "The installed current-user package must match the manifest name, publisher, and version."),
    @('catch\s*\{[\s\S]*if\s*\(\$addedCertificate\s+-and\s+-not\s+\$registrationCompleted\)[\s\S]{0,500}Remove-MachineCertificateTrust', "Certificate rollback must stop after package registration succeeds.")
)
foreach ($rule in $requiredPatterns) {
    if ($text -notmatch $rule[0]) { throw $rule[1] }
}
if ($text -match 'exit\s+\$elevated\.ExitCode|Remove-AppxPackage|ForceUpdateFromAnyVersion|Get-AppxPackage\s+[^\r\n]*-(AllUsers|User)') {
    throw "The installer must not uninstall, downgrade, or query another user's package."
}

Invoke-CheckedScript `
    -Path (Join-Path $PSScriptRoot "test-installer-control-flow.ps1") `
    -Arguments @{
        Path = $Path
        Root = (Split-Path $PSScriptRoot -Parent)
    } `
    -FailureMessage "Installer executable control-flow test failed"

Write-Host "installer script guard passed" -ForegroundColor Green
