@echo off
setlocal EnableExtensions
chcp 65001 >nul
title BARNEY-NANO - University AI Lab
color 0B
set "ROOT=%~dp0..\.."
for %%I in ("%ROOT%") do set "ROOT=%%~fI"
set "VENV=%ROOT%\.venv-barney-nano"
set "PY=%VENV%\Scripts\python.exe"
set "DIR=%ROOT%\scripts\barney_nano"
set "RAW=%ROOT%\data\barney_nano\teacher_raw.jsonl"
set "APPROVED=%ROOT%\data\barney_nano\approved.jsonl"
set "ADAPTER=%ROOT%\training\barney-nano-adapter"
cd /d "%ROOT%"

:MENU
cls
echo.
echo  ================================================================
echo       ____    _    ____  _   _ _____ __   __
echo      ^| __ )  / \  ^|  _ \| \ ^| ^| ____^\ \ / /
echo      ^|  _ \ / _ \ ^| ^|_) ^|  \^| ^|  _^|   \ V /
echo      ^| ^|_) / ___ \^|  _ ^/^| ^|\  ^| ^|___   ^| ^
echo      ^|____/_/   \_\_^| \_\_^| \_^|_____^|  ^|_|
echo.
echo                  B A R N E Y - N A N O
echo             UNIVERSITY DISTILLATION LAB
echo  ----------------------------------------------------------------
echo   Teacher : Ollama / LFM3
echo   Student : SmolLM2-360M-Instruct + LoRA
echo   Hardware: RTX 4050 / 36 GB RAM
echo  ----------------------------------------------------------------
echo.
echo   [1] First-time setup
echo   [2] Hardware and dependencies check
echo   [3] Generate candidates with LFM3 teacher
echo   [4] Review / approve dataset
echo   [5] Train BARNEY-NANO
echo   [6] Evaluate trained adapter
echo   [7] Merge adapter for GGUF export
echo   [8] Open project folders
echo   [0] Exit
echo.
set /p "CHOICE=   Select option ^> "
if "%CHOICE%"=="1" goto SETUP
if "%CHOICE%"=="2" goto CHECK
if "%CHOICE%"=="3" goto GENERATE
if "%CHOICE%"=="4" goto REVIEW
if "%CHOICE%"=="5" goto TRAIN
if "%CHOICE%"=="6" goto EVALUATE
if "%CHOICE%"=="7" goto MERGE
if "%CHOICE%"=="8" (
  if not exist "%ROOT%\data\barney_nano" mkdir "%ROOT%\data\barney_nano"
  if not exist "%ROOT%\training" mkdir "%ROOT%\training"
  start "" "%ROOT%\data\barney_nano"
  start "" "%ROOT%\training"
  goto MENU
)
if "%CHOICE%"=="0" exit /b
goto MENU

:SETUP
cls
where py >nul 2>nul || (echo Install Python 3.11 x64 first.& pause & goto MENU)
if not exist "%PY%" py -3.11 -m venv "%VENV%"
if not exist "%PY%" (echo Could not create virtual environment.& pause & goto MENU)
call "%VENV%\Scripts\activate.bat"
python -m pip install --upgrade pip
python -c "import torch; print('PyTorch:',torch.__version__); print('CUDA:',torch.cuda.is_available())" 2>nul
if errorlevel 1 (
  echo Install PyTorch CUDA build in this venv first.
  start "" "https://pytorch.org/get-started/locally/"
  echo Choose Windows, Pip, Python, CUDA. Run the generated command with this venv active.
  pause
  goto MENU
)
pip install -r "%DIR%\requirements.txt"
if errorlevel 1 (echo Dependency installation failed.& pause & goto MENU)
echo [OK] Environment ready.
pause
goto MENU

:CHECK
cls
where nvidia-smi >nul 2>nul && nvidia-smi || echo [WARN] NVIDIA driver / nvidia-smi not found.
if exist "%PY%" (
  "%PY%" -c "import torch,transformers,peft; print('Torch:',torch.__version__); print('Transformers:',transformers.__version__); print('PEFT:',peft.__version__); print('CUDA:',torch.cuda.is_available()); print('GPU:',torch.cuda.get_device_name(0) if torch.cuda.is_available() else 'not detected'); print('VRAM GB:',round(torch.cuda.get_device_properties(0).total_memory/1024**3,2) if torch.cuda.is_available() else 0)"
) else echo [WARN] Run option 1 first.
where ollama >nul 2>nul && ollama list || echo [WARN] Ollama is unavailable.
pause
goto MENU

:GENERATE
cls
where ollama >nul 2>nul || (echo Install and start Ollama first.& pause & goto MENU)
if not exist "%PY%" (echo Run option 1 first.& pause & goto MENU)
set /p "COUNT=How many candidates? [20] "
if "%COUNT%"=="" set "COUNT=20"
"%PY%" "%DIR%\generate_teacher.py" --count %COUNT%
pause
goto MENU

:REVIEW
cls
if not exist "%PY%" (echo Run option 1 first.& pause & goto MENU)
"%PY%" "%DIR%\review_dataset.py"
pause
goto MENU

:TRAIN
cls
if not exist "%PY%" (echo Run option 1 first.& pause & goto MENU)
if not exist "%APPROVED%" (echo Approved dataset not found. Run option 4 first.& pause & goto MENU)
echo Start LoRA training on the RTX 4050? Keep the laptop plugged in.
set /p "CONFIRM=Continue [Y/N]? "
if /I not "%CONFIRM%"=="Y" goto MENU
"%PY%" "%DIR%\train_student.py" --data "%APPROVED%" --output "%ADAPTER%" --epochs 3 --batch-size 1 --grad-accum 16 --max-length 512 --rank 8
pause
goto MENU

:EVALUATE
cls
if not exist "%PY%" (echo Run option 1 first.& pause & goto MENU)
"%PY%" "%DIR%\evaluate_student.py" --adapter "%ADAPTER%"
pause
goto MENU

:MERGE
cls
if not exist "%PY%" (echo Run option 1 first.& pause & goto MENU)
"%PY%" "%DIR%\merge_adapter.py" --adapter "%ADAPTER%" --output "%ROOT%\training\barney-nano-merged"
pause
goto MENU
