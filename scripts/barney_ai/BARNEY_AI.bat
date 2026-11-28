@echo off
setlocal EnableExtensions
chcp 65001 >nul
title BARNEY AI - Local Training Console
color 0B
set "ROOT=%~dp0..\.."
for %%I in ("%ROOT%") do set "ROOT=%%~fI"
set "VENV=%ROOT%\.venv-barney"
set "PY=%VENV%\Scripts\python.exe"
set "TRAIN=%ROOT%\scripts\barney_ai\train_lfm3_lora.py"
set "PREP=%ROOT%\scripts\barney_ai\prepare_dataset.py"
set "DATA=%ROOT%\data\barney_training\generated_sft.jsonl"
set "LLAMA=%ROOT%\tools\llama.cpp"
cd /d "%ROOT%"

:MENU
cls
echo.
echo  ================================================================
echo       ____    _    ____  _   _ _____ __   __
echo      ^| __ )  / \  ^|  _ \| \ ^| ^| ____^|\ \ / /
echo      ^|  _ \ / _ \ ^| ^|_) ^|  \^| ^|  _^|   \ V /
echo      ^| ^|_) / ___ \^|  _ ^/^| ^|\  ^| ^|___   ^| ^|
echo      ^|____/_/   \_\_^| \_\_^| \_^|_____^|  ^|_^|
echo.
echo                  B A R N E Y   A I
echo             LOCAL LFM3 TRAINING CONSOLE
echo  ----------------------------------------------------------------
echo   Laptop profile: RTX 4050 / 36 GB RAM
echo   Training mode : LoRA / supervised fine-tuning
echo  ----------------------------------------------------------------
echo.
echo    [1]  First-time setup (Python environment)
echo    [2]  Hardware and dependencies check
echo    [3]  Prepare training dataset
echo    [4]  TRAIN BARNEY (LoRA)
echo    [5]  Talk to local LFM3 in Ollama
echo    [6]  Export trained model to Ollama
echo    [7]  Open training folder
echo    [0]  Exit
echo.
set /p "CHOICE=   Select option ^> "
if "%CHOICE%"=="1" goto SETUP
if "%CHOICE%"=="2" goto CHECK
if "%CHOICE%"=="3" goto PREPARE
if "%CHOICE%"=="4" goto TRAINING
if "%CHOICE%"=="5" goto CHAT
if "%CHOICE%"=="6" goto EXPORT
if "%CHOICE%"=="7" start "" "%ROOT%\training" & goto MENU
if "%CHOICE%"=="0" exit /b
goto MENU

:SETUP
cls
echo [BARNEY AI] Creating Python 3.11 environment...
where py >nul 2>nul || (echo Python launcher not found. Install Python 3.11 x64 first.& pause & goto MENU)
if not exist "%VENV%\Scripts\python.exe" py -3.11 -m venv "%VENV%"
if not exist "%PY%" (echo Could not create venv. Check Python 3.11 installation.& pause & goto MENU)
call "%VENV%\Scripts\activate.bat"
python -m pip install --upgrade pip
echo.
python -c "import torch; print('PyTorch:',torch.__version__); print('CUDA:',torch.cuda.is_available())" 2>nul
if errorlevel 1 (
  echo.
  echo PyTorch with CUDA is not installed in this venv.
  echo Open https://pytorch.org/get-started/locally/
  echo Select Windows ^> Pip ^> Python ^> CUDA build, then run its command
  echo with this venv activated. Reopen this menu and choose [1] again.
  pause
  goto MENU
)
pip install -r "%ROOT%\scripts\barney_ai\requirements-training.txt"
if errorlevel 1 (echo Dependency installation failed.& pause & goto MENU)
echo.
echo [OK] Environment is ready.
pause
goto MENU

:CHECK
cls
echo [BARNEY AI] Hardware and dependency check
echo.
where nvidia-smi >nul 2>nul
if errorlevel 1 (echo [WARN] nvidia-smi not found. Install/update NVIDIA driver.) else nvidia-smi
echo.
if exist "%PY%" (
  "%PY%" -c "import torch; print('PyTorch:',torch.__version__); print('CUDA available:',torch.cuda.is_available()); print('GPU:',torch.cuda.get_device_name(0) if torch.cuda.is_available() else 'not detected'); print('VRAM GB:',round(torch.cuda.get_device_properties(0).total_memory/1024**3,2) if torch.cuda.is_available() else 0); print('BF16:',torch.cuda.is_bf16_supported() if torch.cuda.is_available() else False)"
) else echo [WARN] Run option 1 first.
echo.
where ollama >nul 2>nul && ollama list || echo [WARN] Ollama is not installed or not in PATH.
pause
goto MENU

:PREPARE
cls
if not exist "%PY%" (echo Run option 1 first.& pause & goto MENU)
echo [BARNEY AI] Building reviewed dataset...
"%PY%" "%PREP%" --output "%DATA%"
if errorlevel 1 (echo [ERROR] Dataset preparation failed.& pause & goto MENU)
echo.
echo Dataset ready: %DATA%
pause
goto MENU

:TRAINING
cls
if not exist "%PY%" (echo Run option 1 first.& pause & goto MENU)
if not exist "%DATA%" (
  echo Dataset not found. Preparing it now...
  "%PY%" "%PREP%" --output "%DATA%"
  if errorlevel 1 (pause & goto MENU)
)
echo.
echo Training profile for RTX 4050 laptop:
echo   epochs=2, batch=1, grad_accum=16
echo   max_length=512, LoRA rank=8
echo.
echo NOTE: close games and GPU-heavy apps before starting.
echo This can take a while. Do not close this window.
echo.
set /p "CONFIRM=Start training now? [Y/N] "
if /I not "%CONFIRM%"=="Y" goto MENU
"%PY%" "%TRAIN%" --dataset "%DATA%" --epochs 2 --batch-size 1 --grad-accum 16 --max-length 512 --lora-rank 8 --merge
if errorlevel 1 (
  echo.
  echo [ERROR] Training failed. If CUDA out-of-memory, reduce max-length to 256
  echo and batch size to 1. Check the full error above.
) else (
  echo.
  echo [OK] LoRA adapter saved in training\barney-lfm3-lora
  echo Merged checkpoint is also saved if merge was enabled in the script.
)
pause
goto MENU

:CHAT
cls
where ollama >nul 2>nul
if errorlevel 1 (echo Ollama is not installed or not in PATH.& pause & goto MENU)
echo Starting local LFM3. Type /bye or press Ctrl+C to exit.
echo.
ollama run lfm3
pause
goto MENU

:EXPORT
cls
if not exist "%ROOT%\training\barney-lfm3-lora-merged\config.json" (
  echo Merged checkpoint not found.
  echo Run training with --merge first, or edit the train command in this BAT.
  pause
  goto MENU
)
if not exist "%LLAMA%\convert_hf_to_gguf.py" (
  echo Downloading llama.cpp converter...
  where git >nul 2>nul || (echo Git is required. Install Git for Windows first.& pause & goto MENU)
  git clone https://github.com/ggml-org/llama.cpp "%LLAMA%"
  if errorlevel 1 (echo [ERROR] Could not download llama.cpp.& pause & goto MENU)
)
call "%VENV%\Scripts\activate.bat"
pip install -r "%LLAMA%\requirements\requirements-convert_hf_to_gguf.txt"
if errorlevel 1 (echo [ERROR] Could not install GGUF conversion dependencies.& pause & goto MENU)
if not exist "%ROOT%\training" mkdir "%ROOT%\training"
echo Converting merged checkpoint to GGUF...
"%PY%" "%LLAMA%\convert_hf_to_gguf.py" "%ROOT%\training\barney-lfm3-lora-merged" --outfile "%ROOT%\training\barney-lfm3.gguf" --outtype q8_0
if errorlevel 1 (echo [ERROR] Conversion failed. Update llama.cpp and its conversion requirements.& pause & goto MENU)
(
echo FROM ./barney-lfm3.gguf
echo PARAMETER temperature 0.35
echo PARAMETER top_p 0.9
echo PARAMETER repeat_penalty 1.1
echo SYSTEM Ты Барни — спокойный и доброжелательный учебный помощник СПбГЛТУ. Отвечай по-русски, кратко и по существу. Не выдумывай учебные данные.
) > "%ROOT%\training\Modelfile"
echo Importing model into Ollama...
pushd "%ROOT%\training"
ollama create barney-lfm3 -f Modelfile
popd
if errorlevel 1 (echo [ERROR] Ollama import failed.& pause & goto MENU)
echo.
echo [OK] Model created. Test with: ollama run barney-lfm3
pause
goto MENU
