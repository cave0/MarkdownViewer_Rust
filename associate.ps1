<#
.SYNOPSIS
    Makes double-clicking .md / .markdown files open them in mdview.exe (current user only, no admin).
.PARAMETER Exe
    Path to mdview.exe. Defaults to .\dist\mdview.exe, then .\mdview.exe next to this script.
.PARAMETER Remove
    Undo the association.
#>
param([string]$Exe, [switch]$Remove)
$ErrorActionPreference = "Stop"
$progId = "MarkdownViewerRs.md"
$exts = ".md", ".markdown", ".mdown", ".mkd"

if ($Remove) {
    Remove-Item "HKCU:\Software\Classes\$progId" -Recurse -ErrorAction SilentlyContinue
    foreach ($e in $exts) {
        $k = "HKCU:\Software\Classes\$e"
        if ((Get-ItemProperty $k -ErrorAction SilentlyContinue).'(default)' -eq $progId) { Remove-Item $k -Recurse -ErrorAction SilentlyContinue }
    }
    Write-Host "Association removed."
} else {
    if (-not $Exe) {
        foreach ($c in "$PSScriptRoot\dist\mdview.exe", "$PSScriptRoot\mdview.exe") { if (Test-Path $c) { $Exe = $c; break } }
    }
    if (-not $Exe -or -not (Test-Path $Exe)) { Write-Error "mdview.exe not found. Build it first or pass -Exe." }
    $Exe = (Resolve-Path $Exe).Path
    New-Item "HKCU:\Software\Classes\$progId" -Force | Out-Null
    Set-ItemProperty "HKCU:\Software\Classes\$progId" -Name "(default)" -Value "Markdown document"
    New-Item "HKCU:\Software\Classes\$progId\DefaultIcon" -Force | Out-Null
    Set-ItemProperty "HKCU:\Software\Classes\$progId\DefaultIcon" -Name "(default)" -Value "`"$Exe`",0"
    New-Item "HKCU:\Software\Classes\$progId\shell\open\command" -Force | Out-Null
    Set-ItemProperty "HKCU:\Software\Classes\$progId\shell\open\command" -Name "(default)" -Value "`"$Exe`" `"%1`""
    foreach ($e in $exts) {
        New-Item "HKCU:\Software\Classes\$e" -Force | Out-Null
        Set-ItemProperty "HKCU:\Software\Classes\$e" -Name "(default)" -Value $progId
    }
    Write-Host "Associated $($exts -join ', ') with $Exe"
    Write-Host "If Windows still opens another app: right-click a .md file > Open with > Choose another app > mdview > Always."
}
Add-Type -Namespace Win32 -Name Shell -MemberDefinition '[DllImport("shell32.dll")] public static extern void SHChangeNotify(int e, int f, IntPtr a, IntPtr b);'
[Win32.Shell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
