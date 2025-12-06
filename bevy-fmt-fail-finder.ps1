# Fetch metadata only once so we can enumerate workspace crates.
$metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json

# Build a hash set of workspace member IDs for fast lookup.
$workspaceIds = [System.Collections.Generic.HashSet[string]]::new()
foreach ($member in $metadata.workspace_members) {
    $workspaceIds.Add($member) | Out-Null
}

$crateNames = @()
foreach ($pkg in $metadata.packages) {
    if ($workspaceIds.Contains($pkg.id)) {
        if (-not ($crateNames -contains $pkg.name)) {
            $crateNames += $pkg.name
        }
    }
}

Write-Host "Formatting $($crateNames.Count) workspace crates one by one"

$failures = @()
foreach ($name in $crateNames) {
    Write-Host "`n=== cargo fmt -p $name ==="
    $output = & cargo fmt -p $name 2>&1
    $outputText = ($output | Out-String).TrimEnd()
    $panicked = $outputText -match 'error: the compiler unexpectedly panicked\.'
    if ($LASTEXITCODE -eq 0 -and -not $panicked) {
        Write-Host "${name}: OK"
    }
    else {
        Write-Host "${name}: formatting failed (exit $LASTEXITCODE)"
        if ($panicked) {
            Write-Host 'Panic detected in output.'
        }
        if ($outputText) {
            Write-Host 'Command output:'
            Write-Host $outputText
        }
        $failures += $name
    }
}

Write-Host "`nCompleted. $($failures.Count) crate(s) failed formatting."
if ($failures.Count -gt 0) {
    Write-Host '`nFailures summary:'
    foreach ($name in $failures) {
        Write-Host "- $name"
    }
}