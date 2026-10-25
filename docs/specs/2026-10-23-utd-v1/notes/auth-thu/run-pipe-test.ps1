# THỬ: chạy yt-dlp.exe với --cookies \\.\pipe\<tên ngẫu nhiên> do cookiepipe.ps1 phục vụ.
param([string]$Url = "http://127.0.0.1:8766/file.mp4", [string]$Extra = "", [string]$Mode = "InOut")
$S = Split-Path -Parent $PSScriptRoot
$run = Join-Path $S "run"
$log = Join-Path $run ("pipe-" + [guid]::NewGuid().ToString("N").Substring(0, 6) + ".log")
$name = "utd-cookie-" + [guid]::NewGuid().ToString("N")
$cookies = Join-Path $run ($(if ($env:UTD_COOKIE_SRC) { $env:UTD_COOKIE_SRC } else { "c1.orig" }))
$job = Start-Job -ScriptBlock {
  param($script, $name, $cookies, $log, $mode)
  & $script -Name $name -CookieFile $cookies -Connections 4 -Log $log -Mode $mode
} -ArgumentList (Join-Path $PSScriptRoot "cookiepipe.ps1"), $name, $cookies, $log, $Mode
# Chờ pipe xuất hiện.
for ($i = 0; $i -lt 100; $i++) {
  if ([System.IO.Directory]::GetFiles("\\.\pipe\") -contains "\\.\pipe\$name") { break }
  Start-Sleep -Milliseconds 100
}
$yt = Join-Path $S "ytdlp-dl\yt-dlp.exe"
$out = Join-Path $run "pipe-out.json"
$err = Join-Path $run "pipe-err.txt"
$args = @("--no-warnings", "-J", "--cookies", "\\.\pipe\$name")
if ($Extra) { $args += $Extra.Split(" ") }
$args += $Url
$p = Start-Process -FilePath $yt -ArgumentList $args -NoNewWindow -Wait -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
"yt-dlp exit=$($p.ExitCode) (pid $($p.Id))"
"stderr:"; Get-Content $err -ErrorAction SilentlyContinue | Select-Object -First 15
Wait-Job $job -Timeout 20 | Out-Null
Stop-Job $job -ErrorAction SilentlyContinue
"pipe log:"; Get-Content $log -ErrorAction SilentlyContinue
"leftover file named like pipe in run dir:"; Get-ChildItem $run | Where-Object { $_.Name -like "*utd-cookie*" } | Select-Object Name
