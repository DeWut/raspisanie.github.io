param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("promote", "rollback")]
    [string]$Mode,
    [Parameter(Mandatory = $true)]
    [string]$Root
)

$ErrorActionPreference = "Stop"
$Root = [System.IO.Path]::GetFullPath($Root)
$Training = Join-Path $Root "training"
$CandidatePointer = Join-Path $Training "barney-nano-candidate.json"
$ReadinessPath = Join-Path $Training "barney-nano-readiness.json"
$StatePath = Join-Path $Training "barney-nano-render-deploy.json"
$ApiRoot = "https://api.render.com/v1"
$ExpectedServiceName = "spbftuit-smart-campus-api"

function Write-Header([string]$Message) {
    Write-Host ""
    Write-Host "============================================================" -ForegroundColor Cyan
    Write-Host " $Message" -ForegroundColor Cyan
    Write-Host "============================================================" -ForegroundColor Cyan
}
function Read-JsonFile([string]$Path) {
    if (-not (Test-Path $Path)) { throw "Required file not found: $Path" }
    return Get-Content -Path $Path -Raw -Encoding UTF8 | ConvertFrom-Json
}
function Get-SecretText([string]$Prompt) {
    $secure = Read-Host -Prompt $Prompt -AsSecureString
    $pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
    try { return [Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer) }
    finally {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer)
        $secure.Dispose()
    }
}
function Invoke-RenderApi([string]$Method, [string]$Path, $Body = $null) {
    $uri = $ApiRoot + $Path
    $headers = @{ Authorization = "Bearer $script:RenderToken"; Accept = "application/json" }
    try {
        if ($null -eq $Body) {
            return Invoke-RestMethod -Method $Method -Uri $uri -Headers $headers -TimeoutSec 45
        }
        $json = $Body | ConvertTo-Json -Depth 10 -Compress
        return Invoke-RestMethod -Method $Method -Uri $uri -Headers $headers -ContentType "application/json" -Body $json -TimeoutSec 45
    }
    catch {
        throw "Render API request failed ($Method $Path). Check the API key, service ID and permissions. Details: $($_.Exception.Message)"
    }
}
function Get-ServiceEnvTable([string]$ServiceId) {
    $response = Invoke-RenderApi "GET" "/services/$ServiceId/env-vars?limit=100"
    $table = @{}
    foreach ($entry in @($response)) {
        $item = if ($entry.envVar) { $entry.envVar } else { $entry }
        if ($item.key) { $table[[string]$item.key] = [string]$item.value }
    }
    return $table
}
function Get-ServiceObject([string]$ServiceId) {
    $result = Invoke-RenderApi "GET" "/services/$ServiceId"
    if ($result.service) { return $result.service }
    return $result
}
function Set-ModelVariable([string]$ServiceId, [string]$ModelTag) {
    # Update only OLLAMA_MODEL. Do not PUT the whole environment-variable list.
    Invoke-RenderApi "PUT" "/services/$ServiceId/env-vars/OLLAMA_MODEL" @{ value = $ModelTag } | Out-Null
}
function Trigger-Deploy([string]$ServiceId) {
    $result = Invoke-RenderApi "POST" "/services/$ServiceId/deploys" @{ clearCache = "do_not_clear" }
    if ($result.deploy) { return $result.deploy }
    return $result
}
function Test-ModelThroughProxy([string]$BaseUrl, [string]$ApiKey, [string]$ModelTag) {
    $payload = @{
        model = $ModelTag
        stream = $false
        messages = @(
            @{ role = "system"; content = "Ты Барни. Отвечай по-русски, не выдумывай данные и не проси пароли." },
            @{ role = "user"; content = "Кратко объясни, чем семинар отличается от лекции." }
        )
        options = @{ temperature = 0.1; num_predict = 64; num_ctx = 2048 }
    } | ConvertTo-Json -Depth 10 -Compress
    try {
        $answer = Invoke-RestMethod -Method POST -Uri ($BaseUrl.TrimEnd("/") + "/api/chat") -Headers @{ Authorization = "Bearer $ApiKey" } -ContentType "application/json" -Body $payload -TimeoutSec 150
        $text = [string]$answer.message.content
        if ([string]::IsNullOrWhiteSpace($text)) { throw "Model returned an empty answer." }
        return $text
    }
    catch {
        throw "Remote model preflight failed for '$ModelTag'. No Render configuration was changed. Details: $($_.Exception.Message)"
    }
}
function Wait-ForDeploy([string]$ServiceId, [string]$DeployId, [int]$MaxChecks = 72) {
    if ([string]::IsNullOrWhiteSpace($DeployId)) {
        Write-Warning "Render accepted the deploy request but did not return a deploy ID. Check the service Deploys page."
        return "unknown"
    }
    for ($i = 0; $i -lt $MaxChecks; $i++) {
        $deploy = Invoke-RenderApi "GET" "/services/$ServiceId/deploys/$DeployId"
        if ($deploy.deploy) { $deploy = $deploy.deploy }
        $status = [string]$deploy.status
        Write-Host ("Deploy status: " + $status)
        if ($status -eq "live") { return "live" }
        if ($status -in @("build_failed", "update_failed", "canceled", "pre_deploy_failed", "deactivated")) { return $status }
        Start-Sleep -Seconds 10
    }
    return "timeout"
}
function Test-BackendHealth([string]$ServiceUrl, [int]$MaxChecks = 30) {
    if ([string]::IsNullOrWhiteSpace($ServiceUrl)) { return $false }
    for ($i = 0; $i -lt $MaxChecks; $i++) {
        try {
            $health = Invoke-RestMethod -Uri ($ServiceUrl.TrimEnd("/") + "/health") -TimeoutSec 12
            if ($health.status -eq "ok") { return $true }
        }
        catch { }
        Start-Sleep -Seconds 10
    }
    return $false
}
function Save-State($State) {
    $State | ConvertTo-Json -Depth 10 | Set-Content -Path $StatePath -Encoding UTF8
}
function Restore-PreviousModel([string]$ServiceId, [string]$PreviousModel) {
    Write-Host "[ROLLBACK] Restoring previous OLLAMA_MODEL..." -ForegroundColor Yellow
    Set-ModelVariable $ServiceId $PreviousModel
    $deploy = Trigger-Deploy $ServiceId
    $deployId = [string]$deploy.id
    if ($deployId) { Write-Host "Rollback deploy queued: $deployId" }
    else { Write-Warning "Rollback deploy was triggered. Check Render dashboard." }
}

try {
    Write-Header "BARNEY-NANO / RENDER RELEASE MANAGER"

    if ($Mode -eq "promote") {
        $readiness = Read-JsonFile $ReadinessPath
        if ($readiness.status -ne "READY_FOR_PROMOTION") {
            throw "Readiness gate is '$($readiness.status)', not READY_FOR_PROMOTION. Run BAT options [9], [10] and [11] and fix every failed gate first."
        }
        $candidate = Read-JsonFile $CandidatePointer
        $manifestPath = Join-Path $Root ([string]$candidate.manifest)
        $manifest = Read-JsonFile $manifestPath
        if ($manifest.local_smoke_test_passed -ne $true -or $manifest.production_eligible -ne $true) {
            throw "Candidate did not pass its local smoke test or is marked pilot-only. Promotion blocked."
        }
        if ([string]$manifest.tag -notmatch "^barney-nano:rc[0-9]{14}$") {
            throw "Candidate tag is not a valid versioned BARNEY-NANO release tag."
        }
        $targetModel = [string]$manifest.tag
    }
    else {
        $saved = Read-JsonFile $StatePath
        if ([string]::IsNullOrWhiteSpace([string]$saved.previous_model)) {
            throw "No previous production model is recorded; rollback is unavailable."
        }
        $targetModel = [string]$saved.previous_model
    }

    $serviceId = Read-Host "Render Service ID (srv-...)"
    if ([string]::IsNullOrWhiteSpace($serviceId)) { throw "Service ID is required." }
    $script:RenderToken = Get-SecretText "Render API key (input is hidden)"
    if ([string]::IsNullOrWhiteSpace($script:RenderToken)) { throw "Render API key is required." }

    $service = Get-ServiceObject $serviceId
    if ($service.name -ne $ExpectedServiceName) {
        throw "Service name mismatch. Expected '$ExpectedServiceName', received '$($service.name)'. No changes made."
    }
    if ($service.type -ne "web_service") { throw "Target is not a Render web service. No changes made." }

    $envTable = Get-ServiceEnvTable $serviceId
    foreach ($requiredKey in @("OLLAMA_MODEL", "OLLAMA_BASE_URL", "OLLAMA_API_KEY")) {
        if (-not $envTable.ContainsKey($requiredKey) -or [string]::IsNullOrWhiteSpace($envTable[$requiredKey])) {
            throw "$requiredKey is missing from direct service environment variables. This manager will not rewrite linked environment groups or replace the complete variable list. Set/inspect these values in Render first."
        }
    }

    $currentModel = [string]$envTable["OLLAMA_MODEL"]
    $proxyUrl = [string]$envTable["OLLAMA_BASE_URL"]
    $proxyKey = [string]$envTable["OLLAMA_API_KEY"]
    if (-not $proxyUrl.StartsWith("https://", [StringComparison]::OrdinalIgnoreCase)) {
        throw "OLLAMA_BASE_URL is not HTTPS. Refusing to send credentials to a non-HTTPS endpoint."
    }

    if ($Mode -eq "promote") {
        if ($currentModel -eq $targetModel) {
            Write-Host "Render already points to $targetModel. No environment update is needed."
            exit 0
        }

        Write-Header "1/3 - REMOTE PRE-FLIGHT"
        Write-Host "Checking candidate through the same authenticated Ollama proxy used by the backend..."
        $sample = Test-ModelThroughProxy $proxyUrl $proxyKey $targetModel
        Write-Host "Remote model response received ($($sample.Length) characters)."
        Write-Host "[OK] Candidate is reachable through the configured remote model route." -ForegroundColor Green

        $state = [ordered]@{
            schema_version = 1
            service_id = $serviceId
            service_name = $ExpectedServiceName
            previous_model = $currentModel
            candidate_model = $targetModel
            prepared_at = (Get-Date).ToUniversalTime().ToString("o")
            status = "preflight_passed"
            deploy_id = $null
        }
        Save-State $state

        Write-Header "2/3 - EXPLICIT PRODUCTION SWITCH"
        Write-Host "Current model : $currentModel"
        Write-Host "New candidate : $targetModel"
        Write-Host "Only OLLAMA_MODEL will be updated. Other Render variables are untouched."
        $approval = Read-Host "To update the live Barney, type PROMOTE"
        if ($approval -cne "PROMOTE") {
            Write-Host "Cancelled. Production continues using $currentModel."
            exit 0
        }

        Set-ModelVariable $serviceId $targetModel
        $state.status = "environment_updated"
        Save-State $state

        Write-Header "3/3 - RENDER DEPLOY AND HEALTH CHECK"
        $deploy = Trigger-Deploy $serviceId
        $deployId = [string]$deploy.id
        $state.deploy_id = $deployId
        $state.status = "deploy_triggered"
        Save-State $state
        Write-Host "Render deploy ID: $deployId"

        $deployStatus = Wait-ForDeploy $serviceId $deployId
        if ($deployStatus -in @("build_failed", "update_failed", "canceled", "pre_deploy_failed", "deactivated")) {
            Write-Warning "Render deploy failed with status '$deployStatus'. Restoring previous model automatically."
            Restore-PreviousModel $serviceId $currentModel
            $state.status = "auto_rollback_triggered"
            Save-State $state
            throw "Promotion failed; automatic configuration rollback was triggered."
        }
        if ($deployStatus -eq "timeout" -or $deployStatus -eq "unknown") {
            $state.status = "deploy_status_unknown"
            Save-State $state
            Write-Warning "Deploy is not confirmed as live. The previous model name is saved for rollback. Do not delete either Ollama model."
            Write-Host "Check Render Deploys before deciding whether to roll back."
            exit 2
        }

        $serviceUrl = [string]$service.serviceDetails.url
        if (-not (Test-BackendHealth $serviceUrl)) {
            Write-Warning "Backend health check did not return status=ok. Restoring the previous model configuration."
            Restore-PreviousModel $serviceId $currentModel
            $state.status = "health_failed_rollback_triggered"
            Save-State $state
            throw "Backend health check failed; rollback deploy was triggered."
        }

        $state.status = "live"
        $state.activated_at = (Get-Date).ToUniversalTime().ToString("o")
        Save-State $state
        Write-Host ""
        Write-Host "[SUCCESS] BARNEY-NANO is now the configured model for the live backend." -ForegroundColor Green
        Write-Host "Previous model retained for rollback: $currentModel"
        Write-Host "Rollback command: BARNEY_NANO.bat -> [13]"
    }
    else {
        $saved = Read-JsonFile $StatePath
        if ($saved.service_id -ne $serviceId) { throw "Service ID differs from the saved release state. Rollback aborted." }
        if ($currentModel -eq $targetModel) {
            Write-Host "Render already points to the saved previous model '$targetModel'."
            exit 0
        }

        Write-Header "ROLLBACK PRE-FLIGHT"
        $null = Test-ModelThroughProxy $proxyUrl $proxyKey $targetModel
        Write-Host "[OK] Previous model is still reachable through the proxy."
        Write-Host "Current model : $currentModel"
        Write-Host "Restore model : $targetModel"
        $approval = Read-Host "To restore the previous production model, type ROLLBACK"
        if ($approval -cne "ROLLBACK") {
            Write-Host "Cancelled. Production configuration was not changed."
            exit 0
        }

        Set-ModelVariable $serviceId $targetModel
        $deploy = Trigger-Deploy $serviceId
        $deployId = [string]$deploy.id
        $saved.status = "rollback_deploy_triggered"
        $saved.rollback_deploy_id = $deployId
        $saved.rolled_back_at = (Get-Date).ToUniversalTime().ToString("o")
        Save-State $saved
        Write-Host "Rollback deploy queued: $deployId"
        $deployStatus = Wait-ForDeploy $serviceId $deployId
        if ($deployStatus -eq "live" -and (Test-BackendHealth ([string]$service.serviceDetails.url))) {
            $saved.status = "rolled_back"
            Save-State $saved
            Write-Host "[SUCCESS] Previous model restored and backend health check passed." -ForegroundColor Green
        } else {
            Write-Warning "Rollback was requested but a healthy live deployment was not confirmed. Check Render Deploys."
        }
    }
}
catch {
    Write-Host ""
    Write-Host "[ERROR] $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "No Ollama models were deleted. See training\barney-nano-render-deploy.json for the saved rollback target, if a switch had started."
    exit 1
}
finally {
    if ($script:RenderToken) { $script:RenderToken = $null }
}
