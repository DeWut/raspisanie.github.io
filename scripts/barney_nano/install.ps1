param(
    [Parameter(Mandatory = $true)]
    [string]$Root
)

$ErrorActionPreference = "Stop"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$Root = [System.IO.Path]::GetFullPath($Root)
$LogDir = Join-Path $Root "logs"
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$LogFile = Join-Path $LogDir ("barney-nano-install-" + (Get-Date -Format "yyyyMMdd-HHmmss") + ".log")
Start-Transcript -Path $LogFile -Force | Out-Null

$script:Warnings = [System.Collections.Generic.List[string]]::new()
function Write-Step([string]$Message) {
    Write-Host ""
    Write-Host "============================================================" -ForegroundColor Cyan
    Write-Host " $Message" -ForegroundColor Cyan
    Write-Host "============================================================" -ForegroundColor Cyan
}
function Write-Ok([string]$Message) { Write-Host "[ OK ] $Message" -ForegroundColor Green }
function Write-Warn([string]$Message) {
    Write-Host "[WARN] $Message" -ForegroundColor Yellow
    $script:Warnings.Add($Message)
}
function Refresh-Path {
    $machine = [Environment]::GetEnvironmentVariable("Path", "Machine")
    $user = [Environment]::GetEnvironmentVariable("Path", "User")
    $env:Path = "$machine;$user;$env:Path"
}
function Find-Python311 {
    $launcher = Get-Command py.exe -ErrorAction SilentlyContinue
    if ($launcher) {
        $found = (& $launcher.Source -3.11 -c "import sys; print(sys.executable)" 2>$null | Select-Object -First 1)
        if ($found -and (Test-Path $found.Trim())) { return $found.Trim() }
    }
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA "Programs\Python\Python311\python.exe"),
        (Join-Path $env:ProgramFiles "Python311\python.exe"),
        (Join-Path $env:ProgramFiles "Python 3.11\python.exe")
    )
    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path $candidate)) { return $candidate }
    }
    return $null
}

try {
    Write-Step "BARNEY-NANO AUTOMATIC SETUP"
    if (-not [Environment]::Is64BitOperatingSystem) {
        throw "BARNEY-NANO requires 64-bit Windows."
    }
    Write-Host "Project: $Root"
    Write-Host "Log: $LogFile"

    $winget = Get-Command winget.exe -ErrorAction SilentlyContinue
    if (-not $winget) {
        Refresh-Path
        $winget = Get-Command winget.exe -ErrorAction SilentlyContinue
    }
    if (-not $winget) {
        throw "Windows Package Manager (winget) is missing. Install/update App Installer from Microsoft Store, then run this installer again."
    }

    Write-Step "1/6 - Python 3.11"
    $pythonExe = Find-Python311
    if (-not $pythonExe) {
        Write-Host "Installing Python 3.11 x64 through winget..."
        & $winget.Source install --id Python.Python.3.11 --exact --scope user --silent --accept-package-agreements --accept-source-agreements
        if ($LASTEXITCODE -ne 0) {
            Write-Warn "winget could not install Python automatically (exit code $LASTEXITCODE)."
        }
        Refresh-Path
        $pythonExe = Find-Python311
    }
    if (-not $pythonExe) {
        throw "Python 3.11 was not found after installation. Check the install log and winget output."
    }
    Write-Ok "Python: $pythonExe"

    Write-Step "2/6 - Ollama"
    Refresh-Path
    $ollamaCommand = Get-Command ollama.exe -ErrorAction SilentlyContinue
    $ollamaExe = if ($ollamaCommand) { $ollamaCommand.Source } else { $null }
    if (-not $ollamaExe) {
        $knownOllama = Join-Path $env:LOCALAPPDATA "Programs\Ollama\ollama.exe"
        if (Test-Path $knownOllama) { $ollamaExe = $knownOllama }
    }
    if (-not $ollamaExe) {
        Write-Host "Installing Ollama through winget..."
        & $winget.Source install --id Ollama.Ollama --exact --silent --accept-package-agreements --accept-source-agreements
        if ($LASTEXITCODE -ne 0) {
            Write-Warn "Ollama installation returned exit code $LASTEXITCODE."
        }
        Refresh-Path
        $ollamaCommand = Get-Command ollama.exe -ErrorAction SilentlyContinue
        if ($ollamaCommand) { $ollamaExe = $ollamaCommand.Source }
        if (-not $ollamaExe) {
            $knownOllama = Join-Path $env:LOCALAPPDATA "Programs\Ollama\ollama.exe"
            if (Test-Path $knownOllama) { $ollamaExe = $knownOllama }
        }
    }
    if ($ollamaExe) {
        Write-Ok "Ollama: $ollamaExe"
    } else {
        Write-Warn "Ollama is not installed. Teacher generation will remain unavailable until Ollama is installed."
    }

    Write-Step "3/6 - Python virtual environment"
    $venv = Join-Path $Root ".venv-barney-nano"
    $python = Join-Path $venv "Scripts\python.exe"
    if (-not (Test-Path $python)) {
        & $pythonExe -m venv $venv
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path $python)) {
            throw "Could not create the .venv-barney-nano environment."
        }
    }
    & $python -m pip install --upgrade pip
    if ($LASTEXITCODE -ne 0) { throw "pip upgrade failed." }
    Write-Ok "Virtual environment ready."

    Write-Step "4/6 - PyTorch CUDA and project libraries"
    Write-Host "Installing/updating PyTorch CUDA 12.8 wheels..."
    & $python -m pip install --upgrade torch torchvision --index-url https://download.pytorch.org/whl/cu128
    if ($LASTEXITCODE -ne 0) {
        throw "PyTorch CUDA installation failed. See the log. Official selector: https://pytorch.org/get-started/locally/"
    }
    $requirements = Join-Path $Root "scripts\barney_nano\requirements.txt"
    if (-not (Test-Path $requirements)) { throw "requirements.txt not found: $requirements" }
    & $python -m pip install -r $requirements
    if ($LASTEXITCODE -ne 0) { throw "BARNEY-NANO Python dependencies installation failed." }

    $cudaResult = & $python -c "import torch; print('torch=' + torch.__version__); print('cuda=' + str(torch.cuda.is_available())); print('gpu=' + (torch.cuda.get_device_name(0) if torch.cuda.is_available() else 'NOT DETECTED'))"
    $cudaResult | ForEach-Object { Write-Host $_ }
    $cudaReady = ((& $python -c "import torch; print('1' if torch.cuda.is_available() else '0')") -join "") -eq "1"
    if ($cudaReady) {
        Write-Ok "PyTorch can access the NVIDIA GPU."
    } else {
        Write-Warn "PyTorch installed, but CUDA cannot see the GPU. Training is not ready. Update/restart the NVIDIA driver and rerun option 1."
    }

    Write-Step "5/6 - Download student model"
    Write-Host "Downloading HuggingFaceTB/SmolLM2-360M-Instruct into the Hugging Face cache..."
    & $python -c "from huggingface_hub import snapshot_download; print(snapshot_download(repo_id='HuggingFaceTB/SmolLM2-360M-Instruct'))"
    if ($LASTEXITCODE -ne 0) {
        Write-Warn "Student model download failed. It will be downloaded on first training/evaluation when network access is available."
    } else {
        Write-Ok "Student model is cached locally."
    }

    Write-Step "6/6 - Ollama service and teacher model"
    if ($ollamaExe) {
        $apiReady = $false
        try {
            Invoke-RestMethod -Uri "http://127.0.0.1:11434/api/tags" -TimeoutSec 3 | Out-Null
            $apiReady = $true
        } catch { }
        if (-not $apiReady) {
            Write-Host "Starting Ollama background server..."
            Start-Process -FilePath $ollamaExe -ArgumentList "serve" -WindowStyle Hidden
            for ($i = 0; $i -lt 20; $i++) {
                Start-Sleep -Seconds 2
                try {
                    Invoke-RestMethod -Uri "http://127.0.0.1:11434/api/tags" -TimeoutSec 3 | Out-Null
                    $apiReady = $true
                    break
                } catch { }
            }
        }
        if ($apiReady) {
            Write-Ok "Ollama API is responding."
            Write-Host "Attempting to download LFM3. This may take a while..."
            & $ollamaExe pull lfm3
            if ($LASTEXITCODE -eq 0) {
                Write-Ok "LFM3 download completed."
            } else {
                Write-Warn "Ollama could not pull lfm3 on this Windows setup. This may be a platform/model availability limitation; no substitute model was installed."
            }
        } else {
            Write-Warn "Ollama API did not start. Reboot or launch Ollama once, then rerun option 1."
        }
    }

    New-Item -ItemType Directory -Force -Path (Join-Path $Root "data\barney_nano") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $Root "training") | Out-Null

    Write-Step "SETUP SUMMARY"
    Write-Ok "Python environment and project packages are installed."
    Write-Host ""
    Write-Host "Installer log: $LogFile"
    Write-Host "Next: run menu option [2] for diagnostics."
    if ($script:Warnings.Count -gt 0) {
        Write-Host ""
        Write-Host "Warnings to review:" -ForegroundColor Yellow
        foreach ($warning in $script:Warnings) { Write-Host " - $warning" -ForegroundColor Yellow }
    } else {
        Write-Host "No setup warnings were reported." -ForegroundColor Green
    }
    Write-Host ""
    Write-Host "NVIDIA display drivers are not silently replaced by this script."
    Write-Host "A driver update may require Windows approval and a reboot."
}
catch {
    Write-Host ""
    Write-Host "[FATAL] $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "Read the installer log: $LogFile" -ForegroundColor Yellow
    try { Stop-Transcript | Out-Null } catch { }
    exit 1
}
finally {
    try { Stop-Transcript | Out-Null } catch { }
}
