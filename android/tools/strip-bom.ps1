# Strips a UTF-8 BOM from source files (the editor used to author them adds one).
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$bom = [byte[]](0xEF, 0xBB, 0xBF)
$fixed = 0
Get-ChildItem $root -Recurse -File -Include *.java, *.xml, *.gradle, *.properties, *.md, *.json, *.ps1 |
    Where-Object { $_.FullName -notmatch '\\build\\|\\.gradle\\' } |
    ForEach-Object {
        $bytes = [System.IO.File]::ReadAllBytes($_.FullName)
        if ($bytes.Length -ge 3 -and $bytes[0] -eq $bom[0] -and $bytes[1] -eq $bom[1] -and $bytes[2] -eq $bom[2]) {
            [System.IO.File]::WriteAllBytes($_.FullName, $bytes[3..($bytes.Length - 1)])
            $fixed++
        }
    }
Write-Host "stripped BOM from $fixed file(s)"
