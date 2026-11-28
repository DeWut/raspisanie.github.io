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
function Write-Skip([string]$Message) { Write-Host "[SKIP] $Message — already installed and up to date." -ForegroundColor DarkGray }
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
function Invoke-WingetUpgradeIfAvailable([string]$PackageId, [string]$DisplayName) {
    Write-Host "Checking $DisplayName for updates..."
    $output = & $script:Winget.Source upgrade --id $PackageId --exact --silent --accept-package-agreements --accept-source-agreements 2>&1
    $code = $LASTEXITCODE
    $output | ForEach-Object { Write-Host $_ }
    if ($code -eq 0) {
        Write-Ok "$DisplayName update check completed (winget skips packages with no applicable update)."
        return $true
    }
    Write-Warn "$DisplayName update check/install returned exit code $code. Review winget output above."
    return $false
}
function Test-PythonRequirementSet([string]$Python, [string]$RequirementsFile) {
    $code = @'
from importlib.metadata import version, PackageNotFoundError
from packaging.requirements import Requirement
from packaging.utils import canonicalize_name
import sys
bad = []
for raw in open(sys.argv[1], encoding="utf-8"):
    line = raw.strip()
    if not line or line.startswith("#") or line.startswith("-"):
        continue
    req = Requirement(line)
    if req.marker and not req.marker.evaluate():
        continue
    try:
        installed = version(canonicalize_name(req.name))
    except PackageNotFoundError:
        bad.append(req.name + " (missing)")
        continue
    if req.specifier and not req.specifier.contains(installed, prereleases=True):
        bad.append(req.name + " (" + installed + ", need " + str(req.specifier) + ")")
print("\n".join(bad))
sys.exit(1 if bad else 0)
'@
    $result = & $Python -c $code $RequirementsFile 2>&1
    $exitCode = $LASTEXITCODE
    if ($result) { $result | ForEach-Object { Write-Host $_ } }
    return ($exitCode -eq 0)
}

try {
    Write-Step "BARNEY-NANO SMART SETUP"
    if (-not [Environment]::Is64BitOperatingSystem) {
        throw "BARNEY-NANO requires 64-bit Windows."
    }
    Write-Host "Project: $Root"
    Write-Host "Log: $LogFile"

    $script:Winget = Get-Command winget.exe -ErrorAction SilentlyContinue
    if (-not $script:Winget) {
        Refresh-Path
        $script:Winget = Get-Command winget.exe -ErrorAction SilentlyContinue
    }
    if (-not $script:Winget) {
        throw "Windows Package Manager (winget) is missing. Install/update App Installer from Microsoft Store, then run this installer again."
    }

    Write-Step "1/6 - Python 3.11"
    $pythonExe = Find-Python311
    if (-not $pythonExe) {
        Write-Host "Python 3.11 is missing. Installing it..."
        & $script:Winget.Source install --id Python.Python.3.11 --exact --scope user --silent --accept-package-agreements --accept-source-agreements
        if ($LASTEXITCODE -ne 0) { throw "Python installation failed (winget exit code $LASTEXITCODE)." }
        Refresh-Path
        $pythonExe = Find-Python311
    } else {
        Write-Host "Found Python: $pythonExe"
        Invoke-WingetUpgradeIfAvailable "Python.Python.3.11" "Python 3.11" | Out-Null
        Refresh-Path
        $pythonExe = Find-Python311
    }
    if (-not $pythonExe) {
        throw "Python 3.11 was not found after install/update. Check the install log."
    }
    $pythonVersion = (& $pythonExe --version 2>&1 | Out-String).Trim()
    Write-Ok "$pythonVersion ($pythonExe)"

    Write-Step "2/6 - Ollama"
    Refresh-Path
    $ollamaCommand = Get-Command ollama.exe -ErrorAction SilentlyContinue
    $ollamaExe = if ($ollamaCommand) { $ollamaCommand.Source } else { $null }
    if (-not $ollamaExe) {
        $knownOllama = Join-Path $env:LOCALAPPDATA "Programs\Ollama\ollama.exe"
        if (Test-Path $knownOllama) { $ollamaExe = $knownOllama }
    }
    if (-not $ollamaExe) {
        Write-Host "Ollama is missing. Installing it..."
        & $script:Winget.Source install --id Ollama.Ollama --exact --silent --accept-package-agreements --accept-source-agreements
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
    } else {
        Invoke-WingetUpgradeIfAvailable "Ollama.Ollama" "Ollama" | Out-Null
        Refresh-Path
        $ollamaCommand = Get-Command ollama.exe -ErrorAction SilentlyContinue
        if ($ollamaCommand) { $ollamaExe = $ollamaCommand.Source }
    }
    if ($ollamaExe) {
        $ollamaVersion = (& $ollamaExe --version 2>&1 | Out-String).Trim()
        Write-Ok "$ollamaVersion"
    } else {
        Write-Warn "Ollama is not installed. Teacher generation will remain unavailable."
    }

    Write-Step "3/6 - Python virtual environment"
    $venv = Join-Path $Root ".venv-barney-nano"
    $python = Join-Path $venv "Scripts\python.exe"
    if (-not (Test-Path $python)) {
        Write-Host "Creating .venv-barney-nano..."
        & $pythonExe -m venv $venv
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path $python)) {
            throw "Could not create the .venv-barney-nano environment."
        }
    } else {
        Write-Skip "Python virtual environment"
    }
    $pipVersion = (& $python -m pip --version 2>&1 | Out-String).Trim()
    Write-Ok $pipVersion

    Write-Step "4/6 - PyTorch CUDA and project libraries"
    $torchCuda = (& $python -c "import importlib.util; s=importlib.util.find_spec('torch'); print('missing' if s is None else __import__('torch').version.cuda or 'cpu')" 2>$null | Out-String).Trim()
    if ($torchCuda -eq "12.8") {
        Write-Host "Detected PyTorch CUDA 12.8. Checking whether a newer compatible build exists..."
        $torchPlan = & $python -m pip install --upgrade --dry-run torch torchvision --index-url https://download.pytorch.org/whl/cu128 2>&1
        $torchCode = $LASTEXITCODE
        $torchPlan | ForEach-Object { Write-Host $_ }
        if ($torchCode -ne 0) { throw "PyTorch update check failed. See pip output above." }
        if (($torchPlan -join [Environment]::NewLine) -match "(?i)Would install") {
            Write-Host "A newer compatible PyTorch build is available. Updating..."
            & $python -m pip install --upgrade torch torchvision --index-url https://download.pytorch.org/whl/cu128
            if ($LASTEXITCODE -ne 0) { throw "PyTorch CUDA update failed." }
        } else {
            Write-Skip "PyTorch CUDA 12.8"
        }
    } else {
        Write-Host "PyTorch CUDA 12.8 is missing or the installed build uses CUDA '$torchCuda'. Installing the required build..."
        & $python -m pip install --upgrade torch torchvision --index-url https://download.pytorch.org/whl/cu128
        if ($LASTEXITCODE -ne 0) {
            throw "PyTorch CUDA installation failed. Official selector: https://pytorch.org/get-started/locally/"
        }
    }

    $requirements = Join-Path $Root "scripts\barney_nano\requirements.txt"
    if (-not (Test-Path $requirements)) { throw "requirements.txt not found: $requirements" }
    if (Test-PythonRequirementSet $python $requirements) {
        Write-Skip "BARNEY-NANO Python dependencies"
    } else {
        Write-Host "Missing or outdated Python packages detected. Installing only required changes..."
        & $python -m pip install -r $requirements
        if ($LASTEXITCODE -ne 0) { throw "BARNEY-NANO Python dependencies installation failed." }
    }

    $cudaResult = & $python -c "import torch; print('torch=' + torch.__version__); print('cuda=' + str(torch.cuda.is_available())); print('gpu=' + (torch.cuda.get_device_name(0) if torch.cuda.is_available() else 'NOT DETECTED'))"
    $cudaResult | ForEach-Object { Write-Host $_ }
    $cudaReady = ((& $python -c "import torch; print('1' if torch.cuda.is_available() else '0')") -join "") -eq "1"
    if ($cudaReady) {
        Write-Ok "PyTorch can access the NVIDIA GPU."
    } else {
        Write-Warn "PyTorch is installed, but CUDA cannot see the GPU. Training is not ready. Update/restart the NVIDIA driver and rerun setup."
    }

    Write-Step "5/6 - Student model cache"
    $cacheCheck = @'
from huggingface_hub import snapshot_download
try:
    path = snapshot_download(repo_id="HuggingFaceTB/SmolLM2-360M-Instruct", local_files_only=True)
    print(path)
except Exception:
    raise SystemExit(1)
'@
    $cachedPath = & $python -c $cacheCheck 2>$null
    if ($LASTEXITCODE -eq 0 -and $cachedPath) {
        Write-Skip "SmolLM2-360M-Instruct model files"
        Write-Host "Cache: $cachedPath"
    } else {
        Write-Host "Student model is not fully cached. Downloading..."
        & $python -c "from huggingface_hub import snapshot_download; print(snapshot_download(repo_id='HuggingFaceTB/SmolLM2-360M-Instruct'))"
        if ($LASTEXITCODE -ne 0) {
            Write-Warn "Student model download failed. It will be downloaded on first training/evaluation when network access is available."
        } else {
            Write-Ok "Student model downloaded."
        }
    }

    Write-Step "6/6 - Ollama service and LFM3 model"
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
            $tags = Invoke-RestMethod -Uri "http://127.0.0.1:11434/api/tags" -TimeoutSec 10
            $lfmInstalled = @($tags.models | Where-Object { $_.name -match '^lfm3(:|$)' }).Count -gt 0
            if ($lfmInstalled) {
                Write-Host "LFM3 is already present. Checking for an updated model revision..."
            } else {
                Write-Host "LFM3 is not installed. Downloading..."
            }
            & $ollamaExe pull lfm3
            if ($LASTEXITCODE -eq 0) {
                if ($lfmInstalled) { Write-Ok "LFM3 checked; Ollama reports the model is current." }
                else { Write-Ok "LFM3 downloaded." }
            } else {
                Write-Warn "Ollama could not pull lfm3 on this Windows setup. This may be a platform/model availability limitation; no substitute model was installed."
            }
        } else {
            Write-Warn "Ollama API did not start. Launch Ollama or reboot, then rerun option 1."
        }
    }

    New-Item -ItemType Directory -Force -Path (Join-Path $Root "data\barney_nano") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $Root "training") | Out-Null

    Write-Step "SETUP SUMMARY"
    Write-Ok "Smart installation/update checks completed."
    Write-Host "Installer log: $LogFile"
    Write-Host "Next: run menu option [2] for diagnostics."
    if ($script:Warnings.Count -gt 0) {
        Write-Host ""
        Write-Host "Warnings to review:" -ForegroundColor Yellow
        foreach ($warning in $script:Warnings) { Write-Host " - $warning" -ForegroundColor Yellow }
    } else {
        Write-Host "All checked components are ready or already up to date." -ForegroundColor Green
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
