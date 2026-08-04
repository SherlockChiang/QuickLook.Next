param(
    [string]$Root = (Split-Path $PSScriptRoot -Parent)
)

$ErrorActionPreference = "Stop"

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Assert-Matches([string]$Text, [string]$Pattern, [string]$Message) {
    Assert-True ($Text -match $Pattern) $Message
}

$relativePaths = @(
    "AGENTS.md",
    "docs/adr/README.md",
    "docs/adr/0001-supervised-preview-hosts.md",
    "docs/adr/0002-handle-ownership-and-path-authority.md",
    "docs/adr/0003-cancellation-and-stale-work.md",
    "docs/adr/0004-preview-error-contracts.md"
)

foreach ($relativePath in $relativePaths) {
    $fullPath = Join-Path $Root $relativePath
    Assert-True (Test-Path -LiteralPath $fullPath -PathType Leaf) `
        "Tracked architecture guidance is missing: $relativePath"
    $trackedPath = @(& git -C $Root ls-files --error-unmatch -- $relativePath 2>$null)
    Assert-True ($LASTEXITCODE -eq 0 -and $trackedPath.Count -eq 1) `
        "Architecture guidance must be tracked by Git: $relativePath"
}

& git -C $Root check-ignore --quiet -- AGENTS.md 2>$null
Assert-True ($LASTEXITCODE -eq 1) "Tracked AGENTS.md must not be ignored."
& git -C $Root check-ignore --quiet -- agent.md 2>$null
Assert-True ($LASTEXITCODE -eq 0) `
    "The legacy lowercase agent.md must remain ignored local scratch material."

$agentsText = Get-Content -LiteralPath (Join-Path $Root "AGENTS.md") -Raw
foreach ($rule in @(
    @('Rust-first', "AGENTS.md must preserve the Rust-first default."),
    @('QuickLook\.Next\.App[\s\S]*thin WinUI shell', "AGENTS.md must keep the App as a thin WinUI shell."),
    @('ParserHost[\s\S]*Rust FFI', "AGENTS.md must keep ParserHost at the Rust FFI boundary."),
    @('RasterHost[\s\S]*surface production', "AGENTS.md must keep RasterHost scoped to surfaces."),
    @('ShellBroker[\s\S]*Explorer/Shell compatibility', "AGENTS.md must keep Shell compatibility in ShellBroker."),
    @('WebView/WebView2', "AGENTS.md must forbid WebView preview rendering."),
    @('Plugin\.\*[\s\S]*outside default', "AGENTS.md must keep legacy plugins off the default path."),
    @('logical path[\s\S]*metadata', "AGENTS.md must deny logical-path file authority."),
    @('generation[\s\S]*request-ID[\s\S]*cancellation', "AGENTS.md must require stale-work identity checks."),
    @('stable error codes[\s\S]*never host exception text', "AGENTS.md must require typed error branching."),
    @('QuickLook\.Next\.Native\.proj', "AGENTS.md must use the shared Native MSBuild build contract."),
    @('docs/adr/README\.md', "AGENTS.md must link the accepted decision index.")
)) {
    Assert-Matches $agentsText $rule[0] $rule[1]
}
Assert-True ($agentsText -notmatch '(?m)^\s*cargo\s+build\b') `
    "AGENTS.md must not reintroduce a raw Cargo build path."
Assert-True ($agentsText -notmatch 'Current High-Priority Work') `
    "Transient roadmap priorities do not belong in repository instructions."

$indexText = Get-Content -LiteralPath (Join-Path $Root "docs/adr/README.md") -Raw
foreach ($relativePath in $relativePaths | Select-Object -Skip 2) {
    $leaf = Split-Path $relativePath -Leaf
    Assert-Matches $indexText ([regex]::Escape($leaf)) `
        "The ADR index must link $leaf."
}

$adrTexts = @{}
foreach ($relativePath in $relativePaths | Select-Object -Skip 2) {
    $text = Get-Content -LiteralPath (Join-Path $Root $relativePath) -Raw
    $adrTexts[$relativePath] = $text
    foreach ($heading in @("Status: Accepted", "## Context", "## Decision", "## Consequences", "## Verification")) {
        Assert-Matches $text ([regex]::Escape($heading)) `
            "$relativePath is missing required ADR section '$heading'."
    }
}

$processAdr = $adrTexts["docs/adr/0001-supervised-preview-hosts.md"]
Assert-Matches $processAdr 'created suspended[\s\S]*assigned to the job[\s\S]*resumed' `
    "The process ADR must preserve assign-before-resume containment."
Assert-Matches $processAdr 'ParserHost and ShellBroker[\s\S]*write-restricted[\s\S]*RasterHost' `
    "The process ADR must preserve the current write-restriction split."
Assert-Matches $processAdr 'WER[\s\S]*local-dump' `
    "The process ADR must suppress UI without discarding crash evidence."
Assert-Matches $processAdr 'broader-privilege' `
    "Host failures must not retry across a broader trust boundary."

$handleAdr = $adrTexts["docs/adr/0002-handle-ownership-and-path-authority.md"]
Assert-Matches $handleAdr 'logical[\s\S]*paths are untrusted[\s\S]*do not grant file authority' `
    "The HANDLE ADR must deny logical-path authority."
Assert-Matches $handleAdr 'adopts every nonzero transferred HANDLE[\s\S]*before validating' `
    "The HANDLE ADR must require immediate receiver adoption."
Assert-Matches $handleAdr 'Rust receives a bounded borrowed/reopened view' `
    "The HANDLE ADR must distinguish IPC ownership from FFI borrowing."
Assert-Matches $handleAdr 'must not become an implicit fallback' `
    "Pinned HANDLE failures must never silently fall back to paths."
Assert-Matches $handleAdr 'handle-based-preview-inputs\.md' `
    "The ADR must link the detailed HANDLE protocol instead of duplicating it."

$cancellationAdr = $adrTexts["docs/adr/0003-cancellation-and-stale-work.md"]
Assert-Matches $cancellationAdr 'generation[\s\S]*cancellation token[\s\S]*session\s+snapshot' `
    "The cancellation ADR must bind work to a preview snapshot."
Assert-Matches $cancellationAdr 'at most one[\s\S]*terminal Host message' `
    "The cancellation ADR must define exactly-once accepted Host completion."
Assert-Matches $cancellationAdr 'drain it within a hard bound[\s\S]*recycle/terminate' `
    "The cancellation ADR must drain before disposal or fail-stop the host."
Assert-Matches $cancellationAdr 'ShellBroker[\s\S]*R26-P1-09' `
    "The cancellation ADR must disclose the synchronous ShellBroker exception."

$errorAdr = $adrTexts["docs/adr/0004-preview-error-contracts.md"]
Assert-Matches $errorAdr 'at most one typed host terminal' `
    "The error ADR must distinguish Host terminals from local failure."
Assert-Matches $errorAdr 'must never branch\s+on\s+the\s+human-readable\s+host\s+message' `
    "The error ADR must prohibit stringly typed recovery logic."
Assert-Matches $errorAdr 'generation[\s\S]*cancellation token[\s\S]*path' `
    "Error UI/actions must bind to the current preview snapshot."
Assert-Matches $errorAdr 'R26-P1-04[\s\S]*does not claim' `
    "The ADR must not claim that Copy Diagnostics is already implemented."

$protocolText = Get-Content -LiteralPath (Join-Path $Root "src/QuickLook.Next.Core/Protocol.cs") -Raw
$pendingText = Get-Content -LiteralPath (Join-Path $Root "src/QuickLook.Next.Core/PendingRequests.cs") -Raw
Assert-Matches $protocolText 'at most one terminal Host message[\s\S]*client cancellation' `
    "Protocol comments must include cancellation and local failure."
Assert-Matches $pendingText 'at most one terminal Host message[\s\S]*Cancellation removes' `
    "Pending-request comments must describe cancellation separately from Host errors."

$roadmapText = @(
    Get-Content -LiteralPath (Join-Path $Root "docs/post-office-roadmap.md") -Raw
    Get-Content -LiteralPath (Join-Path $Root "docs/prd-next-preview-optimization.md") -Raw
) -join [Environment]::NewLine
Assert-True ($roadmapText -notmatch 'RasterHost[^\r\n]*(?:shell|Shell)[^\r\n]*thumbnail') `
    "Historical roadmaps must not assign Shell thumbnails to RasterHost."
Assert-Matches $roadmapText 'ShellBroker[\s\S]*Shell thumbnail compatibility' `
    "Historical roadmaps must point Shell thumbnail compatibility to ShellBroker."

$contributingText = Get-Content -LiteralPath (Join-Path $Root "CONTRIBUTING.md") -Raw
$readmeText = Get-Content -LiteralPath (Join-Path $Root "README.md") -Raw
Assert-Matches $contributingText 'AGENTS\.md[\s\S]*docs/adr/README\.md' `
    "Contributor guidance must link the tracked contracts."
Assert-Matches $readmeText 'AGENTS\.md[\s\S]*docs/adr/README\.md' `
    "The README architecture summary must link the tracked contracts."

Write-Host "architecture guidance tests passed" -ForegroundColor Green
exit 0
