[CmdletBinding()]
param(
    [string]$RepoRoot,
    [switch]$SkipSkillScan
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3.0

function Fail([string]$Message) {
    throw "Contract validation failed: $Message"
}

function Require([bool]$Condition, [string]$Message) {
    if (-not $Condition) {
        Fail $Message
    }
}

function Has-Property($Object, [string]$Name) {
    if ($null -eq $Object) {
        return $false
    }

    return $null -ne $Object.PSObject.Properties[$Name]
}

function Read-JsonFile([string]$Path) {
    Require (Test-Path -LiteralPath $Path -PathType Leaf) "Missing JSON file: $Path"
    try {
        return (Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json)
    } catch {
        Fail "Invalid JSON in ${Path}: $($_.Exception.Message)"
    }
}

if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    $RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path
} else {
    $RepoRoot = (Resolve-Path -LiteralPath $RepoRoot).Path
}

$schemaPath = Join-Path $RepoRoot 'catalog\schema\runtime-catalog.schema.json'
$fixturePath = Join-Path $RepoRoot 'catalog\fixtures\php-manifest.fixture.json'
$schemaText = Get-Content -LiteralPath $schemaPath -Raw
$fixtureText = Get-Content -LiteralPath $fixturePath -Raw
$schema = Read-JsonFile $schemaPath
$fixture = Read-JsonFile $fixturePath

Require ([string]$schema.'$schema' -eq 'https://json-schema.org/draft/2020-12/schema') 'Catalog schema must use JSON Schema 2020-12.'
Require ([string]$schema.'$id' -eq 'urn:cxszn-menv:schema:runtime-catalog:1') 'Catalog schema must have the stable MEnv URN $id.'
Require ((@($schema.required) -contains 'signature') -and (@($schema.required) -contains 'artifacts')) 'Catalog schema must require signature and artifacts.'
Require ($schemaText -match 'not_for_download') 'Catalog schema must model non-downloadable fixtures explicitly.'
Require ($schemaText -match 'revoked_artifact_ids') 'Catalog schema must model artifact revocation.'
Require ($schemaText -match 'sha256') 'Catalog schema must require an artifact SHA-256 digest.'
Require ($schemaText -match 'redirect_hosts') 'Catalog schema must constrain redirect hosts.'

Require ([string]$fixture.schema_version -eq '1') 'Fixture schema_version must be 1.'
Require ([bool]$fixture.fixture) 'PHP manifest fixture must be marked fixture=true.'
Require ($null -eq $fixture.signature) 'Test fixture must not contain a fake signature.'
Require ([string]$fixture.fixture_note -match '(?i)test only') 'Fixture note must state that the file is test-only.'
try {
    $generatedAt = [DateTimeOffset]::Parse([string]$fixture.generated_at)
    $expiresAt = [DateTimeOffset]::Parse([string]$fixture.expires_at)
    Require ($expiresAt -gt $generatedAt) 'Fixture expires_at must be later than generated_at.'
} catch {
    Fail "Fixture timestamps are not valid RFC 3339 values: $($_.Exception.Message)"
}
Require ($fixtureText -notmatch '(?i)(?:https?|ftp)://') 'Fixture must not contain a download URL.'
Require ($fixtureText -notmatch '-----BEGIN [^-]+ PRIVATE KEY-----') 'Fixture must not contain private-key material.'

$artifactIds = New-Object 'System.Collections.Generic.HashSet[string]'
$artifacts = @($fixture.artifacts)
Require ($artifacts.Count -gt 0) 'Fixture must contain at least one artifact.'

foreach ($artifact in $artifacts) {
    $artifactId = [string]$artifact.artifact_id
    Require ($artifactIds.Add($artifactId)) "Duplicate fixture artifact_id: $artifactId"
    Require ([string]$artifact.runtime_family -eq 'php') "Fixture artifact is not PHP: $artifactId"
    Require ([string]$artifact.platform -eq 'windows') "Fixture artifact is not Windows-only: $artifactId"
    Require ([string]$artifact.source.kind -eq 'fixture') "Fixture artifact source must be kind=fixture: $artifactId"
    Require ([bool]$artifact.source.not_for_download) "Fixture artifact must be marked not_for_download: $artifactId"
    Require (-not (Has-Property $artifact.source 'host')) "Fixture source must not contain a host: $artifactId"
    Require (-not (Has-Property $artifact.source 'artifact_path')) "Fixture source must not contain an artifact path: $artifactId"
    Require ([string]$artifact.file_name -notmatch '[\\/]') "Artifact filename must not contain a path: $artifactId"
    Require ([string]$artifact.sha256 -match '^[A-Fa-f0-9]{64}$') "Artifact SHA-256 must be 64 hexadecimal characters: $artifactId"
    Require ([string]$artifact.entrypoint -match '^[A-Za-z0-9][A-Za-z0-9_.-]*\.exe$') "Artifact entrypoint must be a filename: $artifactId"
    Require (-not [bool]$artifact.flags.public_listener_default) "Fixture artifact must not default to a public listener: $artifactId"
}

if (-not $SkipSkillScan) {
    $skillRoot = Join-Path $RepoRoot '.agents\skills\cxszn-menv-operator'
    Require (Test-Path -LiteralPath (Join-Path $skillRoot 'SKILL.md') -PathType Leaf) 'Missing canonical SKILL.md.'
    $skillFiles = @(Get-ChildItem -LiteralPath $skillRoot -Recurse -File -Include '*.md', '*.ps1')
    $requiredTools = @(
        'menv_get_environment_state',
        'menv_scan_installed_runtimes',
        'menv_list_runtime_catalog',
        'menv_verify_runtime_path',
        'menv_plan_environment_change',
        'menv_apply_environment_change',
        'menv_get_operation_status',
        'menv_cancel_operation'
    )

    foreach ($requiredTool in $requiredTools) {
        $toolFound = $false
        foreach ($skillFile in $skillFiles) {
            if ((Get-Content -LiteralPath $skillFile.FullName -Raw).Contains($requiredTool)) {
                $toolFound = $true
                break
            }
        }
        Require $toolFound "Skill contract is missing tool name: $requiredTool"
    }

    foreach ($skillFile in $skillFiles) {
        $content = Get-Content -LiteralPath $skillFile.FullName -Raw
        Require ($content -notmatch '-----BEGIN [^-]+ PRIVATE KEY-----') "Private-key marker found in skill file: $($skillFile.Name)"
        Require ($content -notmatch '(?i)\b(?:api[_-]?key|client[_-]?secret)\s*[:=]\s*[^\s<]{8,}') "Secret-like assignment found in skill file: $($skillFile.Name)"
        Require ($content -notmatch '(?i)(?:[A-Z]:\\|/(?:Users|home|root|var|etc)/)') "Machine-specific absolute path found in skill file: $($skillFile.Name)"
    }
}

Write-Output "Contract validation passed: catalog schema, PHP fixture, and operator skill ($RepoRoot)"
