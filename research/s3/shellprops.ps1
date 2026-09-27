# Read Windows Property System values (what Explorer / Photos display) for a list of files.
# Input: UTF-8 text file with one absolute path per line. Output: UTF-8 JSON written to $OutFile.
param([string]$ListFile, [string]$OutFile)
$shell = New-Object -ComObject Shell.Application
$out = New-Object System.Collections.ArrayList
foreach ($line in [System.IO.File]::ReadAllLines($ListFile, [System.Text.Encoding]::UTF8)) {
    if (-not $line) { continue }
    $dir = [System.IO.Path]::GetDirectoryName($line)
    $name = [System.IO.Path]::GetFileName($line)
    $ns = $shell.Namespace($dir)
    $item = $ns.ParseName($name)
    $row = [ordered]@{ path = $line }
    foreach ($k in 'System.Author', 'System.Copyright', 'System.Photo.DateTaken', 'System.Title', 'System.Keywords') {
        $v = $item.ExtendedProperty($k)
        if ($v -is [array]) { $v = @($v | ForEach-Object { [string]$_ }) }
        elseif ($v -is [datetime]) { $v = $v.ToString('yyyy-MM-ddTHH:mm:ss') + ' (kind=' + $v.Kind + ')' }
        elseif ($null -ne $v) { $v = [string]$v }
        $row[$k] = $v
    }
    # the text Explorer shows in the "Date taken" column (index 12 on this system)
    $row['DetailsDateTaken'] = ($ns.GetDetailsOf($item, 12)).Replace([string][char]0x200E, '').Replace([string][char]0x200F, '')
    [void]$out.Add([pscustomobject]$row)
}
[System.IO.File]::WriteAllText($OutFile, (ConvertTo-Json -InputObject @($out) -Depth 4), (New-Object System.Text.UTF8Encoding($false)))
