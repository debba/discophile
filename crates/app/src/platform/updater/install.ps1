param([int]$ParentProcess, [string]$Target, [string]$Staged, [string]$Backup, [string]$Root, [string]$Failure)
$ErrorActionPreference = 'Stop'
$moved = $false
try {
    $process = Get-Process -Id $ParentProcess -ErrorAction SilentlyContinue
    if ($process -and !$process.WaitForExit(120000)) { throw 'Discophile did not exit. Nothing was installed.' }
    Move-Item -LiteralPath $Target -Destination $Backup
    $moved = $true
    Move-Item -LiteralPath $Staged -Destination $Target
    $child = Start-Process -FilePath $Target -WorkingDirectory (Split-Path -Parent $Target) -PassThru
    if ($child.WaitForExit(1000)) { throw 'The updated application exited before startup completed.' }
    Remove-Item -LiteralPath $Root -Recurse -Force -ErrorAction SilentlyContinue
} catch {
    $message = $_.Exception.Message
    if ($moved) {
        try {
            if (Test-Path -LiteralPath $Target) { Remove-Item -LiteralPath $Target -Force }
            Move-Item -LiteralPath $Backup -Destination $Target
            $message += ' The previous version was restored.'
        } catch { $message += " The previous version is preserved at: $Backup" }
    }
    [System.IO.File]::WriteAllText($Failure, $message)
    if (Test-Path -LiteralPath $Target) { Start-Process -FilePath $Target -WorkingDirectory (Split-Path -Parent $Target) }
    exit 1
}
