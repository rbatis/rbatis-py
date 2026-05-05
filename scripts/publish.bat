@echo off
REM 发布 rbatis-py 到 PyPI
REM
REM 用法:
REM   set MATURIN_PYPI_TOKEN=pypi-xxxx && scripts\publish.bat
REM   scripts\publish.bat pypi-xxxx

setlocal enabledelayedexpansion

set TOKEN=%MATURIN_PYPI_TOKEN%
if "%TOKEN%"=="" set TOKEN=%1

if "%TOKEN%"=="" (
    echo 错误: 未提供 PyPI API token
    echo.
    echo 用法:
    echo   set MATURIN_PYPI_TOKEN=pypi-xxxx ^&^& scripts\publish.bat
    echo   scripts\publish.bat pypi-xxxx
    echo.
    echo 获取 token: https://pypi.org/manage/account/token/
    exit /b 1
)

for /f "tokens=2 delims= " %%a in ('findstr /b "version = " Cargo.toml') do set VER=%%a
set VER=%VER:"=%
echo 发布 rbatis-py v%VER%

set MATURIN_PYPI_TOKEN=%TOKEN%
maturin publish --release
if %errorlevel% neq 0 exit /b %errorlevel%

echo ✅ 发布成功！https://pypi.org/project/rbatis-py/
