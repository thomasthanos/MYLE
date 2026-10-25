$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

try {
    $drive = [IO.Path]::GetPathRoot([Environment]::SystemDirectory).Substring(0, 1)
    [Console]::WriteLine("Scanning system drive ${drive}: online. This may take several minutes.")
    [Console]::WriteLine('Windows will report any issues. No offline repair or restart is requested.')
    $result = @(Repair-Volume -DriveLetter $drive -Scan -ErrorAction Stop)
    if ($result.Count -ne 1 -or $null -eq $result[0]) { throw 'Windows returned no usable disk scan result.' }
    $code = [int]$result[0]
    [Console]::WriteLine("Windows scan result: $($result[0]) ($code).")
    switch ($code) {
        { $_ -in 0, 4 } {
            [Console]::WriteLine('The disk scan completed. No file-system errors were found.')
            exit 0
        }
        { $_ -in 1, 2, 5, 16 } {
            [Console]::WriteLine('Windows reported online corrections. Review the disk scan result above.')
            exit 2
        }
        { $_ -in 6, 7, 13, 15 } {
            [Console]::WriteLine('Windows reported file-system problems. An offline repair may be needed.')
            exit 2
        }
        default {
            throw "Windows could not complete the disk scan (status $code). Wait for any existing scan to finish and try again."
        }
    }
} catch {
    [Console]::WriteLine($_.Exception.Message)
    exit 1
}
