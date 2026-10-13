#Requires -Version 7.4
param([string]$RepoRoot = (Split-Path -Parent $PSScriptRoot))

$ErrorActionPreference = 'Stop'
foreach ($contract in @('native-confirmation-token', 'native-install-plan-cancel')) {
    $schema = Join-Path $RepoRoot "crates/application/schema/$contract.schema.json"
    $fixturePath = Join-Path $RepoRoot "crates/application/schema/fixtures/$contract.cases.json"
    $cases = Get-Content -LiteralPath $fixturePath -Raw | ConvertFrom-Json -AsHashtable
    $validJson = $cases.valid | ConvertTo-Json -Depth 10 -Compress
    if (-not (Test-Json -Json $validJson -SchemaFile $schema)) {
        throw "$contract shape fixture does not match its schema."
    }

    foreach ($case in $cases.invalid) {
        $value = $validJson | ConvertFrom-Json -AsHashtable
        if ($case.ContainsKey('remove')) { $value.Remove($case.remove) }
        if ($case.ContainsKey('set')) {
            foreach ($key in $case.set.Keys) { $value[$key] = $case.set[$key] }
        }
        $json = $value | ConvertTo-Json -Depth 10 -Compress
        if (Test-Json -Json $json -SchemaFile $schema -ErrorAction SilentlyContinue) {
            throw "$contract schema accepted invalid fixture: $($case.name)"
        }
    }

    # Nonce bounds matter before lookup; fixtures contain no usable credentials.
    $negativeCount = $cases.invalid.Count
    if ($contract -eq 'native-confirmation-token') {
        $longNonce = $validJson | ConvertFrom-Json -AsHashtable
        $longNonce.value = 'x' * 129
        if (Test-Json -Json ($longNonce | ConvertTo-Json) -SchemaFile $schema -ErrorAction SilentlyContinue) {
            throw 'Native confirmation schema accepted an oversized nonce.'
        }
        $negativeCount++
    }
    Write-Output "Validated $contract schema: 1 valid shape / $negativeCount invalid shapes. Authorization is verified separately by application tests."
}
