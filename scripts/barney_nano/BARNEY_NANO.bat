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
cd /d "%ROOT%" || (echo [ERROR] Project folder not found.& pause & exit /b 1)

:MENU
cls
echo.
echo  ================================================================
echo.
echo       B A R N E Y - N A N O
echo       UNIVERSITY DISTILLATION LAB
echo.
echo  ================================================================
echo   Teacher : Ollama / LFM3
echo   Student : SmolLM2-360M-Instruct + LoRA
echo   Hardware: NVIDIA GPU / local training
echo  ----------------------------------------------------------------
echo.
echo   [1] Automatic installation / repair (Python, CUDA PyTorch, Ollama, models)
echo   [2] Hardware and dependencies check
echo   [3] Generate candidates with LFM3 teacher
echo   [4] Review / approve dataset
echo   [5] Train BARNEY-NANO
echo   [6] Evaluate trained adapter
echo   [7] Merge adapter for GGUF export
echo   [8] Open project folders
echo   [0] Exit
echo.
set "CHOICE="
set /p "CHOICE=   Select option ^> "

if "%CHOICE%"=="1" goto SETUP
if "%CHOICE%"=="2" goto CHECK
if "%CHOICE%"=="3" goto GENERATE
if "%CHOICE%"=="4" goto REVIEW
if "%CHOICE%"=="5" goto TRAIN
if "%CHOICE%"=="6" goto EVALUATE
if "%CHOICE%"=="7" goto MERGE
if "%CHOICE%"=="8" goto OPEN_FOLDERS
if "%CHOICE%"=="0" exit /b
goto MENU

:SETUP
cls
echo.
echo  [ BARNEY-NANO / AUTOMATIC INSTALLER ]
echo.
echo  This will install or repair:
echo   - Python 3.11 x64
echo   - Ollama
echo   - PyTorch with CUDA 12.8 wheels
echo   - BARNEY-NANO Python dependencies
echo   - SmolLM2 student model cache
echo   - LFM3 teacher model (if available on this Windows setup)
echo.
echo  NVIDIA driver updates are not forced automatically.
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%DIR%\install.ps1" -Root "%ROOT%"
if errorlevel 1 (
  echo.
  echo [ERROR] Installer finished with errors. Read the log in the project logs folder.
) else (
  echo.
  echo [OK] Installer finished. Run option 2 for diagnostics.
)
pause
goto MENU

:CHECK
cls
echo.
echo  [ BARNEY-NANO / SYSTEM DIAGNOSTICS ]
echo.
echo [1/3] NVIDIA GPU:
where nvidia-smi >nul 2>nul
if errorlevel 1 (
  echo [WARN] NVIDIA driver or nvidia-smi not found.
) else (
  nvidia-smi
)
echo.
echo ------------------------------------------------
echo [2/3] Python environment:
if exist "%PY%" (
  "%PY%" -c "import torch; print('PyTorch:',torch.__version__); print('CUDA:',torch.cuda.is_available()); print('GPU:',torch.cuda.get_device_name(0) if torch.cuda.is_available() else 'NOT DETECTED'); print('VRAM GB:',round(torch.cuda.get_device_properties(0).total_memory/1024**3,2) if torch.cuda.is_available() else 0)"
  "%PY%" -c "import transformers,peft; print('Transformers:',transformers.__version__); print('PEFT:',peft.__version__)"
) else (
  echo [WARN] Virtual environment not found. Run option 1.
)
echo.
echo ------------------------------------------------
echo [3/3] Ollama:
where ollama >nul 2>nul
if errorlevel 1 (
  echo [WARN] Ollama is unavailable.
) else (
  ollama list
)
echo.
pause
goto MENU

:GENERATE
cls
if not exist "%PY%" (echo Run option 1 first.& pause & goto MENU)
where ollama >nul 2>nul || (echo Ollama is unavailable. Run option 1 first.& pause & goto MENU)
set "COUNT="
set /p "COUNT=How many candidates? [20] "
if "%COUNT%"=="" set "COUNT=20"
"%PY%" "%DIR%\generate_teacher.py" --count %COUNT%
pause
goto MENU

:REVIEW
cls
if not exist "%PY%" (echo Run option 1 first.& pause & goto MENU)
if not exist "%RAW%" (echo Teacher dataset not found. Run option 3 first.& pause & goto MENU)
"%PY%" "%DIR%\review_dataset.py"
pause
goto MENU

:TRAIN
cls
if not exist "%PY%" (echo Run option 1 first.& pause & goto MENU)
if not exist "%APPROVED%" (echo Approved dataset not found. Run option 4 first.& pause & goto MENU)
"%PY%" -c "import torch,sys; sys.exit(0 if torch.cuda.is_available() else 1)"
if errorlevel 1 (echo [ERROR] CUDA GPU is not available. Run option 2 and fix the driver/PyTorch setup.& pause & goto MENU)
echo.
echo Start LoRA training on the NVIDIA GPU? Keep the laptop plugged in.
set "CONFIRM="
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

:OPEN_FOLDERS
if not exist "%ROOT%\data\barney_nano" mkdir "%ROOT%\data\barney_nano"
if not exist "%ROOT%\training" mkdir "%ROOT%\training"
start "" explorer "%ROOT%\data\barney_nano"
start "" explorer "%ROOT%\training"
goto MENU
