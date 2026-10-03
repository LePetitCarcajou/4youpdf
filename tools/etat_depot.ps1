# Gathers in one Markdown file what a new conversation about the project
# needs: Git state, roadmap, backlogs, Claude Code rules, and the end of the
# latest session files. Reads only; writes the file outside the repository.
#
# Usage, from the repository root:
#   powershell -ExecutionPolicy Bypass -File tools\etat_depot.ps1
#   powershell -ExecutionPolicy Bypass -File tools\etat_depot.ps1 -Depuis v0.5.0

param(
    [string]$Depuis = "",
    [string]$Sortie = ""
)

$ErrorActionPreference = "Stop"
$racine = Split-Path -Parent $PSScriptRoot
Set-Location $racine

if (-not $Depuis) {
    $Depuis = (git describe --tags --abbrev=0 2>$null)
}
if (-not $Sortie) {
    $dossier = Join-Path (Split-Path -Parent $racine) "4YouPDF-patches"
    New-Item -ItemType Directory -Force $dossier | Out-Null
    $Sortie = Join-Path $dossier ("etat-" + (Get-Date -Format "yyyy-MM-dd") + ".md")
}

$fichiers = @(
    "docs\feuille-de-route.md", "docs\paliers.md",
    "docs\backlog-technique.md", "docs\backlog-ui.md",
    "CLAUDE.md", ".claude\settings.json", "docs\sessions\_gabarit.md"
)
$fichiers += Get-ChildItem .claude\commands\*.md, .claude\agents\*.md -ErrorAction SilentlyContinue |
    ForEach-Object { Resolve-Path -Relative $_.FullName }

# The three most recent session files of each kind, tail only.
$sessions = Get-ChildItem docs\sessions\*-rapport.md, docs\sessions\*-relecture.md, docs\sessions\*-tests.md -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending | Select-Object -First 3

& {
    "# Etat du depot - $(Get-Date -Format 'yyyy-MM-dd HH:mm')"
    "## git status";               '````'; git status -sb; '````'
    "## git log $Depuis..HEAD";    '````'; git log --oneline "$Depuis..HEAD"; '````'
    "## Derniers tags";            '````'; git tag --sort=-creatordate | Select-Object -First 5; '````'
    "## Version du workspace";     '````'; Select-String -Path Cargo.toml -Pattern '^version'; '````'
    "## docs/sessions";            '````'; Get-ChildItem docs\sessions -Name; '````'
    "## claude --version";         '````'; claude --version; '````'
    foreach ($f in $fichiers) {
        if (Test-Path $f) { "## $f"; '````'; Get-Content $f -Encoding UTF8; '````' }
    }
    foreach ($s in $sessions) {
        "## $($s.Name) (fin)"; '````'; Get-Content $s.FullName -Encoding UTF8 -Tail 40; '````'
    }
    "## CHANGELOG.md (debut)";     '````'; Get-Content CHANGELOG.md -Encoding UTF8 -TotalCount 60; '````'
} | Out-File $Sortie -Encoding utf8

Get-Item $Sortie | Select-Object FullName, Length
