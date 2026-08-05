# Turnstile — Setup Verification Script
# Run this after installation to confirm everything is working.

param(
    [string]$ResponderUrl = "http://localhost:8080",
    [string]$EventId = "evt_001"
)

$errors = 0

Write-Host "`n=== Turnstile Setup Verification ===" -ForegroundColor Cyan

# Check ZeroClaw
Write-Host "`n[1] ZeroClaw binary..." -NoNewline
try {
    $ver = & zeroclaw --version 2>&1
    Write-Host " $ver" -ForegroundColor Green
} catch {
    Write-Host " NOT FOUND" -ForegroundColor Red
    $errors++
}

# Check responder health
Write-Host "[2] Responder health ($ResponderUrl)..." -NoNewline
try {
    $health = Invoke-WebRequest -Uri "$ResponderUrl/health" -Headers @{"ngrok-skip-browser-warning"="1"} -TimeoutSec 5 | ConvertFrom-Json
    if ($health.status -eq "ok") {
        Write-Host " OK" -ForegroundColor Green
    } else {
        Write-Host " UNHEALTHY" -ForegroundColor Red
        $errors++
    }
} catch {
    Write-Host " UNREACHABLE" -ForegroundColor Red
    $errors++
}

# Check actions.json
Write-Host "[3] actions.json..." -NoNewline
try {
    $actions = Invoke-WebRequest -Uri "$ResponderUrl/.well-known/actions.json" -Headers @{"ngrok-skip-browser-warning"="1"} -TimeoutSec 5 | ConvertFrom-Json
    if ($actions.rules) {
        Write-Host " OK" -ForegroundColor Green
    } else {
        Write-Host " MALFORMED" -ForegroundColor Red
        $errors++
    }
} catch {
    Write-Host " UNREACHABLE" -ForegroundColor Red
    $errors++
}

# Check enroll GET
Write-Host "[4] Enroll endpoint (GET)..." -NoNewline
try {
    $enroll = Invoke-WebRequest -Uri "$ResponderUrl/actions/enroll?event_id=$EventId" -Headers @{"ngrok-skip-browser-warning"="1"} -TimeoutSec 5
    if ($enroll.StatusCode -eq 200) {
        Write-Host " OK" -ForegroundColor Green
    } else {
        Write-Host " ERROR ($($enroll.StatusCode))" -ForegroundColor Red
        $errors++
    }
} catch {
    Write-Host " FAILED" -ForegroundColor Red
    $errors++
}

# Check TURNSTILE_RECIPIENT
Write-Host "[5] TURNSTILE_RECIPIENT env var..." -NoNewline
if ($env:TURNSTILE_RECIPIENT -and $env:TURNSTILE_RECIPIENT.Length -gt 30) {
    Write-Host " SET" -ForegroundColor Green
} else {
    Write-Host " NOT SET" -ForegroundColor Yellow
}

# Summary
Write-Host "`n=== Summary ===" -ForegroundColor Cyan
if ($errors -eq 0) {
    Write-Host "All checks passed. Turnstile is ready." -ForegroundColor Green
} else {
    Write-Host "$errors check(s) failed. See above for details." -ForegroundColor Red
}