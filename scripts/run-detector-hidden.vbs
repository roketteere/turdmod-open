Set objShell = CreateObject("WScript.Shell")
objShell.Run "powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File ""C:\Development\Claude\turdmod\scripts\scum-update-detector.ps1"" -AutoPatch", 0, True
