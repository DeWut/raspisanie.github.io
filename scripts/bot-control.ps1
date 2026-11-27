param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('start', 'stop', 'status', 'sync')]
    [string]$Action
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$backendDir = Join-Path $repoRoot 'backend'
$exePath = Join-Path $backendDir 'target\debug\smart-campus-backend.exe'
$pidPath = Join-Path $backendDir 'smart-campus-backend.pid'
$logPath = Join-Path $backendDir 'bot.log'
$errorLogPath = Join-Path $backendDir 'bot-error.log'
$envPath = Join-Path $backendDir '.env'

function Get-BackendPort {
    $port = 8080
    if (Test-Path -LiteralPath $envPath) {
        $line = Get-Content -LiteralPath $envPath | Where-Object { $_ -match '^\s*PORT\s*=' } | Select-Object -Last 1
        if ($line -and $line -match '^\s*PORT\s*=\s*"?(\d+)') {
            $port = [int]$Matches[1]
        }
    }
    return $port
}

function Get-ProjectProcesses {
    Get-CimInstance Win32_Process -Filter "Name = 'smart-campus-backend.exe'" |
        Where-Object { $_.ExecutablePath -and $_.ExecutablePath -ieq $exePath }
}

function Show-Status {
    $processes = @(Get-ProjectProcesses)
    $port = Get-BackendPort
    $listeners = @(Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction SilentlyContinue)
    if ($processes.Count -gt 0 -or $listeners.Count -gt 0) {
        Write-Host "  [ OK ] Backend is running (port $port)." -ForegroundColor Green
        foreach ($process in $processes) { Write-Host "         PID: $($process.ProcessId)" }
        if (Test-Path -LiteralPath $logPath) { Write-Host "         Log: $logPath" }
        return $true
    }
    Write-Host '  [ -- ] Backend is stopped.' -ForegroundColor Yellow
    return $false
}

switch ($Action) {
    'status' {
        [void](Show-Status)
    }
    'sync' {
        if (-not (Test-Path -LiteralPath $envPath)) { Write-Host "  [ERR] Missing settings file: $envPath" -ForegroundColor Red; exit 1 }
        & git rev-parse --is-inside-work-tree *> $null
        if ($LASTEXITCODE -ne 0) { Write-Host '  [ERR] This folder is not a Git repository.' -ForegroundColor Red; exit 1 }
        $branch = (& git branch --show-current).Trim()
        if (-not $branch) { Write-Host '  [ERR] Git is in detached HEAD state.' -ForegroundColor Red; exit 1 }
        & git diff --cached --quiet
        if ($LASTEXITCODE -ne 0) { Write-Host '  [ERR] Already staged changes exist. Commit or unstage them first.' -ForegroundColor Red; exit 1 }
        $remote = @(& git remote)
        if ($remote -notcontains 'origin') { Write-Host '  [ERR] Git remote origin is not configured.' -ForegroundColor Red; exit 1 }

        $syncOutputPath = Join-Path $env:TEMP ("spbftuit-sync-{0}.out" -f [guid]::NewGuid().ToString('N'))
        $syncErrorPath = Join-Path $env:TEMP ("spbftuit-sync-{0}.err" -f [guid]::NewGuid().ToString('N'))
        Write-Host ''
        Write-Host '  +--------------------------------------------------+' -ForegroundColor DarkCyan
        Write-Host '  |      SPBFTUIT  DATABASE + GITHUB SYNC            |' -ForegroundColor Cyan
        Write-Host '  +--------------------------------------------------+' -ForegroundColor DarkCyan
        Write-Host '  1/3  Syncing EIOS schedules into PostgreSQL...' -ForegroundColor Gray

        $syncExit = 0
        Push-Location $backendDir
        try {
            $syncProcess = Start-Process -FilePath 'cargo' -ArgumentList @('run', '--quiet', '--bin', 'sync_schedule') -WorkingDirectory $backendDir -NoNewWindow -PassThru -RedirectStandardOutput $syncOutputPath -RedirectStandardError $syncErrorPath
            $shownPercent = 0; $targetPercent = 0; $completedGroups = 0; $totalGroups = 0
            $spinner = @('|', '/', '-', '\'); $frame = 0
            while (-not $syncProcess.HasExited) {
                $lastProgress = Get-Content -LiteralPath $syncOutputPath -ErrorAction SilentlyContinue | Where-Object { $_ -match '^SYNC_PROGRESS=\d+/\d+$' } | Select-Object -Last 1
                if ($lastProgress -match '^SYNC_PROGRESS=(\d+)/(\d+)$') {
                    $completedGroups = [int]$Matches[1]; $totalGroups = [int]$Matches[2]
                    if ($totalGroups -gt 0) {
                        $actualPercent = [Math]::Floor(95 * $completedGroups / $totalGroups)
                        if ($completedGroups -lt $totalGroups) {
                            $nextPercent = [Math]::Floor(95 * ($completedGroups + 1) / $totalGroups)
                            $targetPercent = [Math]::Min(95, $actualPercent + [Math]::Max(1, [Math]::Floor(($nextPercent - $actualPercent) * 0.65)))
                        } else { $targetPercent = 95 }
                    }
                } elseif ($targetPercent -lt 3) { $targetPercent = 3 }
                if ($shownPercent -lt $targetPercent) { $shownPercent++ }
                $barWidth = 32; $filled = [Math]::Floor($shownPercent * $barWidth / 100)
                $bar = ('=' * $filled) + ('-' * ($barWidth - $filled))
                $spin = $spinner[$frame % $spinner.Count]
                $groupLabel = if ($totalGroups -gt 0) { "groups $completedGroups/$totalGroups" } else { 'connecting to Neon/PostgreSQL' }
                Write-Host (([char]13).ToString() + "  [{0}] {1,3}%  {2}  {3}" -f $bar,$shownPercent,$groupLabel,$spin) -NoNewline -ForegroundColor Cyan
                $frame++; Start-Sleep -Milliseconds 120; $syncProcess.Refresh()
            }
            $syncProcess.WaitForExit(); $syncExit = $syncProcess.ExitCode
            if ($syncExit -eq 0) { Write-Host (([char]13).ToString() + "  [{0}] 100%  database synchronization complete       " -f ('=' * 32)) -ForegroundColor Green }
            else { Write-Host ''; Write-Host "  [ERR] Database sync failed (exit code $syncExit). GitHub files were not changed." -ForegroundColor Red }
        } finally { Pop-Location }

        if (Test-Path -LiteralPath $syncOutputPath) {
            Get-Content -LiteralPath $syncOutputPath | Where-Object { $_ -and $_ -notmatch '^SYNC_PROGRESS=' } | ForEach-Object { Write-Host "  $_" }
            Remove-Item -LiteralPath $syncOutputPath -Force
        }
        if (Test-Path -LiteralPath $syncErrorPath) {
            Get-Content -LiteralPath $syncErrorPath | ForEach-Object { Write-Host "  $_" -ForegroundColor DarkYellow }
            Remove-Item -LiteralPath $syncErrorPath -Force
        }
        if ($syncExit -ne 0) { exit $syncExit }

        Write-Host ''
        Write-Host '  2/3  Downloading static timetable files for the website...' -ForegroundColor Cyan
        $python = Get-Command python -ErrorAction SilentlyContinue
        if (-not $python) { Write-Host '  [ERR] Python 3 is not in PATH. PostgreSQL was synced; GitHub was not updated.' -ForegroundColor Red; exit 1 }
        Push-Location $repoRoot
        try { & $python.Source 'update_schedule.py' '--delay' '1'; $scrapeExit = $LASTEXITCODE }
        finally { Pop-Location }
        if ($scrapeExit -ne 0 -and $scrapeExit -ne 2) {
            Write-Host "  [ERR] Website export failed (exit code $scrapeExit). Database is synced; GitHub was not pushed." -ForegroundColor Red
            exit $scrapeExit
        }
        $metaPath = Join-Path $repoRoot 'data\schedule-meta.json'
        if (-not (Test-Path -LiteralPath $metaPath)) { Write-Host '  [ERR] No successful schedule export metadata. GitHub was not pushed.' -ForegroundColor Red; exit 1 }
        $meta = Get-Content -LiteralPath $metaPath -Raw | ConvertFrom-Json
        if ([int]$meta.groups_updated -lt 1) { Write-Host '  [ERR] No groups updated; old public timestamp was preserved.' -ForegroundColor Red; exit 1 }

        Write-Host ''
        Write-Host '  3/3  Publishing schedule files and timestamp to GitHub...' -ForegroundColor Cyan
        $scheduleFiles = @('data/groups.json', 'data/schedule-meta.json')
        $scheduleFiles += Get-ChildItem -LiteralPath (Join-Path $repoRoot 'data') -File -Filter '*.json' | Where-Object { $_.BaseName -match '^\d+$' } | ForEach-Object { 'data/' + $_.Name }
        & git add -- $scheduleFiles
        if ($LASTEXITCODE -ne 0) { Write-Host '  [ERR] Could not stage schedule files.' -ForegroundColor Red; exit 1 }
        & git diff --cached --quiet
        if ($LASTEXITCODE -ne 0) {
            $stamp = [DateTimeOffset]::Now.ToString('yyyy-MM-dd HH:mm:ss zzz')
            & git commit -m "Обновление расписания $stamp"
            if ($LASTEXITCODE -ne 0) { Write-Host '  [ERR] Git commit failed; schedule files remain staged locally.' -ForegroundColor Red; exit 1 }
        } else { Write-Host '  [-- ] Schedule data unchanged; no commit needed.' -ForegroundColor Yellow }

        & git push origin $branch
        if ($LASTEXITCODE -ne 0) { Write-Host "  [ERR] Push failed. Local commit is preserved on '$branch'; push it after fixing Git access." -ForegroundColor Red; exit 1 }
        Write-Host ''
        Write-Host "  [ OK ] PostgreSQL synchronized and website data pushed to origin/$branch." -ForegroundColor Green
        Write-Host "         Groups updated: $($meta.groups_updated)/$($meta.groups_total)"
        Write-Host "         Public timestamp: $($meta.updated_at)"
        if ([int]$meta.groups_failed -gt 0 -or $scrapeExit -eq 2) {
            Write-Host "  [WARN] Some groups failed ($($meta.groups_failed)); successful groups published, old files retained for failed groups." -ForegroundColor Yellow
            exit 2
        }
    }
    'start' {
        if (Show-Status) { exit 0 }
        if (-not (Test-Path -LiteralPath $envPath)) {
            Write-Host "  [ERR] Missing settings file: $envPath" -ForegroundColor Red
            Write-Host '        Copy backend\.env.example to backend\.env and configure it.'
            exit 1
        }

        Write-Host '  [.. ] Building backend...' -ForegroundColor Cyan
        Push-Location $backendDir
        try {
            & cargo build --bin smart-campus-backend
            if ($LASTEXITCODE -ne 0) { throw "Cargo build failed (exit code $LASTEXITCODE)." }
        }
        finally {
            Pop-Location
        }

        if (-not (Test-Path -LiteralPath $exePath)) {
            throw "Backend executable was not created: $exePath"
        }

        if (Test-Path -LiteralPath $logPath) { Remove-Item -LiteralPath $logPath -Force }
        if (Test-Path -LiteralPath $errorLogPath) { Remove-Item -LiteralPath $errorLogPath -Force }
        $process = Start-Process -FilePath $exePath -WorkingDirectory $backendDir -WindowStyle Hidden -PassThru `
            -RedirectStandardOutput $logPath -RedirectStandardError $errorLogPath
        Set-Content -LiteralPath $pidPath -Value $process.Id -Encoding ASCII

        Start-Sleep -Seconds 3
        $process.Refresh()
        if ($process.HasExited) {
            Remove-Item -LiteralPath $pidPath -Force -ErrorAction SilentlyContinue
            Write-Host "  [ERR] Backend exited during startup. Check: $logPath" -ForegroundColor Red
            exit 1
        }
        Write-Host "  [ OK ] Backend started (PID $($process.Id))." -ForegroundColor Green
        Write-Host "         Log: $logPath"
        Write-Host "         Errors: $errorLogPath"
        Write-Host '         Telegram polling starts when the database and bot settings are ready.'
    }
    'stop' {
        $processes = @(Get-ProjectProcesses)
        if ($processes.Count -eq 0) {
            Remove-Item -LiteralPath $pidPath -Force -ErrorAction SilentlyContinue
            Write-Host '  [ -- ] No project backend process is running.' -ForegroundColor Yellow
            exit 0
        }

        foreach ($process in $processes) {
            Stop-Process -Id $process.ProcessId -Force
            Write-Host "  [ OK ] Stopped backend (PID $($process.ProcessId))." -ForegroundColor Green
        }
        Remove-Item -LiteralPath $pidPath -Force -ErrorAction SilentlyContinue
    }
}
