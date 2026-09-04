@echo off
setlocal
set GODOT_EXE=C:\Users\rsfit\AppData\Local\Microsoft\WinGet\Packages\GodotEngine.GodotEngine.Mono_Microsoft.Winget.Source_8wekyb3d8bbwe\Godot_v4.7.1-stable_mono_win64\Godot_v4.7.1-stable_mono_win64.exe
start "" "%GODOT_EXE%" --path "client\src\DungeonBarrage.Client"
