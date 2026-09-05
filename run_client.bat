@echo off
setlocal
echo Building Dungeon Barrage client...
dotnet build client\src\DungeonBarrage.Client\DungeonBarrage.Client.csproj -c Debug --no-restore
if %ERRORLEVEL% NEQ 0 (
    echo Build failed!
    pause
    exit /b %ERRORLEVEL%
)
set GODOT_EXE=C:\Users\rsfit\AppData\Local\Microsoft\WinGet\Packages\GodotEngine.GodotEngine.Mono_Microsoft.Winget.Source_8wekyb3d8bbwe\Godot_v4.7.1-stable_mono_win64\Godot_v4.7.1-stable_mono_win64.exe
start "" "%GODOT_EXE%" --path "client\src\DungeonBarrage.Client"
